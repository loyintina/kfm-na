//! state.rs — na-server 全局注册表（health 面数据源）
//!
//! 纯逻辑（health JSON 构造）与状态持有分离：build_health 是 A 档纯函数，
//! Registry 只是它的饲养员。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 一条会话的登记项
#[derive(Debug, Clone)]
pub struct SessionRec {
    pub id: String,
    /// 启动命令（交互 shell 则为 shell 路径）
    pub cmd: String,
    pub cols: u32,
    pub rows: u32,
    /// 距 epoch 的秒（跨进程可序列化；Instant 不能进 health）
    pub opened_epoch_s: u64,
    /// 最后活动（2026-09-20 修约「真空闲」：input/output 时 touch 刷新；
    /// idle_s 的唯一事实源——此前借 opened_epoch_s 充数 = 年龄冒充空闲）
    pub last_active_epoch_s: u64,
}

/// 全局注册表
pub struct Registry {
    started: Instant,
    started_epoch_s: u64,
    sessions: Mutex<HashMap<String, SessionRec>>,
    /// 活跃 ws 连接数（idle 自退判据之一）
    conns: AtomicUsize,
}

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Registry {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            started_epoch_s: now_epoch_s(),
            sessions: Mutex::new(HashMap::new()),
            conns: AtomicUsize::new(0),
        }
    }

    pub fn uptime_s(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub fn conn_open(&self) {
        self.conns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn conn_close(&self) {
        self.conns.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn conn_count(&self) -> usize {
        self.conns.load(Ordering::Relaxed)
    }

    pub fn register(&self, rec: SessionRec) {
        let _ = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(rec.id.clone(), rec);
    }

    pub fn unregister(&self, id: &str) {
        let _ = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    /// 刷新最后活动（wsterm 在 input/output 时调；不在册 = 零动作）
    pub fn touch(&self, id: &str) {
        if let Some(rec) = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(id)
        {
            rec.last_active_epoch_s = now_epoch_s();
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// health 面 JSON（A 档纯函数 build_health 的装配壳）
    pub fn health_json(&self) -> String {
        let sessions: Vec<SessionRec> = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        build_health(self.uptime_s(), self.started_epoch_s, &sessions).to_string()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// health 面 JSON 构造（A 档纯函数：形状的唯一事实源）
///
/// 形状（服务卡消费）：
/// ```json
/// {"uptime_s": 12, "started_epoch_s": 1, "sessions": [
///   {"id": "s1", "cmd": "tmux attach", "cols": 80, "rows": 24,
///    "alive": true, "idle_s": 3}
/// ]}
/// ```
pub fn build_health(
    uptime_s: u64,
    started_epoch_s: u64,
    sessions: &[SessionRec],
) -> serde_json::Value {
    let now = now_epoch_s();
    let list: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "cmd": s.cmd,
                "cols": s.cols,
                "rows": s.rows,
                "alive": true, // 在册即活（死会话先 unregister 再发 Exit）
                // 真空闲（2026-09-20 修约）：距最后活动，不是会话年龄
                "idle_s": now.saturating_sub(s.last_active_epoch_s),
            })
        })
        .collect();
    serde_json::json!({
        "uptime_s": uptime_s,
        "started_epoch_s": started_epoch_s,
        "sessions": list,
    })
}
