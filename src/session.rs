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
/// 自动重孵放行（A 档纯函数，2026-09-22 BAR-132「反复跳重连」定案）：
/// **远程会话必须隧道可用才许重孵**——隧道断着时它连 127.0.0.1:9021 必
/// `Connection refused`（用户侧看到的就是「反复跳」：05:47 那 30 秒里空转
/// 六次、每条都是同一句失败）；传输恢复自有「隧道可用沿」那条腿接力
/// （tunnel::usable_edge_kick），不怕漏。本地会话不吃这条（本机 PTY 与
/// 隧道无关）。放行后仍走原时间闸（auto_respawn_due）
pub fn auto_respawn_allowed(tunnel_usable: bool, is_remote: bool, due: bool) -> bool {
    due && (!is_remote || tunnel_usable)
}

pub fn auto_respawn_due(last_auto_respawn_ms: Option<u64>, now_ms: u64) -> bool {
    match last_auto_respawn_ms {
        None => true,
        Some(t) => now_ms.saturating_sub(t) >= MIN_AUTO_RESPAWN_MS,
    }
}

/// 预热池名单拉取退避基数（BAR-182，2026-09-29 redroid 实烧定罪）：对端
/// 死亡时「名单未落地先拉」臂（android_app::ctrl_ensure）每圈重发 List，
/// 重试节奏 = ws 握手失败延迟（redroid 实测 2.5Hz 永动、实烧 ~3 核，
/// logcat 「tmux 插件: 执行失败 连接失败」刷屏）——与 BAR-132 瞬死案同族，
/// 那边有时间闸这边漏了。指数退避：连败 n 次间隔 = min(BASE·2^(n-1), CAP)。
pub const LIST_RETRY_BASE_MS: u64 = 1_000;
/// 退避封顶（与预热池停放养档 30s 同尺）
pub const LIST_RETRY_CAP_MS: u64 = 30_000;

/// 名单拉取退避表（A 档纯函数，钉 tests/session_spec.rs）：
/// 连败 1/2/3/4/5/6+ 次 → 1s/2s/4s/8s/16s/30s 封顶。fails=0 防御按基数
/// 返回（正常路径首败即 1 起记）。shift 钳 15 位防溢出（先 min CAP 也
/// 拦住了，双保险）。成功或隧道可用沿由调用方把连败账归零。
pub fn list_retry_backoff_ms(fails: u32) -> u64 {
    let shift = fails.saturating_sub(1).min(15);
    (LIST_RETRY_BASE_MS << shift).min(LIST_RETRY_CAP_MS)
}
