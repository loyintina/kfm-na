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
//! | ping-req  | 回一行 alive 报告（活性探测）    | 无（报告即回执）     |
//! | restart-req | 记遗言后 exit(0) 体面退出     | 无（Termux 侧拉回）  |
//! | trace-req | 行踪环全量落盘（report 流本地副本） | trace.txt        |
//! | stats-req | 运行时统计快照（帧/泵/闸门计数）  | stats-res          |
//! | orb-inject | AI 外显事件注入（tap/drag/run/end/dismiss） | orb-inject-res |
//!
//! restart-req 是热更新闭环的重启腿（2026-08-26 与用户定）：推完热更核心
//! 后要换进程才生效。值守线程见到触发文件即同步直报遗言、就地退出——
//! 不经过事件循环，挂起态也杀得死；进程没了由 Termux 侧 am start 拉回
//! （scripts/na-restart.sh 一键：触发→等死→拉回→等新 boot→ping 判卷）。
//! 若进程被 ROM 冻结（线程不进片，触发文件没人看），拉回时 android_main
//! 重跑，BAR-037 防御接住：遗言 + exit(0)，系统另起全新进程。
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    STAT_FRAMES.fetch_add(1, Ordering::Relaxed);
}

// ---- 通道一：shot-req → 帧倒盘 ----

/// 倒一帧（有触发才干活）：锁终端离屏光栅化当前画面进 Vec 写出。
/// 装帧口径：终端网格 + **AI 外显**（光球/AI 页占位）+ 常驻 chrome
/// （快捷键行/输入栏）——ai-presence 期 0 组件一起，球是「状态核读数
/// 画出来的」，值守线程自有句柄（D9 同源），na-shot 实拍是视觉判卷轨，
/// 倒帧不见球/栏 = 视觉轨瞎。输入栏读 input_bar_handle 快照（与 stats
/// 同源）；快捷键行修饰粘滞位不在共享态，倒帧恒按 mods=0 画（2026-08-31
/// 修订：此前行/栏都不画，输入栏样式排障被迫补这条眼；放大镜触点仍是
/// UI 私有，后台视野不含它）
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
    let ai_snap = ai_presence_handle().map(|ai| ai.snap(crate::report::boot_ms() as u64));
    let bar_snap = input_bar_handle().map(|bar| bar.snap());
    {
        let mut t = term.lock().unwrap();
        let ai_page = ai_snap.is_some_and(|s| s.page == crate::ai_presence::Page::AiFullscreen);
        // 当前栏带高（与 render_inputbar 同源实测折行——后台无 poll 写回，
        // 实测才不得两张皮）——keybar inset 与前台同尺
        let bar_h = bar_snap.as_ref().map_or(crate::input_bar::HEIGHT_PX, |bs| {
            crate::input_bar::height_for_lines(t.bar_text_lines(&bs.text, w))
        });
        if ai_page {
            // 与前台 rasterize 同一分支规则：AI 页 = 占位空壳盖掉终端网格
            // （AI 页不画快捷键行，同前台）
            t.render_ai_page(&mut buf, w, h);
        } else {
            t.render_into(&mut buf, w, h);
            // 快捷键行：前台同规则 inset 叠输入栏当前带高；修饰位无共享态按 0 画
            t.render_keybar(&mut buf, w, h, bar_h, 0);
        }
        // 输入栏：常驻 chrome，两页都画（同前台 rasterize 规则）；
        // sending 图标态跟 AI 运行态硬切；光标闪烁相位按节拍算
        // （dump 是快照，相位取倒帧那一刻，与前台同尺）
        if let Some(bs) = &bar_snap {
            let sending = ai_snap.is_some_and(|s| s.ai_running);
            let caret_on = (crate::report::boot_ms() as u64 / crate::input_bar::CARET_BLINK_MS)
                .is_multiple_of(2);
            t.render_inputbar(&mut buf, w, h, 0, bs, sending, caret_on);
        }
        if let Some(s) = ai_snap {
            let (gain, halo_gain) = crate::ai_presence::orb_gain(s.ai_running, s.pressed, s.page);
            t.render_orb(&mut buf, w, h, s.x, s.y, gain, halo_gain);
        }
    }
    if maybe_dump(dir, &buf, w, h) {
        STAT_SHOTS.fetch_add(1, Ordering::Relaxed);
    }
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
    let ok = std::fs::write(Path::new(dir).join("screen.txt"), text).is_ok();
    if ok {
        STAT_TEXTS.fetch_add(1, Ordering::Relaxed);
    }
    ok
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
    STAT_KEYS.fetch_add(1, Ordering::Relaxed);
    STAT_KEYS_BYTES.fetch_add(len as u64, Ordering::Relaxed);
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
    start_recorder(DUMP_DIR); // 飞行记录仪同生同灭
    std::thread::spawn(|| {
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            tick += 1;
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
            watch_loop(DUMP_DIR);
            restart_check(DUMP_DIR);
            trace_dump(DUMP_DIR);
            stats_answer(DUMP_DIR);
            touch_check(DUMP_DIR);
            switch_req_check(DUMP_DIR);
            orb_check(DUMP_DIR); // 通道十:AI 外显事件注入(直调状态核,落回执)
            bar_check(DUMP_DIR); // 通道十一:输入栏事件注入(直调状态核,落回执)
            alert_tick(tick);
            history_tick(DUMP_DIR, tick);
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
    /// rec = 飞行记录仪见证回调：一切 Output 全量带名经过它（与路由无关），
    /// 纯回调注入，泵自身零 IO 保持 host 可判卷。
    /// 跨槽顺序不保证（每槽内 FIFO 保序）——会话间本无因果序
    pub fn pump(
        &mut self,
        active: &str,
        sink: &mut dyn FnMut(&[u8]),
        rec: &mut dyn FnMut(&str, &[u8]),
    ) -> bool {
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
                        rec(name, data.as_bytes());
                        sink(data.as_bytes());
                        fed = true;
                    }
                    SessionEvent::Output { data } => {
                        rec(name, data.as_bytes());
                        self.push_replay(name, data);
                    }
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
/// sink 收活跃方输出字节。返回 true = 喂过（调用方置 dirty)。
/// rec 见证接飞行记录仪（入队即返回，IO 归记录仪线程）
pub fn pump_once(active: &str, sink: &mut dyn FnMut(&[u8])) -> bool {
    let mut nbytes = 0u64;
    let fed = PUMP.lock().unwrap().pump(
        active,
        &mut |b| {
            nbytes += b.len() as u64;
            sink(b);
        },
        &mut |name, bytes| {
            // 会话分桶字节账(自观测第三块):local/remote 各吞吐多少
            match name {
                "local" => STAT_BYTES_LOCAL.fetch_add(bytes.len() as u64, Ordering::Relaxed),
                "remote" => STAT_BYTES_REMOTE.fetch_add(bytes.len() as u64, Ordering::Relaxed),
                _ => STAT_BYTES_OTHER.fetch_add(bytes.len() as u64, Ordering::Relaxed),
            };
            rec_output(name, bytes)
        },
    );
    STAT_PUMP_CALLS.fetch_add(1, Ordering::Relaxed);
    STAT_PUMP_BYTES.fetch_add(nbytes, Ordering::Relaxed);
    fed
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

// ---- 飞行记录仪（2026-08-24 自观测·确定性回放，与用户定） ----
//
// 一切会话 Output 经泵的 rec 见证回调全量带名落带，resize 事件同带——
// host 回放器（src/bin/na-replay.rs）把字节流喂进同一台 TermView,
// 渲染现场不用碰手机就能复现。落盘 = 单文件 flight-rec.bin（时间线
// 保跨会话交错序,切换期的交互现场不丢）。
//
// 纪律:tap 入队即返回(零 IO 零分配压力可控),文件写/超帽压缩全在
// 记录仪线程;进程死在半条记录 = 截尾,解码端容忍(考题 2)。

/// 记录文件魔数（防拿错文件白分析）
pub const REC_MAGIC: &[u8] = b"KFMREC01\n";
/// 记录文件帽（超帽保新丢旧,单条超帽也留最新一条——那是案发点）
pub const REC_FILE_CAP: usize = 2 * 1024 * 1024;
/// 记录文件名（闸门目录下）
pub const REC_FILE: &str = "flight-rec.bin";
/// 上一世记录文件名（启动轮换的去处,坠机现场保全）
pub const REC_PREV_FILE: &str = "flight-rec.prev.bin";

/// 开机备带（可单测的纯文件操作）：旧带轮换成 .prev（坠机现场不丢），
/// 新带写魔数起新时间线。语义定案（BAR-034,2026-08-25 实拍）：记录带
/// 代表**当前进程**的屏幕真相,跨重启不追加——追加会让旧时间线的输出
/// 在回放时堆到新开机屏幕头顶,「回放末屏=读屏」判卷永远对不上
pub fn rec_boot_file(path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    if path.exists() {
        let prev = path.with_file_name(REC_PREV_FILE);
        std::fs::rename(path, prev)?; // 同目录改名,原子;旧 prev 被覆盖
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(REC_MAGIC)
}

/// 一条记录（解码侧 owned;ts_ms = 记录仪启动起的毫秒）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecEvent {
    Output {
        ts_ms: u64,
        name: String,
        data: Vec<u8>,
    },
    Resize {
        ts_ms: u64,
        name: String,
        cols: u32,
        rows: u32,
        cell_w: u32,
        cell_h: u32,
    },
}

impl RecEvent {
    pub fn ts_ms(&self) -> u64 {
        match self {
            RecEvent::Output { ts_ms, .. } | RecEvent::Resize { ts_ms, .. } => *ts_ms,
        }
    }
}

// 记录线格式:[u64 ts_ms][u8 kind][u8 name_len][name][u32 a][u32 b]
//             [u32 payload_len][payload]
// kind 1 = Output(a=b=0,payload=字节流);kind 2 = Resize(a=cols,b=rows,
// payload = 8B: cell_w u32 + cell_h u32)。未知 kind 按 payload_len 跳过
// (前向兼容);截尾(半条记录)安静丢弃。

/// 编码一条记录（纯函数,考题钉死）
pub fn rec_encode(ev: &RecEvent) -> Vec<u8> {
    let mut out = Vec::new();
    let (kind, name, a, b, payload): (u8, &str, u32, u32, Vec<u8>) = match ev {
        RecEvent::Output { name, data, .. } => (1, name.as_str(), 0, 0, data.clone()),
        RecEvent::Resize {
            name,
            cols,
            rows,
            cell_w,
            cell_h,
            ..
        } => {
            let mut p = Vec::with_capacity(8);
            p.extend_from_slice(&cell_w.to_le_bytes());
            p.extend_from_slice(&cell_h.to_le_bytes());
            (2, name.as_str(), *cols, *rows, p)
        }
    };
    out.extend_from_slice(&ev.ts_ms().to_le_bytes());
    out.push(kind);
    out.push(name.len().min(255) as u8);
    out.extend_from_slice(&name.as_bytes()[..name.len().min(255)]);
    out.extend_from_slice(&a.to_le_bytes());
    out.extend_from_slice(&b.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// 解码整条记录流（须带魔数;截尾容忍——最后半条安静丢;未知 kind 跳过）
pub fn rec_decode_all(buf: &[u8]) -> Result<Vec<RecEvent>, String> {
    if !buf.starts_with(REC_MAGIC) {
        return Err("坏魔数:不是记录仪文件".into());
    }
    let mut evs = Vec::new();
    let mut cur = &buf[REC_MAGIC.len()..];
    loop {
        // 固定头:8(ts)+1(kind)+1(nlen),nlen 后才知全长
        if cur.len() < 10 {
            break;
        }
        let ts_ms = u64::from_le_bytes(cur[..8].try_into().unwrap());
        let kind = cur[8];
        let nlen = cur[9] as usize;
        let rest = &cur[10..];
        if rest.len() < nlen + 12 {
            break; // 截尾:名字/定长段不全
        }
        let name = String::from_utf8_lossy(&rest[..nlen]).into_owned();
        let a = u32::from_le_bytes(rest[nlen..nlen + 4].try_into().unwrap());
        let b = u32::from_le_bytes(rest[nlen + 4..nlen + 8].try_into().unwrap());
        let plen = u32::from_le_bytes(rest[nlen + 8..nlen + 12].try_into().unwrap()) as usize;
        if rest.len() < nlen + 12 + plen {
            break; // 截尾:payload 不全
        }
        let payload = &rest[nlen + 12..nlen + 12 + plen];
        match kind {
            1 => evs.push(RecEvent::Output {
                ts_ms,
                name,
                data: payload.to_vec(),
            }),
            2 if plen == 8 => evs.push(RecEvent::Resize {
                ts_ms,
                name,
                cols: a,
                rows: b,
                cell_w: u32::from_le_bytes(payload[..4].try_into().unwrap()),
                cell_h: u32::from_le_bytes(payload[4..].try_into().unwrap()),
            }),
            _ => {} // 未知 kind:跳过
        }
        cur = &rest[nlen + 12 + plen..];
    }
    Ok(evs)
}

/// 超帽压缩（纯函数,考题钉死）:保新丢旧、魔数保留;单条超帽也留最新
/// 一条(宁爆帽不丢案发点)。帽内原样返回。输入坏魔数 = Err 原样上交。
pub fn rec_compact(buf: &[u8], cap: usize) -> Vec<u8> {
    if buf.len() <= cap {
        return buf.to_vec();
    }
    let Ok(evs) = rec_decode_all(buf) else {
        return buf.to_vec(); // 解不开的不动(压缩器不毁尸灭迹)
    };
    // 从最新往最旧攒,攒到帽沿停手(至少留最新一条)
    let mut kept: Vec<&RecEvent> = Vec::new();
    let mut budget = cap.saturating_sub(REC_MAGIC.len());
    for ev in evs.iter().rev() {
        let sz = rec_encode(ev).len();
        if sz > budget && !kept.is_empty() {
            break;
        }
        budget = budget.saturating_sub(sz);
        kept.push(ev);
    }
    kept.reverse();
    let mut out = REC_MAGIC.to_vec();
    for ev in kept {
        out.extend_from_slice(&rec_encode(ev));
    }
    out
}

// ---- 记录仪线程（生产面) ----

static REC_TX: Mutex<Option<std::sync::mpsc::Sender<RecEvent>>> = Mutex::new(None);
static REC_T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn rec_ts() -> u64 {
    REC_T0
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// 输出落带（泵 rec 见证回调经 pump_once 到这里）:入队即返回。
/// 记录仪没起(单测/早期) = 静默丢,同 report 铁律:观测通道不拖垮本体
pub fn rec_output(name: &str, data: &[u8]) {
    let tx = REC_TX.lock().unwrap().clone();
    if let Some(tx) = tx {
        let _ = tx.send(RecEvent::Output {
            ts_ms: rec_ts(),
            name: name.to_owned(),
            data: data.to_vec(),
        });
    }
}

/// 尺寸事件落带（壳 apply_window_size 调）:回放网格几何的锚点
pub fn rec_resize(name: &str, cols: u32, rows: u32, cell_w: u32, cell_h: u32) {
    let tx = REC_TX.lock().unwrap().clone();
    if let Some(tx) = tx {
        let _ = tx.send(RecEvent::Resize {
            ts_ms: rec_ts(),
            name: name.to_owned(),
            cols,
            rows,
            cell_w,
            cell_h,
        });
    }
}

/// 起记录仪线程（spawn_gate_watcher 调一次,幂等）
fn start_recorder(dir: &str) {
    let mut guard = REC_TX.lock().unwrap();
    if guard.is_some() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel::<RecEvent>();
    *guard = Some(tx);
    let path = std::path::PathBuf::from(dir).join(REC_FILE);
    std::thread::spawn(move || {
        use std::io::Write;
        // 开机轮换备带(BAR-034):旧带→.prev 保全,新带魔数起线;
        // 备带失败 = 丢记录不拖垮(观测通道铁律)
        if rec_boot_file(&path).is_err() {
            return;
        }
        loop {
            // 阻塞等第一条,再非阻塞排空(批量写,省 IO 次数)
            let Ok(first) = rx.recv() else { return };
            let mut batch = vec![first];
            while let Ok(ev) = rx.try_recv() {
                batch.push(ev);
            }
            let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) else {
                continue; // 文件打不开:丢批次不拖垮(观测通道铁律)
            };
            for ev in &batch {
                let _ = f.write_all(&rec_encode(ev));
            }
            drop(f);
            // 超帽压缩(读全量→保新丢旧→重写;2MB 级别,代价可忽略)
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.len() as usize > REC_FILE_CAP
                && let Ok(data) = std::fs::read(&path)
            {
                let _ = std::fs::write(&path, rec_compact(&data, REC_FILE_CAP));
            }
        }
    });
}

// ---- 死亡观测:panic 落盘 + loop 看门狗(2026-08-25 与用户定) ----
//
// 补的是自观测最后的瞎区:「它是怎么死的」。记录仪能答死前屏幕什么样
// (flight-rec.prev.bin 保全现场),但答不了为什么死——panic 信息原本
// 直接进 logcat 黑洞(线程 panic 更是无声无息),循环卡死/冬眠此前只能
// 靠人肉推理(blackout 案、BAR-029 冻结案都是这么硬查的)。
//
// 两路都守观测铁律:落盘失败静默、自身绝不许 panic、不拖垮本体。

/// panic 落盘文件(闸门目录,追加制——一世可能 panic 多次)
pub const PANIC_FILE: &str = "panic.log";
/// panic 时的行踪环落尾(覆写制——最新一案死前 64 行行踪)
pub const PANIC_TRACE_FILE: &str = "panic-trace.txt";
/// loop 看门狗档案(只在状态迁移时写,不刷屏)
pub const LOOP_STALL_FILE: &str = "loop-stall.log";
/// 心跳龄期阈值:重绘泵是降频轮询(WaitUntil 4ms,前台正常 ≥250 圈/s),
/// 超此即卡死/冬眠。看门狗因此可以纯被动——
/// 不需要 proxy 探针(proxy 挂起态本来就送不达,实锤过的弯路)
pub const LOOP_STALL_MS: u64 = 3_000;

static LOOP_BEAT_MS: AtomicU64 = AtomicU64::new(0); // 0 = 未起跳
static LOOP_BEAT_T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static WATCH_STALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// UI 循环每圈盖戳(android_app about_to_wait 首行调)
pub fn note_loop_beat() {
    let ms = LOOP_BEAT_T0
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64;
    LOOP_BEAT_MS.store(ms, Ordering::Relaxed);
}

/// 心跳龄期(None = 循环还没起跳过)
pub fn loop_beat_age_ms() -> Option<u64> {
    let beat = LOOP_BEAT_MS.load(Ordering::Relaxed);
    if beat == 0 {
        return None;
    }
    let now = LOOP_BEAT_T0.get()?.elapsed().as_millis() as u64;
    Some(now.saturating_sub(beat))
}

/// 卡死判定(纯函数,钉边界)
pub fn is_stall(age_ms: u64) -> bool {
    age_ms > LOOP_STALL_MS
}

/// 前台门控(BAR-036):Activity 挂起态 about_to_wait 合法停跳(闸门
/// 值守线程存在的理由就是这个),看门狗不认这个状态就每次退后台都
/// 误报 STALL——首装实拍即踩(退后台 5 分钟 beat_age=355s 报警)。
/// 壳在 resumed/suspended 喂这个状态
static APP_FOREGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn note_foreground(fg: bool) {
    APP_FOREGROUND.store(fg, Ordering::Relaxed);
}

/// 看门狗判决(纯函数,四态钉死)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchState {
    /// 挂起休假:退后台循环合法停跳,不判
    Background,
    /// 前台但循环从未盖戳(还没跑起来)
    NoBeat,
    Alive(u64),
    Stall(u64),
}

pub fn watch_verdict(foreground: bool, age_ms: Option<u64>) -> WatchState {
    if !foreground {
        return WatchState::Background;
    }
    match age_ms {
        None => WatchState::NoBeat,
        Some(a) if is_stall(a) => WatchState::Stall(a),
        Some(a) => WatchState::Alive(a),
    }
}

/// panic 档案行格式(纯函数,钉格式):unix 秒 + 线程名 + 位置 + 消息。
/// 消息内换行一律换成 ␤——一行一案,grep/awk 友好,不许撕成多行
pub fn panic_line(unix_secs: u64, thread: &str, loc: &str, msg: &str) -> String {
    format!(
        "unix={unix_secs} thread={thread} at={loc} msg={}",
        msg.replace('\n', "␤")
    )
}

/// 装 panic 钩子(android_main 挂,替换旧的「仅 report 异步直报」版):
/// ①落盘闸门目录(进程死了现场还在,8024 随时可查);②report 异步直报
/// (尽力而为——冲洗队列随进程死会丢,所以它只是补充不是主道);
/// ③链默认钩子(logcat 照走)。线程 panic 同样收——记录仪/值守线程
/// 若死,此前无声无息。三处失败都必须静默(观测铁律):hook 自己
/// panic = 进程直接 abort
pub fn install_panic_hook(dir: &str) {
    let default_hook = std::panic::take_hook();
    let path = std::path::PathBuf::from(dir).join(PANIC_FILE);
    let trace_path = std::path::PathBuf::from(dir).join(PANIC_TRACE_FILE);
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let tname = thread.name().unwrap_or("<无名>").to_owned();
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "-".into());
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<非串荷载>".into()
        };
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = panic_line(unix, &tname, &loc, &msg);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
        crate::report::report("panic", &line);
        // 行踪环落尾(2026-08-26 自观测第二块):panic 一行只答「死在哪」,
        // 环尾 64 行答「死前干了什么」——覆写制(只要最新一案的现场)
        let _ = std::fs::write(&trace_path, crate::trace::dump_tail(64));
        default_hook(info);
    }));
}

/// 值守每轮一查:①看门狗——心跳龄期过阈写迁移档(卡死/复活各一行,
/// 同步 report 让服务器实时看见);②ping 探测——ping-req 触发写
/// ping-res(8024 侧 na-ping.sh 随查随答)
fn watch_loop(dir: &str) {
    let fg = APP_FOREGROUND.load(Ordering::Relaxed);
    let state = watch_verdict(fg, loop_beat_age_ms());
    // 迁移档:只在进出 Stall 时写(Background 不算恢复,算休假销案)
    let stalled = matches!(state, WatchState::Stall(_));
    let was = WATCH_STALLED.swap(stalled, Ordering::Relaxed);
    if stalled != was {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = match state {
            WatchState::Stall(a) => format!("unix={unix} STALL beat_age={a}ms(前台)"),
            WatchState::Background => format!("unix={unix} SUSPEND(退后台,挂起休假销案)"),
            _ => format!("unix={unix} RECOVERED(前台恢复盖戳)"),
        };
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::path::PathBuf::from(dir).join(LOOP_STALL_FILE))
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
        crate::report::report("loop", &line);
    }

    let preq = std::path::PathBuf::from(dir).join("ping-req");
    if preq.exists() {
        std::fs::remove_file(&preq).ok();
        let verdict = match state {
            WatchState::Background => "background(挂起休假中,看门狗不判——循环停跳合法)".to_string(),
            WatchState::NoBeat => "loop 未起跳(前台但事件循环还没跑起来)".to_string(),
            WatchState::Stall(a) => format!("stall beat_age={a}ms(前台 >{LOOP_STALL_MS}ms,真卡死)"),
            WatchState::Alive(a) => format!("alive beat_age={a}ms(前台)"),
        };
        std::fs::write(std::path::PathBuf::from(dir).join("ping-res"), verdict).ok();
    }
}

// ---- 通道五：restart-req → 体面退出（热更闭环的重启腿） ----
/// restart-req 触发文件在 → 摘触发、同步直报遗言、exit(0)。
/// 从值守线程直接退进程，不经过事件循环——挂起态也杀得死。
/// 遗言必须 report_sync：exit(0) 不给异步入队留活路（同 BAR-022 教训）。
fn restart_check(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("restart-req");
    if !trigger.exists() {
        return;
    }
    std::fs::remove_file(&trigger).ok();
    crate::report::report_sync("death", "restart-req 收到,体面退出(等 Termux 拉回)");
    std::process::exit(0);
}

// ---- 自观测第二块:运行时统计随查(2026-08-26,配套 trace.rs) ----
//
// trace ring 答「发生了什么」(事件流),本块答「现在什么状态」(计数器)。
// 计数点散在各通道热路径(帧/泵/shot/text/keys),全是 AtomicU64 加一,
// 零锁零分配——观测铁律:不许反咬业务。

/// 帧计数(draw_frame 每帧 +1,降频泵下 ≈ 真实重绘活度计)
static STAT_FRAMES: AtomicU64 = AtomicU64::new(0);
/// 泵调用/喂字节累计(挂起期也走值守线程,所以数字一直会长)
static STAT_PUMP_CALLS: AtomicU64 = AtomicU64::new(0);
static STAT_PUMP_BYTES: AtomicU64 = AtomicU64::new(0);
/// 闸门动作计数(shots/texts/keys 各通道被用了几次、注了多少字节)
static STAT_SHOTS: AtomicU64 = AtomicU64::new(0);
static STAT_TEXTS: AtomicU64 = AtomicU64::new(0);
static STAT_KEYS: AtomicU64 = AtomicU64::new(0);
static STAT_KEYS_BYTES: AtomicU64 = AtomicU64::new(0);
/// 帧耗时画像:累计毫秒/峰值毫秒(note_draw,自观测第三块)
static STAT_DRAW_TOTAL_MS: AtomicU64 = AtomicU64::new(0);
static STAT_DRAW_MAX_MS: AtomicU64 = AtomicU64::new(0);
/// 会话分桶吞吐(泵 rec 回调按名记账)
static STAT_BYTES_LOCAL: AtomicU64 = AtomicU64::new(0);
static STAT_BYTES_REMOTE: AtomicU64 = AtomicU64::new(0);
static STAT_BYTES_OTHER: AtomicU64 = AtomicU64::new(0);
/// 会话死亡计数(重连/重孵频度 = 网络与 PTY 健康的温度计)
static STAT_SESSION_DEATHS: AtomicU64 = AtomicU64::new(0);

/// draw_frame 每帧报耗时(含 present)
pub fn note_draw(elapsed: std::time::Duration) {
    let ms = elapsed.as_millis() as u64;
    STAT_DRAW_TOTAL_MS.fetch_add(ms, Ordering::Relaxed);
    STAT_DRAW_MAX_MS.fetch_max(ms, Ordering::Relaxed);
}

/// 会话死亡/重孵计数(on_slot_dead 调)
pub fn note_session_death() {
    STAT_SESSION_DEATHS.fetch_add(1, Ordering::Relaxed);
}

/// /proc/self/stat 解析(纯函数,钉死):utime+stime 总 jiffies。
/// comm 字段可含空格/括号,必须从最后一个 ')' 之后切
pub fn parse_self_stat_jiffies(content: &str) -> Option<u64> {
    let after = content.rsplit_once(')')?.1;
    let f: Vec<&str> = after.split_whitespace().collect();
    // ')' 之后第 12/13 项 = utime/stime(原序号 14/15)
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    Some(utime + stime)
}

/// /proc/self/status 的 VmRSS 解析(纯函数):常驻内存 KB
pub fn parse_vmrss_kb(content: &str) -> Option<u64> {
    let line = content.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// 统计快照(纯数据,host 可判卷)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsSnap {
    pub uptime_ms: u128,
    pub foreground: bool,
    pub loop_age_ms: Option<u64>,
    pub frames: u64,
    pub pump_calls: u64,
    pub pump_bytes: u64,
    pub shots: u64,
    pub texts: u64,
    pub keys: u64,
    pub keys_bytes: u64,
    pub active: String,
    pub sessions: String,
    // ---- 自观测第三块:资源画像 ----
    /// 帧耗时:累计/峰值毫秒(均值由 format 侧算,防除零)
    pub draw_total_ms: u64,
    pub draw_max_ms: u64,
    /// CPU 占用(utime+stime jiffies,读 /proc/self/stat,失败 0)
    pub cpu_jiffies: u64,
    /// 常驻内存 KB(/proc/self/status VmRSS,失败 0)
    pub rss_kb: u64,
    /// 会话分桶吞吐(泵 rec 回调按名记账)
    pub bytes_local: u64,
    pub bytes_remote: u64,
    pub bytes_other: u64,
    /// 会话死亡/重孵累计
    pub session_deaths: u64,
    /// 触摸注入动作计数(通道八)
    pub touches: u64,
    // ---- ai_presence 字段族(2026-08-30,期 0 组件一,D9 机器轨) ----
    /// 页("terminal"/"ai";服务未登记 = "-")
    pub ai_page: String,
    pub ai_running: bool,
    pub ai_orb_x: i64,
    pub ai_orb_y: i64,
    pub ai_pressed: bool,
    pub ai_overlay: bool,
    // ---- input_bar 字段族(2026-08-31,期 0 组件三,D9 机器轨) ----
    /// 聚焦态(服务未登记 = false)
    pub bar_focused: bool,
    /// 栏内文本字符数
    pub bar_text_len: i64,
}

/// 拍一张当前快照(各静态即读即还;会话名单过 router 锁,取完即还)
pub fn stats_snap() -> StatsSnap {
    let (active, sessions) = match GATE_ROUTER.lock().unwrap().clone() {
        Some(r) => {
            let r = r.lock().unwrap();
            (r.active_name().to_owned(), r.names().join(","))
        }
        None => ("-".to_owned(), "-".to_owned()),
    };
    // 资源画像:/proc 读取失败静默给 0(观测铁律:不许反咬业务)
    let cpu_jiffies = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|c| parse_self_stat_jiffies(&c))
        .unwrap_or(0);
    let rss_kb = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|c| parse_vmrss_kb(&c))
        .unwrap_or(0);
    // AI 外显读数(期 0 组件一):未登记给中性值(观测铁律:不许反咬业务)。
    // now_ms 与运行侧同一把尺 = boot_ms
    let (ai_page, ai_running, ai_orb_x, ai_orb_y, ai_pressed, ai_overlay) =
        match ai_presence_handle() {
            Some(ai) => {
                let s = ai.snap(crate::report::boot_ms() as u64);
                (
                    match s.page {
                        crate::ai_presence::Page::Terminal => "terminal".to_owned(),
                        crate::ai_presence::Page::AiFullscreen => "ai".to_owned(),
                    },
                    s.ai_running,
                    s.x as i64,
                    s.y as i64,
                    s.pressed,
                    s.overlay_visible,
                )
            }
            None => ("-".to_owned(), false, 0, 0, false, false),
        };
    // 输入栏读数(期 0 组件三):未登记给中性值(观测铁律:不许反咬业务)
    let (bar_focused, bar_text_len) = match input_bar_handle() {
        Some(bar) => {
            let s = bar.snap();
            (s.focused, s.text.chars().count() as i64)
        }
        None => (false, 0),
    };
    StatsSnap {
        uptime_ms: crate::report::boot_ms(),
        foreground: APP_FOREGROUND.load(Ordering::Relaxed),
        loop_age_ms: loop_beat_age_ms(),
        frames: STAT_FRAMES.load(Ordering::Relaxed),
        pump_calls: STAT_PUMP_CALLS.load(Ordering::Relaxed),
        pump_bytes: STAT_PUMP_BYTES.load(Ordering::Relaxed),
        shots: STAT_SHOTS.load(Ordering::Relaxed),
        texts: STAT_TEXTS.load(Ordering::Relaxed),
        keys: STAT_KEYS.load(Ordering::Relaxed),
        keys_bytes: STAT_KEYS_BYTES.load(Ordering::Relaxed),
        active,
        sessions,
        draw_total_ms: STAT_DRAW_TOTAL_MS.load(Ordering::Relaxed),
        draw_max_ms: STAT_DRAW_MAX_MS.load(Ordering::Relaxed),
        cpu_jiffies,
        rss_kb,
        bytes_local: STAT_BYTES_LOCAL.load(Ordering::Relaxed),
        bytes_remote: STAT_BYTES_REMOTE.load(Ordering::Relaxed),
        bytes_other: STAT_BYTES_OTHER.load(Ordering::Relaxed),
        session_deaths: STAT_SESSION_DEATHS.load(Ordering::Relaxed),
        touches: STAT_TOUCHES.load(Ordering::Relaxed),
        ai_page,
        ai_running,
        ai_orb_x,
        ai_orb_y,
        ai_pressed,
        ai_overlay,
        bar_focused,
        bar_text_len,
    }
}

/// 格式化(纯函数,钉死行格式):key=value 一行一项,机器可读
pub fn format_stats(s: &StatsSnap) -> String {
    let age = s
        .loop_age_ms
        .map(|a| format!("{a}ms"))
        .unwrap_or_else(|| "未起跳".into());
    // 帧均耗防除零:一帧没画过就报 0
    let draw_avg = s.draw_total_ms.checked_div(s.frames).unwrap_or(0);
    format!(
        "uptime={}ms\nforeground={}\nloop_beat_age={}\nframes={}\npump_calls={}\npump_bytes={}\nshots={}\ntexts={}\nkeys={}\nkeys_bytes={}\ntouches={}\nactive={}\nsessions={}\ndraw_avg_ms={}\ndraw_max_ms={}\ncpu_jiffies={}\nrss_kb={}\nbytes_local={}\nbytes_remote={}\nbytes_other={}\nsession_deaths={}\nai_page={}\nai_running={}\nai_orb_x={}\nai_orb_y={}\nai_pressed={}\nai_overlay={}\nbar_focused={}\nbar_text_len={}\n",
        s.uptime_ms,
        s.foreground,
        age,
        s.frames,
        s.pump_calls,
        s.pump_bytes,
        s.shots,
        s.texts,
        s.keys,
        s.keys_bytes,
        s.touches,
        s.active,
        s.sessions,
        draw_avg,
        s.draw_max_ms,
        s.cpu_jiffies,
        s.rss_kb,
        s.bytes_local,
        s.bytes_remote,
        s.bytes_other,
        s.session_deaths,
        s.ai_page,
        s.ai_running,
        s.ai_orb_x,
        s.ai_orb_y,
        s.ai_pressed,
        s.ai_overlay,
        s.bar_focused,
        s.bar_text_len
    )
}

/// 通道六:trace-req → 行踪环全量落 trace.txt(覆写制,随查随新)
fn trace_dump(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("trace-req");
    if !trigger.exists() {
        return;
    }
    std::fs::remove_file(&trigger).ok();
    std::fs::write(
        std::path::PathBuf::from(dir).join("trace.txt"),
        crate::trace::dump_all(),
    )
    .ok();
}

/// 通道七:stats-req → 统计快照落 stats-res(同 ping-req 一问一答)
fn stats_answer(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("stats-req");
    if !trigger.exists() {
        return;
    }
    std::fs::remove_file(&trigger).ok();
    std::fs::write(
        std::path::PathBuf::from(dir).join("stats-res"),
        format_stats(&stats_snap()),
    )
    .ok();
}

// ---- 通道八:touch-in → 触摸注入(2026-08-27,观测矩阵输入侧空格销案) ----
//
// 闸门前三条输入通道(keys-in)只能注字节进 PTY;手势类 bug(滚动/选择/
// 快捷键行)没有复现腿,回回要用户当手。本通道把触摸事件参数化:
// host 写脚本行进 touch-in,值守线程解析入队,主循环抽干后与真手指
// 同一入口(handle_touch)——判卷尺与真实手势同一把。

/// 注入触摸指令(平台无关;android_app 侧映射 TouchPhase 双喂)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchCmd {
    /// 按下/移动/抬起(单指默认 id=90;多指捏合可显式给第二 id)
    Down {
        id: u64,
        x: f64,
        y: f64,
    },
    Move {
        id: u64,
        x: f64,
        y: f64,
    },
    Up {
        id: u64,
        x: f64,
        y: f64,
    },
    /// 点按(down+up 同点,不过阈值 → 走唤键盘/keybar 命中路径)
    Tap {
        x: f64,
        y: f64,
    },
    /// 滚屏语法糖:n>0 = 看历史(等效手指下扫 n 行,scroll.rs 契约:
    /// y 增大 = 正行数);由 App 侧按真实格高展开成 down/move/up 序列
    Scroll {
        lines: i32,
    },
    /// 脚本节拍:主循环挂起到点再取下一条(长按选择等时序手势用)
    Sleep {
        ms: u64,
    },
}

/// 注入指令队列(值守线程入,主循环出)
static TOUCH_IN: Mutex<VecDeque<TouchCmd>> = Mutex::new(VecDeque::new());
/// 注入动作计数(stats 闸门计数家族添丁)
static STAT_TOUCHES: AtomicU64 = AtomicU64::new(0);

/// 解析一行(纯函数,钉死)。None = 空行/注释跳过;Some(Err) = 坏行
pub fn parse_touch_line(line: &str) -> Option<Result<TouchCmd, String>> {
    let s = line.trim();
    if s.is_empty() || s.starts_with('#') {
        return None;
    }
    let tok: Vec<&str> = s.split_whitespace().collect();
    let bad = || Some(Err(format!("坏行(跳过): {s}")));
    let num = |i: usize| tok.get(i).and_then(|t| t.parse::<f64>().ok());
    match tok[0] {
        "tap" => {
            let (Some(x), Some(y)) = (num(1), num(2)) else {
                return bad();
            };
            Some(Ok(TouchCmd::Tap { x, y }))
        }
        "down" | "move" | "up" => {
            let (Some(x), Some(y)) = (num(1), num(2)) else {
                return bad();
            };
            let id = match tok.get(3) {
                Some(t) => match t.parse::<u64>() {
                    Ok(v) => v,
                    Err(_) => return bad(),
                },
                None => 90, // 注入默认指:90(与真手指 id 撞车概率≈0)
            };
            let cmd = match tok[0] {
                "down" => TouchCmd::Down { id, x, y },
                "move" => TouchCmd::Move { id, x, y },
                _ => TouchCmd::Up { id, x, y },
            };
            Some(Ok(cmd))
        }
        "scroll" => match tok.get(1).and_then(|t| t.parse::<i32>().ok()) {
            Some(lines) if lines != 0 => Some(Ok(TouchCmd::Scroll { lines })),
            _ => bad(),
        },
        "sleep" => match tok.get(1).and_then(|t| t.parse::<u64>().ok()) {
            // 封顶 10s:注入脚本写错不许把主循环节拍器焊死
            Some(ms) => Some(Ok(TouchCmd::Sleep { ms: ms.min(10_000) })),
            None => bad(),
        },
        _ => bad(),
    }
}

/// 解析整段脚本(纯函数):有效指令序列 + 坏行清单(空行/注释不计)
pub fn parse_touch_script(content: &str) -> (Vec<TouchCmd>, Vec<String>) {
    let mut cmds = Vec::new();
    let mut errs = Vec::new();
    for line in content.lines() {
        match parse_touch_line(line) {
            Some(Ok(c)) => cmds.push(c),
            Some(Err(e)) => errs.push(e),
            None => {}
        }
    }
    (cmds, errs)
}

/// 通道八值守:touch-in 存在即读走(读后即删),解析入队
fn touch_check(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("touch-in");
    if !trigger.exists() {
        return;
    }
    let content = std::fs::read_to_string(&trigger).unwrap_or_default();
    std::fs::remove_file(&trigger).ok();
    let (cmds, errs) = parse_touch_script(&content);
    let n = cmds.len();
    if n > 0 {
        TOUCH_IN.lock().unwrap().extend(cmds);
        STAT_TOUCHES.fetch_add(n as u64, Ordering::Relaxed);
    }
    if !errs.is_empty() {
        crate::report::report(
            "gate",
            &format!("touch-in 坏行 {} 条: {}", errs.len(), errs[0]),
        );
    }
}

/// 主循环抽干(仅 UI 调)
pub fn touch_take() -> Vec<TouchCmd> {
    TOUCH_IN.lock().unwrap().drain(..).collect()
}

// ---- 通道九:switch-req → 会话切换注入(2026-08-28,补 Ctrl-] UI 层缺口) ----
//
// Ctrl-] 切换是 UI 层拦截( 不落 PTY),keys-in 注不进来——观测矩阵
// 上最后一块登记在案的缺口。本通道给 switch_session()(与 Ctrl-] 完全
// 同一入口)配遥控器:v1 双会话只做「切换」开关,toggle 语义。
// 安全性:切换本身无损(两会话都活着,死的那个切入即自动重连)。

static SWITCH_REQ: AtomicBool = AtomicBool::new(false);

/// 值守:switch-req 文件存在即置标志(读后即删)
pub fn switch_req_check(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("switch-req");
    if trigger.exists() {
        std::fs::remove_file(&trigger).ok();
        SWITCH_REQ.store(true, Ordering::Relaxed);
    }
}

/// 主循环取走并清标志(true=请求一次切换;仅 UI 调,调 switch_session)
pub fn switch_take() -> bool {
    SWITCH_REQ.swap(false, Ordering::Relaxed)
}

// ---- 通道十:orb-inject → AI 外显事件注入(2026-08-30,ai-presence 期 0
// 组件一,规格书 ai-presence.md §八 D9 驱动轨) ----
//
// AI 外显状态核的遥控器:host 写脚本行进 orb-inject,值守线程解析后
// **直调 AiPresenceState 服务方法**——状态核 Sync 内部可变,人走触摸、
// AI 走服务/注入,同一状态核同一套考题(D9 同源),不需经主循环中转
// (通道八触摸注入要过 handle_touch 才借得到 UI 态,本通道无此需求)。
// 处理后落 orb-inject-res 回执(应用条数 + 事后快照),一问一答同
// stats-req 家族。判卷:na-stats.sh 看 ai_* 字段族翻转 + na-shot.sh 实拍。

/// AI 外显状态核服务句柄(App 装插件时登记;观测/注入同一份)
static AI_PRESENCE: Mutex<Option<Arc<crate::ai_presence::AiPresenceState>>> = Mutex::new(None);

/// 登记 AI 外显状态核(android_app init_terminal 装插件后调一次)
pub fn register_ai_presence(ai: &Arc<crate::ai_presence::AiPresenceState>) {
    *AI_PRESENCE.lock().unwrap() = Some(ai.clone());
}

/// 取句柄(owned Arc,借用即还——同 GATE_ROUTER 套路);未登记 = None
fn ai_presence_handle() -> Option<Arc<crate::ai_presence::AiPresenceState>> {
    AI_PRESENCE.lock().unwrap().clone()
}

/// 注入 orb 指令(平台无关;值守线程直调状态核服务方法)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrbCmd {
    /// 点球(终端 ↔ AI 全屏往返)
    Tap,
    /// 拖球到 (x, y)(状态核钳制)
    Drag { x: f64, y: f64 },
    /// 假跑 ms 毫秒(= 长按球的 debug 钩子同款 fake_run)
    Run { ms: u64 },
    /// 结束运行(run_end)
    End,
    /// 甩掉浮层(per-run dismissed)
    Dismiss,
}

/// 解析一行(纯函数,钉死)。None = 空行/注释跳过;Some(Err) = 坏行
pub fn parse_orb_line(line: &str) -> Option<Result<OrbCmd, String>> {
    let s = line.trim();
    if s.is_empty() || s.starts_with('#') {
        return None;
    }
    let tok: Vec<&str> = s.split_whitespace().collect();
    let bad = || Some(Err(format!("坏行(跳过): {s}")));
    let num = |i: usize| tok.get(i).and_then(|t| t.parse::<f64>().ok());
    match tok[0] {
        "tap" => Some(Ok(OrbCmd::Tap)),
        "end" => Some(Ok(OrbCmd::End)),
        "dismiss" => Some(Ok(OrbCmd::Dismiss)),
        "drag" => {
            let (Some(x), Some(y)) = (num(1), num(2)) else {
                return bad();
            };
            Some(Ok(OrbCmd::Drag { x, y }))
        }
        "run" => match tok.get(1).and_then(|t| t.parse::<u64>().ok()) {
            // 封顶 60s:注入脚本写错不许把灯焊亮
            Some(ms) => Some(Ok(OrbCmd::Run { ms: ms.min(60_000) })),
            None => bad(),
        },
        _ => bad(),
    }
}

/// 解析整段脚本(纯函数):有效指令序列 + 坏行清单(空行/注释不计)
pub fn parse_orb_script(content: &str) -> (Vec<OrbCmd>, Vec<String>) {
    let mut cmds = Vec::new();
    let mut errs = Vec::new();
    for line in content.lines() {
        match parse_orb_line(line) {
            Some(Ok(c)) => cmds.push(c),
            Some(Err(e)) => errs.push(e),
            None => {}
        }
    }
    (cmds, errs)
}

/// 通道十值守:orb-inject 存在即读走(读后即删),逐条直调状态核,
/// 落 orb-inject-res 回执(应用条数 + 坏行数 + 事后快照)
fn orb_check(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("orb-inject");
    if !trigger.exists() {
        return;
    }
    let content = std::fs::read_to_string(&trigger).unwrap_or_default();
    std::fs::remove_file(&trigger).ok();
    let (cmds, errs) = parse_orb_script(&content);
    let res = std::path::PathBuf::from(dir).join("orb-inject-res");
    let Some(ai) = ai_presence_handle() else {
        std::fs::write(&res, "error=ai-presence 服务未登记\n").ok();
        return;
    };
    let now = crate::report::boot_ms() as u64;
    for cmd in &cmds {
        match *cmd {
            OrbCmd::Tap => ai.tap_orb(),
            OrbCmd::Drag { x, y } => ai.drag_to(x, y),
            OrbCmd::Run { ms } => ai.fake_run(ms, now),
            OrbCmd::End => ai.run_end(now),
            OrbCmd::Dismiss => ai.dismiss_overlay(),
        }
    }
    let s = ai.snap(now);
    let page = match s.page {
        crate::ai_presence::Page::Terminal => "terminal",
        crate::ai_presence::Page::AiFullscreen => "ai",
    };
    std::fs::write(
        &res,
        format!(
            "applied={} bad={}\npage={page} running={} x={:.0} y={:.0} pressed={} overlay={}\n",
            cmds.len(),
            errs.len(),
            s.ai_running,
            s.x,
            s.y,
            s.pressed,
            s.overlay_visible
        ),
    )
    .ok();
    if !errs.is_empty() {
        crate::report::report(
            "gate",
            &format!("orb-inject 坏行 {} 条: {}", errs.len(), errs[0]),
        );
    }
}

// ---- 通道十一:bar-inject → 全局输入栏事件注入(2026-08-31,期 0 组件三,
// 规格书 ai-presence.md §二 常驻 chrome 一,D9 驱动轨同通道十先例) ----
//
// 输入栏状态核的遥控器:host 写脚本行进 bar-inject,值守线程解析后直调
// InputBarState 服务方法(状态核 Sync 内部可变,不经主循环)。submit 走
// 状态核的发送口(壳层装配的脑闭包)——触摸发送钮/IME Enter/本注入
// 三路同一路径(D9 同源)。处理后落 bar-inject-res 回执。

/// 输入栏状态核服务句柄(App 装插件时登记;观测/注入同一份)
static INPUT_BAR: Mutex<Option<Arc<crate::input_bar::InputBarState>>> = Mutex::new(None);

/// 登记输入栏状态核(android_app init_terminal 装插件后调一次)
pub fn register_input_bar(bar: &Arc<crate::input_bar::InputBarState>) {
    *INPUT_BAR.lock().unwrap() = Some(bar.clone());
}

/// 取句柄(owned Arc,借用即还);未登记 = None
fn input_bar_handle() -> Option<Arc<crate::input_bar::InputBarState>> {
    INPUT_BAR.lock().unwrap().clone()
}

/// 注入 bar 指令(平台无关;值守线程直调状态核服务方法)
#[derive(Debug, Clone, PartialEq)]
pub enum BarCmd {
    /// 聚焦(等同点文本区;弹键盘是壳层动作,注入不做)
    Focus,
    /// 失焦(等同点终端区/Esc)
    Unfocus,
    /// 追加文本(原样,含空格中文)
    Text(String),
    /// 退格一整字符
    Backspace,
    /// 清空栏
    Clear,
    /// 发送(= 点发送钮/Enter:取文推进发送口)
    Submit,
}

/// 解析一行(纯函数,钉死)。None = 空行/注释跳过;Some(Err) = 坏行
pub fn parse_bar_line(line: &str) -> Option<Result<BarCmd, String>> {
    let s = line.trim_end();
    let t = s.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let bad = || Some(Err(format!("坏行(跳过): {t}")));
    if let Some(rest) = t.strip_prefix("text ") {
        return Some(Ok(BarCmd::Text(rest.to_string()))); // 原文照收(空格保留)
    }
    match t {
        "focus" => Some(Ok(BarCmd::Focus)),
        "unfocus" => Some(Ok(BarCmd::Unfocus)),
        "backspace" => Some(Ok(BarCmd::Backspace)),
        "clear" => Some(Ok(BarCmd::Clear)),
        "submit" => Some(Ok(BarCmd::Submit)),
        "text" => bad(), // text 指令必须带内容
        _ => bad(),
    }
}

/// 解析整段脚本(纯函数):有效指令序列 + 坏行清单(空行/注释不计)
pub fn parse_bar_script(content: &str) -> (Vec<BarCmd>, Vec<String>) {
    let mut cmds = Vec::new();
    let mut errs = Vec::new();
    for line in content.lines() {
        match parse_bar_line(line) {
            Some(Ok(c)) => cmds.push(c),
            Some(Err(e)) => errs.push(e),
            None => {}
        }
    }
    (cmds, errs)
}

/// 通道十一值守:bar-inject 存在即读走(读后即删),逐条直调状态核,
/// 落 bar-inject-res 回执(应用条数 + 坏行数 + 事后快照)
fn bar_check(dir: &str) {
    let trigger = std::path::PathBuf::from(dir).join("bar-inject");
    if !trigger.exists() {
        return;
    }
    let content = std::fs::read_to_string(&trigger).unwrap_or_default();
    std::fs::remove_file(&trigger).ok();
    let (cmds, errs) = parse_bar_script(&content);
    let res = std::path::PathBuf::from(dir).join("bar-inject-res");
    let Some(bar) = input_bar_handle() else {
        std::fs::write(&res, "error=input-bar 服务未登记\n").ok();
        return;
    };
    for cmd in &cmds {
        match cmd {
            BarCmd::Focus => bar.focus(),
            BarCmd::Unfocus => bar.unfocus(),
            BarCmd::Text(t) => bar.insert_text(t),
            BarCmd::Backspace => bar.backspace(),
            BarCmd::Clear => bar.clear(),
            BarCmd::Submit => {
                let sent = bar.submit();
                crate::report::report("ai", &format!("bar-inject 发送: {sent:?}"));
            }
        }
    }
    let s = bar.snap();
    std::fs::write(
        &res,
        format!(
            "applied={} bad={}\nfocused={} text_len={} text={:?}\n",
            cmds.len(),
            errs.len(),
            s.focused,
            s.text.chars().count(),
            s.text
        ),
    )
    .ok();
    if !errs.is_empty() {
        crate::report::report(
            "gate",
            &format!("bar-inject 坏行 {} 条: {}", errs.len(), errs[0]),
        );
    }
}

// ---- 自观测第四块②:异常自报告警(2026-08-27) ----
//
// 前七块全是「人来查」:不查不知道。告警把方向反过来——值守线程每 3s
// 对快照过一遍规则,越线即 report("alert", ...) 自动进 trace 环 +
// field-reports.log,下次 na-trace.sh 顺手就看见。规则只报「该有人
// 看一眼」级别,误报宁可少(冷却与窗口全在 AlertState,纯函数可判卷)。

/// 帧耗时报警线(ms):超过且是新峰值才报(峰值单调爬,每爬一档报一次)
pub const ALERT_DRAW_MS: u64 = 100;
/// RSS 绝对线(KB):512MB,中低端机上这是「快被杀了」的水位
pub const ALERT_RSS_ABS_KB: u64 = 512 * 1024;
/// RSS 窗口净涨线(KB):5 分钟内涨 64MB = 泄漏嫌疑
pub const ALERT_RSS_GROW_KB: u64 = 64 * 1024;
/// RSS 窗口长(ms)
pub const ALERT_RSS_WINDOW_MS: u128 = 300_000;
/// RSS 报警冷却(ms):报一次歇 10 分钟,不刷屏
pub const ALERT_RSS_COOLDOWN_MS: u128 = 600_000;
/// 会话死亡窗口新增线:5 分钟内 ≥3 次 = 网络/PTY 在抽风
pub const ALERT_DEATHS_NEW: u64 = 3;
/// 死亡窗口长与冷却(ms):窗 5 分钟,报一次歇 5 分钟
pub const ALERT_DEATHS_WINDOW_MS: u128 = 300_000;
pub const ALERT_DEATHS_COOLDOWN_MS: u128 = 300_000;

/// 告警状态机(纯数据;生产实例挂静态,考题自己 new 一把)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertState {
    /// 已报过的帧耗时峰值(只有刷新峰值才再报)
    pub draw_peak_ms: u64,
    /// RSS 窗口基线:(窗起点 ms, 起点 rss_kb)
    pub rss_base: Option<(u128, u64)>,
    /// RSS 上次报警时刻(冷却用)
    pub rss_alerted_ms: Option<u128>,
    /// 死亡窗口基线:(窗起点 ms, 起点累计死亡数)
    pub deaths_base: Option<(u128, u64)>,
    /// 死亡上次报警时刻
    pub deaths_alerted_ms: Option<u128>,
}

impl AlertState {
    pub const fn new() -> Self {
        AlertState {
            draw_peak_ms: 0,
            rss_base: None,
            rss_alerted_ms: None,
            deaths_base: None,
            deaths_alerted_ms: None,
        }
    }
}

/// 告警判定(纯函数,钉死):吃快照 + 旧状态 + 当前时刻,
/// 出 (警报文案清单, 新状态)。now_ms 由调用方给(s.uptime_ms),
/// 考题传死数,全程确定性
pub fn alert_check(s: &StatsSnap, st: &AlertState, now_ms: u128) -> (Vec<String>, AlertState) {
    let mut out = Vec::new();
    let mut n = st.clone();
    // 规则一:帧耗新峰值
    if s.draw_max_ms > ALERT_DRAW_MS && s.draw_max_ms > st.draw_peak_ms {
        out.push(format!(
            "帧耗时新峰值 {}ms(线 {}ms)——卡顿体感可查 trace 环",
            s.draw_max_ms, ALERT_DRAW_MS
        ));
        n.draw_peak_ms = s.draw_max_ms;
    }
    // 规则二:RSS 绝线 / 窗口净涨(共用一个冷却)
    let rss_cooled = st
        .rss_alerted_ms
        .map(|t| now_ms.saturating_sub(t) >= ALERT_RSS_COOLDOWN_MS)
        .unwrap_or(true);
    if rss_cooled {
        if s.rss_kb > ALERT_RSS_ABS_KB {
            out.push(format!(
                "RSS {}MB 越过绝线 {}MB——内存水位高危",
                s.rss_kb / 1024,
                ALERT_RSS_ABS_KB / 1024
            ));
            n.rss_alerted_ms = Some(now_ms);
            n.rss_base = Some((now_ms, s.rss_kb));
        } else {
            match st.rss_base {
                // 窗口过期/未起:重置基线
                Some((t0, _)) if now_ms.saturating_sub(t0) >= ALERT_RSS_WINDOW_MS => {
                    n.rss_base = Some((now_ms, s.rss_kb));
                }
                Some((_, kb0)) if s.rss_kb.saturating_sub(kb0) > ALERT_RSS_GROW_KB => {
                    out.push(format!(
                        "RSS 5 分钟净涨 {}MB(线 {}MB)——泄漏嫌疑",
                        s.rss_kb.saturating_sub(kb0) / 1024,
                        ALERT_RSS_GROW_KB / 1024
                    ));
                    n.rss_alerted_ms = Some(now_ms);
                    n.rss_base = Some((now_ms, s.rss_kb));
                }
                None => n.rss_base = Some((now_ms, s.rss_kb)),
                _ => {}
            }
        }
    }
    // 规则三:会话死亡窗口新增 ≥3(独立冷却)
    let deaths_cooled = st
        .deaths_alerted_ms
        .map(|t| now_ms.saturating_sub(t) >= ALERT_DEATHS_COOLDOWN_MS)
        .unwrap_or(true);
    match st.deaths_base {
        Some((t0, _)) if now_ms.saturating_sub(t0) >= ALERT_DEATHS_WINDOW_MS => {
            n.deaths_base = Some((now_ms, s.session_deaths));
        }
        Some((_, d0))
            if deaths_cooled && s.session_deaths.saturating_sub(d0) >= ALERT_DEATHS_NEW =>
        {
            out.push(format!(
                "会话 5 分钟内死亡 {} 次——网络/PTY 在抽风",
                s.session_deaths.saturating_sub(d0)
            ));
            n.deaths_alerted_ms = Some(now_ms);
            n.deaths_base = Some((now_ms, s.session_deaths));
        }
        None => n.deaths_base = Some((now_ms, s.session_deaths)),
        _ => {}
    }
    (out, n)
}

/// 生产状态实例(值守线程专用)
static ALERT_STATE: Mutex<AlertState> = Mutex::new(AlertState::new());

/// 值守告警节拍:每 10 tick(≈3s)过一遍规则,越线走 report
/// (report 自动进 trace 环 + field-reports.log,双通道留痕)
fn alert_tick(tick: u64) {
    if !tick.is_multiple_of(10) {
        return;
    }
    let s = stats_snap();
    let now = s.uptime_ms;
    let msgs = {
        let mut st = ALERT_STATE.lock().unwrap();
        let (msgs, new_st) = alert_check(&s, &st, now);
        *st = new_st;
        msgs
    }; // 锁即取即还——report 另有自己的锁,不嵌套
    for m in msgs {
        crate::report::report("alert", &m);
    }
}

// ---- 自观测第四块③:stats 历史水位环(2026-08-27) ----
//
// stats_answer 答「现在」,本环答「这一路」——趋势类 bug(越来越慢/
// 内存爬坡)靠单点快照看不出坡,要一串。值守每 100 tick(≈30s)压一
// 张快照进环(帽 48 ≈ 24 分钟),通道九 history-req → history.txt
// 每张一行紧凑格式,na-history.sh 一拉就是一条曲线。

/// 水位环帽:48 张 × 30s ≈ 24 分钟回望窗
pub const HISTORY_CAP: usize = 48;
/// 压环节拍:每 100 tick(≈30s)一张
pub const HISTORY_EVERY_TICKS: u64 = 100;

static STATS_RING: Mutex<VecDeque<StatsSnap>> = Mutex::new(VecDeque::new());

/// 压环(纯函数,钉死推挤语义):满帽丢最旧
pub fn ring_push(ring: &mut VecDeque<StatsSnap>, s: StatsSnap) {
    if ring.len() >= HISTORY_CAP {
        ring.pop_front();
    }
    ring.push_back(s);
}

/// 历史行格式(纯函数,钉死):一张快照一行,空格分隔 key=value,
/// 直接可 awk 取列画曲线。均值防除零同 format_stats
pub fn format_history_line(s: &StatsSnap) -> String {
    let draw_avg = s.draw_total_ms.checked_div(s.frames).unwrap_or(0);
    format!(
        "t={} fg={} fr={} pump={} draw={}/{}ms cpu={} rss={}kb l={} r={} o={} d={} tch={} act={}",
        s.uptime_ms,
        u8::from(s.foreground),
        s.frames,
        s.pump_calls,
        draw_avg,
        s.draw_max_ms,
        s.cpu_jiffies,
        s.rss_kb,
        s.bytes_local,
        s.bytes_remote,
        s.bytes_other,
        s.session_deaths,
        s.touches,
        s.active
    )
}

/// 值守水位环节拍:到点压快照 + 应答通道九 history-req
fn history_tick(dir: &str, tick: u64) {
    if tick.is_multiple_of(HISTORY_EVERY_TICKS) {
        let s = stats_snap();
        ring_push(&mut STATS_RING.lock().unwrap(), s);
    }
    let trigger = std::path::PathBuf::from(dir).join("history-req");
    if !trigger.exists() {
        return;
    }
    std::fs::remove_file(&trigger).ok();
    let body = {
        let ring = STATS_RING.lock().unwrap();
        let mut out: Vec<String> = ring.iter().map(format_history_line).collect();
        if out.is_empty() {
            out.push("# (环空——启动未满 30s,稍候再查)".into());
        }
        out.join("\n") + "\n"
    };
    std::fs::write(std::path::PathBuf::from(dir).join("history.txt"), body).ok();
}
