//! 解析页壳内容 inset 同源考题（BAR-120 源码守卫，A 档）：2026-09-20
//! 用户实机报「召出键盘后，外部框架卡回缩了，但里面的内容却没有，
//! 内容超出了卡片」。redroid 实录定罪（截屏像素扫描 + logcat 几何
//! 遥测）：页外框环吃 bottom_inset=ime+bar_h（键盘在场环底停在
//! 1638），内容吃 bar_h（BAR-119 红线：只盖不重排，环境卡底 1927）——
//! 框与内容两把尺，键盘在场框底边悬在卡半腰。修复：解析页壳与内容
//! 同吃 bar_h（框底与内容同缘，键盘遮盖对两者同线一致）。
//!
//! 本钉 = 源码守卫：android_app.rs 两个产品路（GLES 烘焙/softbuffer
//! 兜底）的 paint_parser_page_chrome 调用，inset 实参必须与紧邻的
//! paint_parser_content 同源（bar_h），不许回潮成 bottom_inset 族。
//!
//! 变异抽检：任一调用点把 bar_h 换回 bottom_inset（BAR-120 原病灶）
//! 必须咬。

/// 产品路（android_app.rs）里每个 paint_parser_page_chrome 调用点的
/// inset 实参都是 bar_h
#[test]
fn spec_bar120_解析页壳内容inset同源() {
    let src = include_str!("../src/android_app.rs");
    let mut n = 0usize;
    for (i, _) in src.match_indices("paint_parser_page_chrome(") {
        // 调用实参窗：从调用点起逐行扫到闭合「);」（中文注释多字节，
        // 按字节切窗会切穿 char 边界——逐行扫无此坑）
        let mut args = String::new();
        for line in src[i..].lines() {
            let closed = line.contains(");");
            args.push_str(line);
            if closed {
                break;
            }
        }
        assert!(
            args.contains("bar_h"),
            "paint_parser_page_chrome 调用必须传 bar_h（与内容同源）"
        );
        assert!(
            !args.contains("bottom_inset"),
            "BAR-120 回潮：解析页壳吃了 bottom_inset（含键盘）= 框与内容两把尺"
        );
        n += 1;
    }
    assert!(n >= 2, "GLES 烘焙与 softbuffer 兜底两个产品路都要钉到");
}
