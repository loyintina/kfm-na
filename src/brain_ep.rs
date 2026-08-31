//! brain_ep.rs — 脑插座（BrainEndpoint）与 echo-brain 夹具。
//!
//! 契约真相源：docs/active/ai-presence.md §四A trait 草案 / §三 数据源层。
//! 一个「脑」= 能开 run、吐四A 九事件流、可中断的后端；UI 面对接口编程，
//! 换脑（echo / direct-api / server）零改动。
//!
//! 线程模型：start/attach 返回 mpsc Receiver，脑自开线程推事件——
//! apply 瞬时返回契约（50ms 预算）要求网络/回放都不许堵 UI 线程。
//! 本模块只碰线程与 channel，不碰 socket（IO 归 direct-api-brain 插件）。

use crate::brain::{ChatEvent, events_from_upstream_sse};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

// ========== 请求与句柄 ==========

/// 开 run 请求（期 0 简版；roleFile/extraSystem 等 kfmv4 可选字段随用随加）。
#[derive(Debug, Clone)]
pub struct ChatStartReq {
    pub session_id: String,
    /// (role, content) 纯文本对，与 build_chat_request 同形
    pub messages: Vec<(String, String)>,
    pub model: String,
    pub provider: String,
    pub tools: Vec<String>,
}

/// run 句柄：取消/观测的同源读数。clone 便宜（Arc）。
pub struct RunHandle {
    pub id: u64,
    pub(crate) state: Arc<RunState>,
}

pub struct RunState {
    pub(crate) done: AtomicBool,
    pub(crate) cancelled: AtomicBool,
}

impl RunHandle {
    pub(crate) fn new(id: u64) -> Self {
        Self {
            id,
            state: Arc::new(RunState {
                done: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
            }),
        }
    }
    /// 流终结（自然播完或被取消）——UI 灭灯 / cancel 返 false 的依据。
    pub fn is_done(&self) -> bool {
        self.state.done.load(Ordering::Acquire)
    }
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }
}

// ========== 脑插座 ==========

/// 任何脑都长这个形状：开 run 吐事件、可中断、可重连。
/// direct-api-brain（期 0② 主力）/ echo-brain（夹具）/ server-brain（期 3 数据源）。
pub trait BrainEndpoint: Send + Sync {
    /// 开一轮对话，返回 run 句柄 + 事件流（四A 九事件，与上游无关）。
    fn start(&self, req: ChatStartReq) -> (RunHandle, Receiver<ChatEvent>);
    /// 中断 run（尽力而为；已终结的 run 返回 false——kfmv4 ok:false 语义）。
    fn cancel(&self, run: &RunHandle) -> bool;
    /// 重连接回：从事件游标回放+尾随。echo 支持（历史后缀回放）；
    /// direct-api 无缓冲可 None（期 0 接受，断线即重来）。
    fn attach(&self, run: &RunHandle, from: u64) -> Option<Receiver<ChatEvent>>;
}

// ========== echo-brain：考题夹具（零网络，回放 fixture 事件流） ==========

/// 回放固定节目单的假大脑。用途：期 0③ 对话页断网开发、协议层断网回归、
/// 双源对拍的本地基准。pace = 事件间隔（零 = 尽快；非零模拟流式节奏，
/// 也是取消考题能「抓到现行」的时间窗）。
pub struct EchoBrain {
    events: Arc<Vec<ChatEvent>>,
    pace: Duration,
    next_id: AtomicU64,
}

impl EchoBrain {
    pub fn new(events: Vec<ChatEvent>, pace: Duration) -> Self {
        Self {
            events: Arc::new(events),
            pace,
            next_id: AtomicU64::new(1),
        }
    }

    /// 从上游 SSE 全文造节目单（走真解析管，夹具与生产同路径）。
    pub fn from_upstream_sse(raw: &str, pace: Duration) -> Self {
        Self::new(events_from_upstream_sse(raw), pace)
    }

    fn spawn_replay(
        &self,
        from: u64,
        tx: Sender<ChatEvent>,
        state: Option<Arc<RunState>>,
        paced: bool,
    ) {
        let events = Arc::clone(&self.events);
        let pace = if paced { self.pace } else { Duration::ZERO };
        thread::spawn(move || {
            for ev in events.iter().skip(from as usize) {
                if let Some(st) = &state
                    && st.cancelled.load(Ordering::Acquire)
                {
                    // 四C 错误语义：用户取消 → error '已取消' 收尾
                    let _ = tx.send(ChatEvent::Error {
                        content: "已取消".to_string(),
                    });
                    break;
                }
                if pace > Duration::ZERO {
                    thread::sleep(pace);
                }
                if tx.send(ev.clone()).is_err() {
                    break; // 接收端已走（页面关了），静默退场
                }
            }
            if let Some(st) = &state {
                st.done.store(true, Ordering::Release);
            }
        });
    }
}

impl BrainEndpoint for EchoBrain {
    fn start(&self, _req: ChatStartReq) -> (RunHandle, Receiver<ChatEvent>) {
        let handle = RunHandle::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = channel();
        self.spawn_replay(0, tx, Some(Arc::clone(&handle.state)), true);
        (handle, rx)
    }

    fn cancel(&self, run: &RunHandle) -> bool {
        if run.is_done() {
            return false;
        }
        run.state.cancelled.store(true, Ordering::Release);
        true
    }

    fn attach(&self, _run: &RunHandle, from: u64) -> Option<Receiver<ChatEvent>> {
        // echo 的节目单全量在手：历史后缀回放，零节奏（重连求快），
        // 不吃取消标记（回放的是历史，不是活体）
        let (tx, rx) = channel();
        self.spawn_replay(from, tx, None, false);
        Some(rx)
    }
}
