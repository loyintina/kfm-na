//! offline_keys.rs — 断线输入暂存队列（2026-09-23 用户拍板「断联影响
//! 最小化」设计：断线期间敲键不打扰页面、不盲孵必死连接，连接好了
//! 卡住的输入自动补发；提示全部在断线状态卡上，终端正文零污染）。
//!
//! 收编条件（纯函数 should_hold）：活跃会话死了 + 是远程 + 隧道不可用
//! ——此刻重孵必 `Connection refused`（BAR-132 定罪的「反复跳」），
//! 击键进本队列等「隧道可用沿 → 重孵 → Opened」后回冲。本地会话与
//! 隧道可用时的远程死会话不吃这条（走原 kick_reconnect 路，conn 的
//! pending_input 会在 Opened 前代收——两条缓存轨不重叠，无重复触发）。
//!
//! 容量闸：64KB 封顶，超了丢最旧（丢旧保新，同 report 冲洗队列哲学），
//! 丢计数随队可查（上报用——静默丢输入 = 比丢更糟的事故）。

/// 暂存容量上限（字节）：够装断线期正常手速几分钟的击键，
/// 又不可能成为内存事故源
pub const CAP_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct OfflineKeys {
    queue: std::collections::VecDeque<String>,
    bytes: usize,
    dropped: usize,
    /// WAL 落盘路径（BAR-186 臂③）：None = 纯内存（旧行为）
    wal: Option<std::path::PathBuf>,
}

impl OfflineKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// 挂 WAL 落盘（BAR-186 臂③「进程死全灭」的修）：先读回已有文件
    /// （wal 未挂状态下逐条过 push 重建——容量闸/丢账同律，读回即合规），
    /// 再一次整写归齐，之后每次 push/drain 同步落盘。IO 失败一律静默
    /// 降级纯内存（缓存是加强不是命脉，BAR-174 同宪法）。编码 = 一行
    /// 一条 hex——击键含 \r/ESC/多字节，分隔符编码必须零歧义。
    pub fn attach_wal(&mut self, path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut recovered = Vec::new();
        if let Ok(body) = std::fs::read_to_string(path) {
            recovered = body.lines().filter_map(hex_dec).collect();
        }
        for s in recovered {
            self.push(s); // wal 未挂 = 纯内存 push，容量闸/丢账照常生效
        }
        self.wal = Some(path.to_path_buf());
        if !self.is_empty() && wal_rewrite(path, &self.queue).is_err() {
            self.wal = None; // 首写即失败 = 路径不可用，摘 WAL 降级纯内存
        }
    }

    pub fn wal_path(&self) -> Option<&std::path::Path> {
        self.wal.as_deref()
    }

    /// 收一条击键。超容量丢最旧（dropped 计数 +1 不静默）
    pub fn push(&mut self, input: String) {
        self.bytes += input.len();
        self.queue.push_back(input);
        let mut shrunk = false;
        while self.bytes > CAP_BYTES {
            if let Some(old) = self.queue.pop_front() {
                self.dropped += 1;
                self.bytes = self.bytes.saturating_sub(old.len());
                shrunk = true;
            } else {
                break;
            }
        }
        if let Some(p) = &self.wal {
            // 丢最旧发生 = 尾巴追加已失真，整文件重写（≤64KB，罕发）；
            // 否则增量追加一行
            let r = if shrunk {
                wal_rewrite(p, &self.queue)
            } else if let Some(last) = self.queue.back() {
                wal_append(p, last)
            } else {
                Ok(())
            };
            if r.is_err() {
                self.wal = None; // 落盘失败即摘 WAL——降级纯内存不反复撞 IO
            }
        }
    }

    /// 全量取出（保序）——Opened 时回冲用，取完即空
    pub fn drain(&mut self) -> Vec<String> {
        self.bytes = 0;
        if let Some(p) = &self.wal {
            let _ = std::fs::remove_file(p); // 回冲成功 = WAL 清账（路径保留，
            // 下次断线 push 照挂——drain 摘 wal 会让再断线退成纯内存）
        }
        self.queue.drain(..).collect()
    }

    pub fn pending_bytes(&self) -> usize {
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 累计丢最旧条数（不随 drain 清零——它是账不是态）
    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

/// 收编判定（纯函数，A 档）：只有「远程 + 死会话 + 隧道不可用」才暂存。
/// 本地会话死 = PTY 自己的事，与隧道无关；隧道可用 = 重孵能活，
/// 走 kick_reconnect + conn pending_input 原路
pub fn should_hold(session_over: bool, is_remote: bool, tunnel_usable: bool) -> bool {
    session_over && is_remote && !tunnel_usable
}

// ---- BAR-186 臂③：WAL 落盘助手（append-only，一行一条 hex）----

/// 追加一行（hex 编码的击键条目）
fn wal_append(path: &std::path::Path, input: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", hex_enc(input))
}

/// 整文件重写（丢最旧发生后队列与文件尾巴失真的归一锤）
fn wal_rewrite(
    path: &std::path::Path,
    queue: &std::collections::VecDeque<String>,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    for s in queue {
        writeln!(f, "{}", hex_enc(s))?;
    }
    Ok(())
}

fn hex_enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    out
}

/// hex 解码：坏行（奇数长/非法字符/非 UTF-8）返回 None——读回跳过不炸
fn hex_dec(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let bs = s.as_bytes();
    for pair in bs.chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        bytes.push(((hi << 4) | lo) as u8);
    }
    String::from_utf8(bytes).ok()
}
