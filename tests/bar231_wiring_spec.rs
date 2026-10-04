//! tests/bar231_wiring_spec.rs — BAR-231 楼层引用行左竖线接线守卫
//! （源码钉，仿 viewer_fling_wiring_spec 同款）：楼层引用行涂装在
//! termview 网格引擎直画面（android 宿主编不到、光栅不便行为断言），
//! 故 include_str! 把关键接线字面量钉死——竖线件（QUOTE_BAR_W 渐变）
//! 与缩进件（INDENT_W）必须与栏二标题引用行同尺同源，整块灰字
//! 「> N楼」字面形态必须绝迹。卡内只显摘要的行为钉在
//! mail_feed_spec（spec_bar231_楼层卡只显摘要_display_text）。

const TERM: &str = include_str!("../src/termview.rs");

#[test]
fn spec_bar231_引用行左竖线_接线守卫() {
    // ① 竖线件在位且两处同尺：栏二标题引用行 + 楼层引用行都吃
    // demo_page::QUOTE_BAR_W（md 引用同尺，删楼层竖线即 count 掉档）
    assert!(
        TERM.matches("tx..tx + i64::from(crate::ui::demo_page::QUOTE_BAR_W)")
            .count()
            >= 2,
        "楼层引用行必须画 QUOTE_BAR_W 渐变竖线（与标题引用行同尺）"
    );
    // ② 缩进件同尺：两处标签都从 INDENT_W（1 格）起笔
    assert!(
        TERM.matches("let qx = tx + i64::from(crate::ui::demo_page::INDENT_W);")
            .count()
            >= 2,
        "引用行标签必须吃 INDENT_W 1 格缩进（与标题引用行同尺）"
    );
    // ③ 「> N楼」整块灰字形态绝迹：标签 = 「N楼」，无 > 字面前缀
    assert!(
        !TERM.contains("format!(\"> {}楼\", f.n)"),
        "楼层引用行不许再画「> N楼」整块灰字（用户口述缺陷形态）"
    );
    assert!(
        TERM.contains("let qword = format!(\"{}楼\", f.n);"),
        "楼层引用行标签 = 「N楼」"
    );
}
