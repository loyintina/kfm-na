//! gate — 调试闸门（2026-08-24 与用户定：看见 + 读懂 + 动手三件套）
//!
//! na 的画面是 Rust 软渲染、终端网格就在内存里、输入管线是 TermCmd——
//! 三者全在自己手里，所以调试闸门不需要 Android 截屏/注入权限，
//! 三条「文件即信号」通道直通（目录 = DUMP_DIR，8024 ssh 闸门可达）：
//!
//! | 触发文件  | 动作                           | 产出                |
//! |-----------|--------------------------------|---------------------|
//! | shot-req  | 锁终端离屏光栅化当前帧          | shot.rgb + shot.dim |
//! | text-req  | 锁终端导当前视野纯文本          | screen.txt          |
//! | keys-in   | 内容当裸字节发活跃会话 PTY      | 无（注入即消费）     |
//!
//! 服务器一键入口：scripts/na-shot.sh / na-text.sh / na-type.sh。
//!
//! 为什么全收在一条值守线程（2026-08-24 实拍证伪过弯路）：
//! 事件循环在 Activity 挂起态叫不醒——EventLoopProxy 的 send_event 送达
//! 但 winit 挂起分支不跑 about_to_wait（循环心跳停跳 130s+ 实锤）。
//! 所以闸门全部动作由独立线程 300ms 轮询执行，前台后台一个样，
//! 单消费者，无竞态。拍不到的：软键盘/系统弹窗（不在帧缓冲里，预期内）。
//!
//! 锁序约定（防死锁）：值守线程锁 term 或 router 都单独持有、互不嵌套；
//! UI 侧只有 term→router 方向（滚轮上报站点），无 router→term → 无环。
//!
//! keys-in 半写防护：写入端先写 keys-in.new 再 mv 成 keys-in（rename
//! 原子）；消费端 rename 到 keys-in.reading 再读再删（原子取走）。
//! 注入是裸字节——Ctrl-](\x1d)会话切换是 UI 层逻辑，闸门不过（有意留白）；
//! 会话死亡时 `SessionRouter::send` 静默吞（语义不动），闸门注入走
//! `send_checked` 回执，三环结果各留一行 gate 报告。
//!
//! ---- 会话泵（Output 数据面分家，2026-08-24 与用户定） ----
//!
//! 背景：挂起态事件循环不抽 event_rx，PTY 输出全堆在 mpsc 里，网格冻结
//! ——keys-in 注入真执行了，闸门读屏却读到旧画面（keys-in 修复案实证）。
//! 分家后 SessionPump 是会话入向的唯一消费者：
//! - 活跃方 Output：谁 pump 谁喂共享终端（UI 每圈 + 值守线程 300ms 双
//!   caller——前台零延迟，后台网格照新，闸门眼睛实时）；
//! - 待机方 Output：进 replay 缓存（切换时补屏，限量丢最旧）；
//! - 控制事件（Opened/Exited/Failed）：一粒不动，留给 UI 记健康账。
//!
//! 锁序追加：pump→term（sink 回调内单方向）；pump 与 router 互不嵌套。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::session::SessionEvent;

/// 闸门目录（na 沙箱 $PREFIX/tmp，调试闸门同机可见）
pub const DUMP_DIR: &str = "/data/data/dev.kfm.na/files/usr/tmp";

/// shot-req 触发文件在不在（轻量探测：值守线程每 300ms 看一眼，靠它决定
/// 要不要锁终端做一次全帧光栅化——没触发就一行 stat，零分配）
pub fn trigger_pending(dir: &str) -> bool {
    Path::new(dir).join("shot-req").exists()
}

// ---- 共享注册表（App 装配时登记；进程重启 = 全新注册） ----

/// 共享终端句柄（UI 线程与闸门值守线程各持一份）
pub type SharedTerm = Arc<Mutex<Box<dyn crate::termview::TermEmu>>>;
/// 共享输入路由句柄（keys-in 注入用；Arc 身份不变，切换/重连换内脏不影响）
pub type SharedRouter = Arc<Mutex<crate::session_router::SessionRouter>>;

static DUMP_TERM: Mutex<Option<SharedTerm>> = Mutex::new(None);
static GATE_ROUTER: Mutex<Option<SharedRouter>> = Mutex::new(None);
/// 最后帧尺寸 w<<32|h（draw_frame 每帧记账；0 = 还没画过，没尺寸可倒）
static DUMP_WH: AtomicU64 = AtomicU64::new(0);

/// 登记共享终端句柄（App 装终端时调一次）
pub fn register_dump_term(term: &SharedTerm) {
    *DUMP_TERM.lock().unwrap() = Some(term.clone());
}

/// 登记共享输入路由（App 装配 router 时调一次）
pub fn register_gate_router(router: &SharedRouter) {
    *GATE_ROUTER.lock().unwrap() = Some(router.clone());
}

/// draw_frame 每帧报尺寸（后台时没有 surface，尺寸只能来自这里）
pub fn note_frame_size(w: u32, h: u32) {
    DUMP_WH.store(((w as u64) << 32) | h as u64, Ordering::Relaxed);
}

// ---- 通道一：shot-req → 帧倒盘 ----

/// 倒一帧（有触发才干活）：锁终端离屏光栅化当前画面进 Vec 写出。
/// 注意：只画终端网格本体——快捷键行/放大镜是 UI 层装帧，后台调试
/// 要看的是终端内容，装帧状态反正已知（前台整帧由 draw_frame 负责）。
pub fn dump_now(dir: &str) {
    if !trigger_pending(dir) {
        return;
    }
    let term = DUMP_TERM.lock().unwrap().clone();
    let Some(term) = term else { return };
    let wh = DUMP_WH.load(Ordering::Relaxed);
    if wh == 0 {
        return; // 还没画过一帧,没尺寸可倒
    }
    let (w, h) = ((wh >> 32) as u32, (wh & 0xFFFF_FFFF) as u32);
    let mut buf = vec![0u32; (w as usize) * (h as usize)];
    term.lock().unwrap().render_into(&mut buf, w, h);
    maybe_dump(dir, &buf, w, h);
}

// ---- 通道二：text-req → 视野纯文本 ----

/// 倒文本（有触发才干活）：锁终端导当前视野纯文本写 screen.txt，
/// 倒完摘触发。返回是否真倒了。文件 IO 失败不致命（调试通道不拖垮终端）。
pub fn text_dump(dir: &str) -> bool {
    let trigger = Path::new(dir).join("text-req");
    if !trigger.exists() {
        return false;
    }
    let term = DUMP_TERM.lock().unwrap().clone();
    let Some(term) = term else { return false };
    let _ = std::fs::remove_file(&trigger);
    let text = term.lock().unwrap().dump_text();
    std::fs::write(Path::new(dir).join("screen.txt"), text).is_ok()
}

// ---- 通道三：keys-in → 裸字节注入活跃会话 ----

/// 原子取走 keys-in 的内容（rename 抢先，防写入端半写被读）：
/// 无文件/空内容 → None；有内容 → Some(原文) 且文件已消费
pub fn drain_keys_in(dir: &str) -> Option<String> {
    let pending = Path::new(dir).join("keys-in");
    let reading = Path::new(dir).join("keys-in.reading");
    if std::fs::rename(&pending, &reading).is_err() {
        return None;
    }
    let content = std::fs::read_to_string(&reading).ok();
    let _ = std::fs::remove_file(&reading);
    match content {
        Some(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// keys-in 在就取出注入活跃会话（裸字节 = 按键流；Ctrl 组合直接写控制字节）。
/// 注入全程上报（闸门判案纪律：drain/登记/send 哪环断了都得在报告里看得见）
pub fn inject_keys(dir: &str) {
    let Some(keys) = drain_keys_in(dir) else {
        return;
    };
    let len = keys.len();
    let router = GATE_ROUTER.lock().unwrap().clone();
    let Some(router) = router else {
        crate::report::report(
            "gate",
            &format!("keys-in {len}B 已取走但路由未登记——注入丢失"),
        );
        return;
    };
    let r = router.lock().unwrap();
    let alive = r.send_checked(crate::conn::TermCmd::Input(keys));
    crate::report::report(
        "gate",
        &format!(
            "keys-in {len}B 注入: 活跃={} 通道存活={alive}",
            r.active_name()
        ),
    );
}

// ---- 值守线程 ----

/// 起闸门值守线程（android_main 调一次）：300ms 一轮，泵 + 三通道各查一遍
pub fn spawn_gate_watcher() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            // 数据面泵：挂起态事件循环不抽，值守顶上进料——闸门眼睛看实时
            // 画面。锁序：注册表锁即取即还 → router 取名即还 → pump→term
            // （sink 回调内，单方向）。终端没建好就不抽（事件堆 mpsc,同旧制）
            let router = GATE_ROUTER.lock().unwrap().clone();
            let term = DUMP_TERM.lock().unwrap().clone();
            if let (Some(r), Some(t)) = (router, term) {
                let active = r.lock().unwrap().active_name();
                pump_once(active, &mut |b| t.lock().unwrap().feed(b));
            }
            dump_now(DUMP_DIR);
            text_dump(DUMP_DIR);
            inject_keys(DUMP_DIR);
        }
    });
}

// ---- 会话泵（Output 数据面与生命周期控制面分家） ----

/// replay 缓存帽（按名按字节）：挂起期待机话痨不许把内存吃穿
/// （旧制 mpsc 无界积压本身就是暗雷，分家顺手排掉）
pub const REPLAY_CAP_BYTES: usize = 256 * 1024;
/// 控制队列帽（挂起期生死事件有限；真爆了丢最旧保最新）
const CONTROL_CAP: usize = 256;

/// 会话泵——全部会话入向通道的唯一消费者（纯数据面，零静态依赖，
/// host 可判卷；生产实例挂模块静态 PUMP，UI 与值守线程双 caller）
pub struct SessionPump {
    /// (会话名, 入向通道)；同名 register = 换心脏（旧通道随旧 rx 一起 drop）
    slots: Vec<(&'static str, Receiver<SessionEvent>)>,
    /// 待机输出缓存：(名, 队列, 字节账)——会话数 ≤2,Vec 顺序扫够用
    replay: Vec<(&'static str, VecDeque<String>, usize)>,
    /// 控制事件队列（Opened/Exited/Failed 按名带进；UI 专属——健康牌/
    /// 重连归壳管，泵一粒不判）
    control: VecDeque<(&'static str, SessionEvent)>,
}

impl SessionPump {
    pub const fn new() -> Self {
        SessionPump {
            slots: Vec::new(),
            replay: Vec::new(),
            control: VecDeque::new(),
        }
    }

    /// 登记会话入向通道；同名 = 断线重连换心脏——旧通道遗物一粒不收，
    /// 该名 replay 一并清（旧 shell 遗物不喂新会话）
    pub fn register(&mut self, name: &'static str, rx: Receiver<SessionEvent>) {
        self.slots.retain(|(n, _)| *n != name);
        self.slots.push((name, rx));
        self.replay.retain(|(n, _, _)| *n != name);
    }

    /// 抽干一轮：活跃方 Output 喂 sink（返回 true = 喂过，调用方置 dirty）；
    /// 待机方 Output 进 replay（限量丢最旧）；控制事件进控制队列等 UI。
    /// 跨槽顺序不保证（每槽内 FIFO 保序）——会话间本无因果序
    pub fn pump(&mut self, active: &str, sink: &mut dyn FnMut(&[u8])) -> bool {
        // 先全槽抽干成批次（借 &self.slots)，再处理（要 &mut self)——
        // 一趟借完再改，不嵌套
        let mut batches: Vec<(&'static str, Vec<SessionEvent>)> = Vec::new();
        for (name, rx) in &self.slots {
            let mut b = Vec::new();
            while let Ok(ev) = rx.try_recv() {
                b.push(ev);
            }
            if !b.is_empty() {
                batches.push((*name, b));
            }
        }
        let mut fed = false;
        for (name, batch) in batches {
            for ev in batch {
                match ev {
                    SessionEvent::Output { data } if name == active => {
                        sink(data.as_bytes());
                        fed = true;
                    }
                    SessionEvent::Output { data } => self.push_replay(name, data),
                    ctl => {
                        if self.control.len() >= CONTROL_CAP {
                            self.control.pop_front();
                        }
                        self.control.push_back((name, ctl));
                    }
                }
            }
        }
        fed
    }

    /// 待机输出补屏料（切换时 UI 取）：取走即清，一次性
    pub fn take_replay(&mut self, name: &str) -> Vec<String> {
        let Some(i) = self.replay.iter().position(|(n, _, _)| *n == name) else {
            return Vec::new();
        };
        let entry = &mut self.replay[i];
        entry.2 = 0;
        entry.1.drain(..).collect()
    }

    /// 控制事件出队（UI 每圈取）：取走即清
    pub fn take_control(&mut self) -> Vec<(&'static str, SessionEvent)> {
        self.control.drain(..).collect()
    }

    fn push_replay(&mut self, name: &'static str, data: String) {
        let i = match self.replay.iter().position(|(n, _, _)| *n == name) {
            Some(i) => i,
            None => {
                self.replay.push((name, VecDeque::new(), 0));
                self.replay.len() - 1
            }
        };
        let entry = &mut self.replay[i];
        entry.2 += data.len();
        entry.1.push_back(data);
        while entry.2 > REPLAY_CAP_BYTES {
            match entry.1.pop_front() {
                Some(old) => entry.2 -= old.len(),
                None => {
                    entry.2 = 0;
                    break;
                }
            }
        }
    }
}

impl Default for SessionPump {
    fn default() -> Self {
        Self::new()
    }
}

// ---- 泵的生产实例（模块静态；UI 线程与值守线程双 caller) ----

static PUMP: Mutex<SessionPump> = Mutex::new(SessionPump::new());

/// 登记会话入向通道（装配/断线重连时壳调用）
pub fn pump_register(name: &'static str, rx: Receiver<SessionEvent>) {
    PUMP.lock().unwrap().register(name, rx);
}

/// 抽干一轮（壳每圈 + 值守线程 300ms)。active = 当时活跃会话名；
/// sink 收活跃方输出字节。返回 true = 喂过（调用方置 dirty)
pub fn pump_once(active: &str, sink: &mut dyn FnMut(&[u8])) -> bool {
    PUMP.lock().unwrap().pump(active, sink)
}

/// 控制事件出队（仅 UI 调）
pub fn pump_take_control() -> Vec<(&'static str, SessionEvent)> {
    PUMP.lock().unwrap().take_control()
}

/// 待机输出补屏料（仅 UI 切换时调）
pub fn pump_take_replay(name: &str) -> Vec<String> {
    PUMP.lock().unwrap().take_replay(name)
}

// ---- 纯函数（考题钉死层） ----

/// XRGB u32 帧缓冲 → 原始字节流（小端，平台统一 aarch64 LE）。
/// 每像素 4 字节，内存序 = B,G,R,X（0x00RRGGBB 的小端排布）。
pub fn encode_rgb(buf: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len() * 4);
    for px in buf {
        out.extend_from_slice(&px.to_le_bytes());
    }
    out
}

/// shot-req 触发文件在 → 倒一帧（单次触发单次倒，倒完摘触发）。
/// 返回是否真倒了。文件 IO 失败不致命（调试通道不拖垮渲染）。
pub fn maybe_dump(dir: &str, buf: &[u32], w: u32, h: u32) -> bool {
    let trigger = Path::new(dir).join("shot-req");
    if !trigger.exists() {
        return false;
    }
    let _ = std::fs::remove_file(&trigger);
    let rgb = encode_rgb(buf);
    if std::fs::write(Path::new(dir).join("shot.rgb"), rgb).is_err() {
        return false;
    }
    let _ = std::fs::write(Path::new(dir).join("shot.dim"), format!("{w} {h}"));
    true
}
