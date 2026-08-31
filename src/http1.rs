//! http1.rs — 手写 HTTP/1.1 客户端（direct-api-brain 的传输层）。
//!
//! 契约真相源：docs/active/ai-presence.md §四B「HTTP/1.1 手写」。
//! 为什么不引 reqwest/hyper：它们拖 hyper+http-body+tower 一整棵树，
//! 而我们要的只是「发个 POST、按块读 body」——SSE 的生命线是 chunked
//! 分帧的实时性，读到一个块就要立刻吐给上层，不许等全量。
//!
//! 只实现用到的子集：请求序列化 / 响应头解析 / chunked / content-length /
//! EOF 三种 body 形态。不支持：压缩、重定向、keep-alive 复用（一期不需要）。
//!
//! tick 钩子：带读超时的 socket 会周期性醒来（WouldBlock/TimedOut）——
//! 所有读取函数的 `_hook` 变体把 tick 吐给调用方钩子（取消检查用），
//! 钩子返回 Err 即中断读取。无参变体等价于永不中断（阻塞 socket 行为不变）。

use std::collections::VecDeque;
use std::io::{self, Read};

/// 读超时 tick 错误判定（rustls 透传底层 TcpStream 的超时）
pub fn is_tick_err(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

type TickHook<'a> = &'a mut dyn FnMut() -> io::Result<()>;

fn never_tick() -> impl FnMut() -> io::Result<()> {
    || Ok(())
}

// ========== 请求 ==========

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// 序列化为线上字节。自动补 Content-Length（调用方不给也要给，
/// 否则上游可能挂起等 body）。不补 Host——调用方必须自己给。
pub fn serialize_request(req: &Request) -> Vec<u8> {
    let mut out = format!("{} {} HTTP/1.1\r\n", req.method, req.path).into_bytes();
    let mut has_len = false;
    for (k, v) in &req.headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_len = true;
        }
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    if !has_len {
        out.extend_from_slice(format!("Content-Length: {}\r\n", req.body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&req.body);
    out
}

// ========== 响应头 ==========

#[derive(Debug, Clone)]
pub struct Head {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

/// body 形态：响应头决定怎么读 body。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Chunked,
    Len(u64),
    /// 无长度信息：读到连接关闭为止
    Eof,
}

impl Head {
    /// 大小写不敏感查表（HTTP 头名规范即如此）。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn body_kind(&self) -> BodyKind {
        if let Some(te) = self.header("transfer-encoding")
            && te.to_ascii_lowercase().contains("chunked")
        {
            return BodyKind::Chunked;
        }
        if let Some(cl) = self.header("content-length")
            && let Ok(n) = cl.trim().parse::<u64>()
        {
            return BodyKind::Len(n);
        }
        BodyKind::Eof
    }
}

// ========== 缓冲读（自带缓冲，read_head 预读的字节不丢） ==========

pub struct BufIo<R> {
    inner: R,
    buf: VecDeque<u8>,
}

impl<R: Read> BufIo<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: VecDeque::new(),
        }
    }

    /// 底层直读一次入缓冲；返回读到字节数（0=EOF）。tick 错误原样上抛。
    fn fill(&mut self) -> io::Result<usize> {
        let mut tmp = [0u8; 8192];
        let n = self.inner.read(&mut tmp)?;
        self.buf.extend(&tmp[..n]);
        Ok(n)
    }

    /// tick 容忍版 fill：超时醒来先过钩子（取消检查），再续读。
    fn fill_hook(&mut self, on_tick: TickHook) -> io::Result<usize> {
        loop {
            match self.fill() {
                Err(e) if is_tick_err(&e) => on_tick()?,
                other => return other,
            }
        }
    }

    /// 读一行（不含 \n，兼容 CRLF 去 \r）。EOF 且无残留 → Ok(None)。
    fn read_line_hook(&mut self, on_tick: TickHook) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop(); // \n
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            if self.fill_hook(on_tick)? == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                // EOF 残留：整段当最后一行
                return Ok(Some(self.buf.drain(..).collect()));
            }
        }
    }

    fn read_line_str_hook(&mut self, on_tick: TickHook) -> io::Result<Option<String>> {
        Ok(self
            .read_line_hook(on_tick)?
            .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    fn read_some_hook(&mut self, out: &mut [u8], on_tick: TickHook) -> io::Result<usize> {
        if self.buf.is_empty() && self.fill_hook(on_tick)? == 0 {
            return Ok(0);
        }
        let n = out.len().min(self.buf.len());
        for slot in out.iter_mut().take(n) {
            *slot = self.buf.pop_front().expect("buf 长度已检查");
        }
        Ok(n)
    }

    pub fn read_head(&mut self) -> io::Result<Head> {
        self.read_head_hook(&mut never_tick())
    }

    pub fn read_head_hook(&mut self, mut on_tick: TickHook) -> io::Result<Head> {
        let status_line = self
            .read_line_str_hook(&mut on_tick)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "空响应"))?;
        // "HTTP/1.1 200 OK" — 取第二段状态码
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("坏状态行: {status_line}"),
                )
            })?;
        let mut headers = Vec::new();
        while let Some(line) = self.read_line_str_hook(&mut on_tick)? {
            if line.is_empty() {
                break; // 头体分界空行
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
            // 无冒号的畸形头行：容忍跳过（真实服务器偶尔会发）
        }
        Ok(Head { status, headers })
    }

    pub fn body_reader(self, kind: BodyKind) -> BodyReader<R> {
        BodyReader {
            io: self,
            state: match kind {
                BodyKind::Chunked => BodyState::ChunkSize,
                BodyKind::Len(n) => BodyState::Len(n),
                BodyKind::Eof => BodyState::Eof,
            },
        }
    }
}

// ========== body 读取（状态机，碎喂安全） ==========

enum BodyState {
    Len(u64),
    Eof,
    /// 下一块尺寸行；内部值 = 当前块剩余字节
    ChunkSize,
    ChunkData(u64),
    /// 块数据后的 CRLF
    ChunkCrlf,
    /// 0 块之后的 trailer 区：吃到空行为止
    Trailers,
    Done,
}

pub struct BodyReader<R> {
    io: BufIo<R>,
    state: BodyState,
}

impl<R: Read> BodyReader<R> {
    /// 读一段 body 到 out。Ok(0) = body 终结（chunked 的 0 块 / 长度读完 / EOF）。
    pub fn read_body(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.read_body_hook(out, &mut never_tick())
    }

    /// tick 容忍版：每次底层超时醒来先过钩子。
    pub fn read_body_hook(&mut self, out: &mut [u8], mut on_tick: TickHook) -> io::Result<usize> {
        loop {
            match &mut self.state {
                BodyState::Done => return Ok(0),
                BodyState::Len(0) => {
                    self.state = BodyState::Done;
                    return Ok(0);
                }
                BodyState::Len(remain) => {
                    let n = out.len().min(*remain as usize);
                    if n == 0 {
                        return Ok(0);
                    }
                    let got = read_exact_or_eof(&mut self.io, &mut out[..n], &mut on_tick)?;
                    *remain -= got as u64;
                    return Ok(got);
                }
                BodyState::Eof => return self.io.read_some_hook(out, &mut on_tick),
                BodyState::ChunkSize => {
                    let line = self.io.read_line_str_hook(&mut on_tick)?.ok_or_else(|| {
                        io::Error::new(io::ErrorKind::UnexpectedEof, "chunk 尺寸行缺失")
                    })?;
                    // 尺寸行 = 十六进制 [; 扩展忽略]
                    let hex = line.split(';').next().unwrap_or("").trim();
                    let size = u64::from_str_radix(hex, 16).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, format!("坏 chunk 尺寸: {line}"))
                    })?;
                    self.state = if size == 0 {
                        BodyState::Trailers
                    } else {
                        BodyState::ChunkData(size)
                    };
                }
                BodyState::ChunkData(0) => self.state = BodyState::ChunkCrlf,
                BodyState::ChunkData(remain) => {
                    let n = out.len().min(*remain as usize);
                    if n == 0 {
                        return Ok(0);
                    }
                    let got = read_exact_or_eof(&mut self.io, &mut out[..n], &mut on_tick)?;
                    *remain -= got as u64;
                    return Ok(got);
                }
                BodyState::ChunkCrlf => {
                    let _ = self.io.read_line_hook(&mut on_tick)?; // 块尾 CRLF
                    self.state = BodyState::ChunkSize;
                }
                BodyState::Trailers => {
                    match self.io.read_line_hook(&mut on_tick)? {
                        None => self.state = BodyState::Done,
                        Some(l) if l.is_empty() => self.state = BodyState::Done,
                        Some(_) => {} // trailer 字段：吃掉
                    }
                }
            }
        }
    }
}

/// 尽力读满 out；底层 EOF 提前来了就把已读的返回（调用方按短读处理）。
fn read_exact_or_eof<R: Read>(
    io: &mut BufIo<R>,
    out: &mut [u8],
    on_tick: TickHook,
) -> io::Result<usize> {
    let mut done = 0;
    while done < out.len() {
        let n = io.read_some_hook(&mut out[done..], on_tick)?;
        if n == 0 {
            break;
        }
        done += n;
    }
    Ok(done)
}
