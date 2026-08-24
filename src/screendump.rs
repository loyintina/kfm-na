//! screendump — 画面回传（调试闸门配套，2026-08-24 与用户定）
//!
//! na 的画面是 Rust 软渲染进帧缓冲的，像素本来就在自己手里——截图
//! 不需要 Android 截屏权限，把帧缓冲原样倒出来就是图。链路：
//!
//!   调试侧(8024 闸门)`touch $PREFIX/tmp/shot-req`
//!     → 渲染循环下一帧发现触发文件 → 帧缓冲(XRGB u32)原样写
//!       `$PREFIX/tmp/shot.rgb` + 尺寸 `$PREFIX/tmp/shot.dim`(“w h”)
//!     → 调试侧 scp 拉回，服务器 PIL 转 PNG 查看
//!
//! 服务器一键入口：scripts/na-shot.sh(--watch 循环 = 近同步直播)。
//! 注意：软键盘/系统弹窗不在我们的帧缓冲里，拍不到(预期内)。
//!
//! 后台倒帧（2026-08-24 与用户定：截图不该要求应用在前台）：
//! 退后台/息屏后渲染泵歇工，draw_frame 不再跑。值守线程(唤醒锤二阶段)
//! 发现触发文件就 EventLoopProxy 锤醒事件循环，由 about_to_wait 把终端
//! 离屏光栅化进 Vec 倒出来——不依赖 surface，像素与前台的同一条
//! 光栅化路径（rasterize），只是少了呈现动作。

use std::path::Path;

/// 倒帧目录（na 沙箱 $PREFIX/tmp，调试闸门同机可见）
pub const DUMP_DIR: &str = "/data/data/dev.kfm.na/files/usr/tmp";

/// 触发文件在不在（轻量探测：值守线程每 300ms 看一眼，靠它决定要不要
/// 锁终端做一次全帧光栅化——没触发就一行 stat，零分配）
pub fn trigger_pending(dir: &str) -> bool {
    Path::new(dir).join("shot-req").exists()
}

// ---- 后台倒帧值守 ----
// 事件循环在 Activity 挂起态叫不醒（2026-08-24 实拍：EventLoopProxy 的
// send_event 送达,但 winit 挂起分支不跑 about_to_wait——循环心跳停跳、
// 触发文件晾着没人收）。所以倒帧的全部动作收进这条独立线程：轮询触发
// → 锁共享终端 → 离屏光栅化 → 写文件。前台后台一个样，单消费者。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 共享终端句柄（UI 线程与倒帧值守线程各持一份）
pub type SharedTerm = Arc<Mutex<Box<dyn crate::termview::TermEmu>>>;

/// 值守线程的终端句柄（App 装终端时登记；进程重启 = 全新注册）
static DUMP_TERM: Mutex<Option<SharedTerm>> = Mutex::new(None);
/// 最后帧尺寸 w<<32|h（draw_frame 每帧记账；0 = 还没画过，没尺寸可倒）
static DUMP_WH: AtomicU64 = AtomicU64::new(0);

/// 登记共享终端句柄（App 装终端时调一次）
pub fn register_dump_term(term: &SharedTerm) {
    *DUMP_TERM.lock().unwrap() = Some(term.clone());
}

/// draw_frame 每帧报尺寸（后台时没有 surface，尺寸只能来自这里）
pub fn note_frame_size(w: u32, h: u32) {
    DUMP_WH.store(((w as u64) << 32) | h as u64, Ordering::Relaxed);
}

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

/// 起值守线程（android_main 调一次）：300ms 一轮询，触发在就倒
pub fn spawn_dump_watcher() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            dump_now(DUMP_DIR);
        }
    });
}

/// XRGB u32 帧缓冲 → 原始字节流（小端，平台统一 aarch64 LE）。
/// 每像素 4 字节，内存序 = B,G,R,X（0x00RRGGBB 的小端排布）。
pub fn encode_rgb(buf: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len() * 4);
    for px in buf {
        out.extend_from_slice(&px.to_le_bytes());
    }
    out
}

/// 触发文件在 → 倒一帧（单次触发单次倒，倒完摘触发）。
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
