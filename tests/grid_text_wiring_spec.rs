//! tests/grid_text_wiring_spec.rs — 网格文字引擎收编接线守卫（BAR-195 起，
//! 仿 viewer_fling_wiring_spec 源码钉形制）：涂装面迁移是 android 宿主外的
//! 纯涂装改动，行为钉多数只能 C 档实拍——但「旧自然步进件不许回潮」是
//! 源码可查的硬线，用 include_str! 钉死。后续面迁移的守卫钉追加在本册。

/// 键栏涂装面（第 3 层控件，render_keybar 在本册 impl TermView）
const KEYBAR: &str = include_str!("../src/ui/keybar.rs");
/// 断线状态卡涂装面（paint_down_card 在 termview.rs）
const TERMVIEW: &str = include_str!("../src/termview.rs");

#[test]
fn spec_bar195_键栏断线卡_网格引擎接线守卫() {
    // ① 键栏标签 = 网格引擎居中件（量宽=总格×格宽、字号吃实例格 pinch
    // 联动）；旧 draw_label（rh×0.26 字面量+自然步进）退役，定义与调用
    // 都不许回潮
    assert!(
        KEYBAR.contains("self.draw_grid_text_centered("),
        "键栏标签必须走 draw_grid_text_centered（网格文字引擎）"
    );
    assert!(
        !KEYBAR.contains("draw_label("),
        "键栏不许回潮 draw_label（旧自然步进件）"
    );
    assert!(
        !TERMVIEW.contains("draw_label("),
        "draw_label 整件已退役（定义+调用零残留）"
    );
    // ② 断线状态卡：状态行 = 格引擎左对齐（measure_items_grid +
    // draw_grid_text_left），双钮 = 格引擎居中；30px 字面量退役
    assert!(
        TERMVIEW.contains("fn paint_down_card("),
        "断线状态卡涂装件必须在（本钉的看护对象）"
    );
    assert!(
        TERMVIEW.contains("self.measure_items_grid(status)"),
        "断线状态卡状态行必须吃网格引擎量宽"
    );
    // fmt 会折行，钉多行序列的稳态片段
    assert!(
        TERMVIEW.contains("self.draw_grid_text_centered(\n                frame,\n                label,\n                b.0,"),
        "断线状态卡双钮必须走 draw_grid_text_centered"
    );
}
