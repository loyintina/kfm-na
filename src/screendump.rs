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

use std::path::Path;

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
