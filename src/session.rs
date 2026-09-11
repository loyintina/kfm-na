//! session.rs — 单个终端会话的纯逻辑状态机（A 档考题 tests/session_spec.rs 的答案区）
//!
//! 职责：跟踪一次 terminal-open 会话的生命周期（Opening → Live → Exited/Failed），
//! 把服务端消息翻译成会话事件，并约束出向消息（未 opened 不许发 input/resize/close）。
//! 零 I/O——网络胶水在 conn.rs。
//!
//! 纪律：本文件是「答案」，只允许为通过考题而写；考题不许动。

use crate::protocol::{ClientMsg, ServerMsg};

/// 会话生命周期
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SessionState {
    /// 已发 terminal-open，等 terminal-opened
    #[default]
    Opening,
    /// 已绑定 sessionId，双向流通
    Live,
    /// 收到 terminal-exit（附退出码）
    Exited(i32),
    /// 收到 error 或解码层失败
    Failed(String),
}

/// 会话事件（喂给上层：渲染/上报）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Opened { session_id: String },
    Output { data: String },
    Exited { code: i32 },
    Failed { message: String },
}

#[derive(Default)]
pub struct Session {
    state: SessionState,
    session_id: Option<String>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 构造 terminal-open 出向消息（command = None 即交互 shell）
    pub fn open_msg(command: Option<&str>) -> ClientMsg {
        ClientMsg::Open {
            cwd: None,
            command: command.map(str::to_string),
            tag: None,
        }
    }

    /// 出向 input：仅 Live 态可发（带绑定的 sessionId）
    pub fn input_msg(&self, input: &str) -> Option<ClientMsg> {
        if self.state == SessionState::Live {
            self.session_id.as_ref().map(|id| ClientMsg::Input {
                session_id: id.clone(),
                input: input.to_string(),
            })
        } else {
            None
        }
    }

    /// 出向 resize：仅 Live 态可发
    pub fn resize_msg(&self, cols: u32, rows: u32) -> Option<ClientMsg> {
        if self.state == SessionState::Live {
            self.session_id.as_ref().map(|id| ClientMsg::Resize {
                session_id: id.clone(),
                cols,
                rows,
            })
        } else {
            None
        }
    }

    /// 出向 close：仅 Live 态可发
    pub fn close_msg(&self) -> Option<ClientMsg> {
        if self.state == SessionState::Live {
            self.session_id.as_ref().map(|id| ClientMsg::Close {
                session_id: id.clone(),
            })
        } else {
            None
        }
    }

    /// 喂一条服务端消息，产出事件（无关注释义为 None）并迁移状态
    pub fn on_server(&mut self, msg: ServerMsg) -> Option<SessionEvent> {
        // 终态（Exited/Failed）之后一律静默——迟到帧不改变结局
        if matches!(
            self.state,
            SessionState::Exited(_) | SessionState::Failed(_)
        ) {
            return None;
        }
        match msg {
            ServerMsg::Opened { session_id, .. } => {
                if self.session_id.is_some() {
                    return None; // 重复 Opened 容忍忽略
                }
                self.session_id = Some(session_id.clone());
                self.state = SessionState::Live;
                Some(SessionEvent::Opened { session_id })
            }
            ServerMsg::Output { session_id, data } => {
                if self.session_id.as_deref() == Some(session_id.as_str()) {
                    Some(SessionEvent::Output { data })
                } else {
                    None // 别的会话的 output / opened 前的 output：忽略
                }
            }
            ServerMsg::Exit { session_id, code } => {
                if self.session_id.as_deref() == Some(session_id.as_str()) {
                    self.state = SessionState::Exited(code);
                    Some(SessionEvent::Exited { code })
                } else {
                    None
                }
            }
            ServerMsg::Error { message } => {
                self.state = SessionState::Failed(message.clone());
                Some(SessionEvent::Failed { message })
            }
            // Ping/Unknown：协议层噪声，不升会话事件
            ServerMsg::Ping | ServerMsg::Unknown { .. } => None,
        }
    }
}

/// 自动重孵最小间隔（2026-09-11 redroid 瞬死案）：死亡驱动的自动重孵
/// 改纯时间闸节流。云安卓 local 会话「Opened 即 Failed」瞬死（aarch64
/// bootstrap 在 x86_64 链接即败）击穿旧「每剧集只自动重孵一次」的语义
/// ——每次 Opened 清牌就是新剧集新第一次，死亡↔重孵每帧一轮实烧
/// 2.5 核。时间闸不吃这套：首次（None）立即放行，其后距上次重孵
/// ≥ 本间隔才再放行。手动触发（敲键/切入死会话）不过此闸。
/// 调用方：android_app（壳；本模块宿主可编 = A 档考题可达）。
pub const MIN_AUTO_RESPAWN_MS: u64 = 5000;

/// 自动重孵闸门（纯函数，A 档考题 tests/session_spec.rs）：
/// 从未自动重孵过 → 立即放行；否则距上次够钟才放行。
/// 时钟回拨按 0 间隔处理 = 压住（saturating_sub，不透支不 panic）。
pub fn auto_respawn_due(last_auto_respawn_ms: Option<u64>, now_ms: u64) -> bool {
    match last_auto_respawn_ms {
        None => true,
        Some(t) => now_ms.saturating_sub(t) >= MIN_AUTO_RESPAWN_MS,
    }
}
