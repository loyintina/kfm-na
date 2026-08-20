//! build.rs — 编译期字体选择（BAR-021，2026-08-18）
//!
//! 规则（占位资产 + 本地覆盖）：
//! - `assets/fonts/local/main.ttf` 存在 → 主/CJK 字体都用它（本机商业字体，
//!   gitignore 钉死永不进库；它是等宽化+GB2312 子集化后的全功能像素字体）
//! - 否则落开源占位：主 = DejaVuSansMono.ttf，CJK = 缝合像素 12px 等宽
//!   GB2312 子集——任何克隆仓库的人编出的包行为一致
//!
//! 选中的文件拷进 OUT_DIR/fonts/{main,cjk}.ttf，termview.rs 用
//! `include_bytes!(concat!(env!("OUT_DIR"), ...))` 内嵌。源码树里不产生
//! 生成物，全新克隆开箱即编。

use std::path::{Path, PathBuf};

fn pick(name: &str, default: &Path, out_dir: &Path) {
    let local = Path::new("assets/fonts/local").join(name);
    // 监视集必须恒定：四条路径每次都声明——只在选中分支声明的话，
    // local/ 缺席时跑过一轮后 cargo 就不再监视它，覆盖字体改动静默失效
    // （2026-08-18 变异抽检实踩：local/main.ttf 换文件不触发重编）
    println!("cargo:rerun-if-changed={}", local.display());
    println!("cargo:rerun-if-changed={}", default.display());
    let src = if local.exists() { &local } else { default };
    // cargo::warning 让选用结果在编译输出可见（判卷：到底嵌了哪份字体）
    println!("cargo:warning=字体选择 {name} ← {}", src.display());
    std::fs::copy(src, out_dir.join(name))
        .unwrap_or_else(|e| panic!("字体备料失败 {} → {}: {e}", src.display(), name));
}

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("fonts");
    std::fs::create_dir_all(&out_dir).expect("OUT_DIR/fonts 建目录失败");
    pick(
        "main.ttf",
        Path::new("assets/fonts/DejaVuSansMono.ttf"),
        &out_dir,
    );
    // CJK/符号 fallback 链：local/cjk.ttf > 缝合像素子集（终端符号补丁包：
    // 盲文/方块/▽/powerline 全有；主字体缺的字形按 prefer_cjk 逐字路由给它。
    // 注意不再默认落 local/main.ttf——商业美术字体天然缺终端符号，
    // fallback 的职责就是补这个，BAR-022）
    pick(
        "cjk.ttf",
        Path::new("assets/fonts/FusionPixelMono12-gb2312.ttf"),
        &out_dir,
    );
}
