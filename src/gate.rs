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
    start_recorder(DUMP_DIR); // 飞行记录仪同生同灭
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
    PUMP.lock()
        .unwrap()
        .pump(active, sink, &mut |name, bytes| rec_output(name, bytes))
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
        // 新文件先写魔数;已有文件接着录(重启=新时间线,追加不截断——
        // 时间戳从 0 重计,回放器按魔数后 ts 回退点自知分段,v1 够用)
        if !path.exists()
            && let Ok(mut f) = std::fs::File::create(&path)
        {
            let _ = f.write_all(REC_MAGIC);
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
