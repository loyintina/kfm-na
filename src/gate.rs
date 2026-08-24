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
//! 会话死亡时 send 静默吞，闸门不报错（text/shot 仍可读现场）。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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

/// keys-in 在就取出注入活跃会话（裸字节 = 按键流；Ctrl 组合直接写控制字节）
pub fn inject_keys(dir: &str) {
    let Some(keys) = drain_keys_in(dir) else {
        return;
    };
    let router = GATE_ROUTER.lock().unwrap().clone();
    let Some(router) = router else { return };
    router
        .lock()
        .unwrap()
        .send(crate::conn::TermCmd::Input(keys));
}

// ---- 值守线程 ----

/// 起闸门值守线程（android_main 调一次）：300ms 一轮，三通道各查一遍
pub fn spawn_gate_watcher() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            dump_now(DUMP_DIR);
            text_dump(DUMP_DIR);
            inject_keys(DUMP_DIR);
        }
    });
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
