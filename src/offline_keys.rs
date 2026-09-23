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
}

impl OfflineKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// 收一条击键。超容量丢最旧（dropped 计数 +1 不静默）
    pub fn push(&mut self, input: String) {
        self.bytes += input.len();
        self.queue.push_back(input);
        while self.bytes > CAP_BYTES {
            if let Some(old) = self.queue.pop_front() {
                self.dropped += 1;
                self.bytes = self.bytes.saturating_sub(old.len());
            } else {
                break;
            }
        }
    }

    /// 全量取出（保序）——Opened 时回冲用，取完即空
    pub fn drain(&mut self) -> Vec<String> {
        self.bytes = 0;
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
