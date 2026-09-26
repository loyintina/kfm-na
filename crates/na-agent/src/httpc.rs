//! httpc.rs — HTTPS 直连传输层（OpenAI 兼容端点，非流式 POST）。
//!
//! 形制照 src/direct_api_brain.rs + src/http1.rs（期 0② AI 脑直连的既有
//! 实现）：rustls ring 纯 Rust TLS + 手写 HTTP/1.1 子集，不引 reqwest。
//! 只实现 v1 需要的子集：POST 一次、读头、按 content-length/chunked/EOF
//! 读全量 body。平台无关（std::net + rustls，aarch64-linux-android 可编
//! 由 chain 第 6/11 步机械判卷）。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::dialect::{ChatClient, ChatReply, Message, ToolSpec, build_request, parse_response};
use crate::providers::Provider;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 读心跳：阻塞读周期醒来看总Deadline（ glm/deepseek 首 token 慢，
/// 但整轮 300s 封顶够 flash 级模型跑工具循环单轮）
const READ_TICK: Duration = Duration::from_millis(500);
const ROUND_TIMEOUT: Duration = Duration::from_secs(300);
/// 错误体/正常体截断上限（防爆内存；agent 响应远在量级内）
const BODY_CAP: usize = 4 * 1024 * 1024;

pub struct OpenAiClient {
    provider: Provider,
}

impl OpenAiClient {
    pub fn new(provider: Provider) -> Self {
        Self { provider }
    }

    /// 一轮非流式对话（阻塞）。返回 (status, body)。
    fn post(&self, body: &str) -> Result<(u16, String), String> {
        let (host, port, base_path) = parse_https_url(&self.provider.base_url)?;
        let addr = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("DNS 解析失败 {host}: {e}"))?
            .next()
            .ok_or_else(|| format!("DNS 无结果: {host}"))?;
        let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("连接失败 {host}:{port}: {e}"))?;
        tcp.set_read_timeout(Some(READ_TICK))
            .map_err(|e| e.to_string())?;
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| format!("坏主机名 {host}: {e}"))?;
        let conn = rustls::ClientConnection::new(Arc::new(config), server_name)
            .map_err(|e| format!("TLS 初始化失败: {e}"))?;
        let mut tls = rustls::StreamOwned::new(conn, tcp);

        let path = format!("{}/chat/completions", base_path.trim_end_matches('/'));
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.provider.api_key,
            body.len()
        );
        tls.write_all(req.as_bytes())
            .and_then(|_| tls.write_all(body.as_bytes()))
            .map_err(|e| format!("写请求失败: {e}"))?;

        let deadline = Instant::now() + ROUND_TIMEOUT;
        let mut r = BufRead::new(tls);
        let head = read_head(&mut r, deadline)?;
        let body_bytes = read_body(&mut r, &head, deadline)?;
        Ok((
            head.status,
            String::from_utf8_lossy(&body_bytes).into_owned(),
        ))
    }
}

impl ChatClient for OpenAiClient {
    fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
    ) -> Result<ChatReply, String> {
        let body = build_request(model, messages, tools);
        let (status, text) = self.post(&body)?;
        if status != 200 {
            // key 不落日志：错误体可能回显请求片段，截断到 512 字节
            let cap: String = text.chars().take(512).collect();
            return Err(format!("HTTP {status}: {cap}"));
        }
        parse_response(&text)
    }
}

/// https://host[:port]/base/path → (host, port, base_path)
pub fn parse_https_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("只支持 https: {url}"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| format!("坏端口: {authority}"))?,
        ),
        None => (authority.to_string(), 443),
    };
    Ok((host, port, path.to_string()))
}

// ---------- 手写 HTTP/1.1 响应读取（http1.rs 子集裁剪） ----------

struct BufRead {
    tls: rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    buf: std::collections::VecDeque<u8>,
}

impl BufRead {
    fn new(tls: rustls::StreamOwned<rustls::ClientConnection, TcpStream>) -> Self {
        Self {
            tls,
            buf: std::collections::VecDeque::new(),
        }
    }

    /// 读一块入缓冲；超时心跳先看 deadline。返回字节数（0=EOF）。
    fn fill(&mut self, deadline: Instant) -> Result<usize, String> {
        let mut tmp = [0u8; 8192];
        loop {
            return match self.tls.read(&mut tmp) {
                Ok(n) => {
                    self.buf.extend(&tmp[..n]);
                    Ok(n)
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if Instant::now() >= deadline {
                        Err("整轮响应超时（300s 封顶）".to_string())
                    } else {
                        continue;
                    }
                }
                Err(e) => Err(format!("读响应失败: {e}")),
            };
        }
    }

    fn read_line(&mut self, deadline: Instant) -> Result<Option<Vec<u8>>, String> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            if self.fill(deadline)? == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(self.buf.drain(..).collect()));
            }
        }
    }

    fn read_n(&mut self, mut n: usize, out: &mut Vec<u8>, deadline: Instant) -> Result<(), String> {
        while n > 0 {
            if self.buf.is_empty() && self.fill(deadline)? == 0 {
                return Err("body 提前 EOF".to_string());
            }
            let take = n.min(self.buf.len());
            out.extend(self.buf.drain(..take));
            n -= take;
        }
        Ok(())
    }
}

pub struct Head {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_head(r: &mut BufRead, deadline: Instant) -> Result<Head, String> {
    let status_line = r.read_line(deadline)?.ok_or_else(|| "空响应".to_string())?;
    let status_line = String::from_utf8_lossy(&status_line);
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("坏状态行: {status_line}"))?;
    let mut headers = Vec::new();
    while let Some(line) = r.read_line(deadline)? {
        if line.is_empty() {
            break;
        }
        let line = String::from_utf8_lossy(&line);
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(Head { status, headers })
}

fn read_body(r: &mut BufRead, head: &Head, deadline: Instant) -> Result<Vec<u8>, String> {
    let chunked = head
        .header("transfer-encoding")
        .is_some_and(|te| te.to_ascii_lowercase().contains("chunked"));
    let mut out = Vec::new();
    if chunked {
        loop {
            let line = r
                .read_line(deadline)?
                .ok_or_else(|| "chunk 尺寸行缺失".to_string())?;
            let line = String::from_utf8_lossy(&line);
            let hex = line.split(';').next().unwrap_or("").trim();
            let size =
                usize::from_str_radix(hex, 16).map_err(|_| format!("坏 chunk 尺寸: {line}"))?;
            if size == 0 {
                // trailer 区吃到空行
                while let Some(l) = r.read_line(deadline)? {
                    if l.is_empty() {
                        break;
                    }
                }
                break;
            }
            if out.len() + size > BODY_CAP {
                return Err("body 超 4MB 截断上限".to_string());
            }
            r.read_n(size, &mut out, deadline)?;
            r.read_line(deadline)?; // 块尾 CRLF
        }
        return Ok(out);
    }
    if let Some(cl) = head
        .header("content-length")
        .and_then(|c| c.trim().parse::<usize>().ok())
    {
        if cl > BODY_CAP {
            return Err("body 超 4MB 截断上限".to_string());
        }
        r.read_n(cl, &mut out, deadline)?;
        return Ok(out);
    }
    // EOF 形态：读到连接关闭
    loop {
        if r.buf.is_empty() && r.fill(deadline)? == 0 {
            break;
        }
        let take = r.buf.len().min(BODY_CAP - out.len());
        out.extend(r.buf.drain(..take));
        if out.len() >= BODY_CAP {
            return Err("body 超 4MB 截断上限".to_string());
        }
    }
    Ok(out)
}
