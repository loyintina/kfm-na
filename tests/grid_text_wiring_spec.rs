//! tests/grid_text_wiring_spec.rs — 网格文字引擎收编接线守卫（BAR-195 起，
//! 仿 viewer_fling_wiring_spec 源码钉形制）：涂装面迁移是 android 宿主外的
//! 纯涂装改动，行为钉多数只能 C 档实拍——但「旧自然步进件不许回潮」是
//! 源码可查的硬线，用 include_str! 钉死。后续面迁移的守卫钉追加在本册。

/// 键栏涂装面（第 3 层控件，render_keybar 在本册 impl TermView）
const KEYBAR: &str = include_str!("../src/ui/keybar.rs");
/// 断线状态卡涂装面（paint_down_card 在 termview.rs）
const TERMVIEW: &str = include_str!("../src/termview.rs");
/// 设置页字段/下拉几何命中面（BAR-196：壳侧量宽与涂装同尺）
const APP: &str = include_str!("../src/android_app.rs");

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

#[test]
fn spec_bar196_设置页解析页字段_网格引擎接线守卫() {
    // ① 格量宽件定义在（涂装格步进与几何/命中同一条尺的唯一取数口）。
    // 必须咬 `pub fn` 前缀：trait 声明/委托 impl 也含 "fn grid_text_width("，
    // 固有方法被改名时 trait 兜底会让全仓静默编译通过（无限递归），
    // 裸 "fn grid_text_width(" 咬不住（2026-09-30 变异实证）
    assert!(
        TERMVIEW.contains("pub fn grid_text_width("),
        "TermView::grid_text_width 固有方法定义必须在（BAR-196 新增胶水）"
    );
    // ② 旧 freetype 自由排版字段件 draw_field_lines 整件退役——定义与
    // 调用零残留（新件 draw_field_lines_grid 名含旧名子串，先抹新名再查）
    assert!(
        !TERMVIEW
            .replace("draw_field_lines_grid(", "")
            .contains("draw_field_lines("),
        "draw_field_lines 整件已退役（定义+调用零残留）"
    );
    assert!(
        TERMVIEW.matches("draw_field_lines_grid(").count() >= 8,
        "字段行涂装全家必须走 draw_field_lines_grid（上池 2 + 解析页 8 + 定义 1）"
    );
    // ③ 壳侧（android_app）设置页字段/下拉几何量宽全换格尺：六处调用
    // （首行 label/value 2 + 下拉内容最小宽 1 + 面板几何 label/value/
    // 选项 3），旧 text_width 实量宽形态零残留
    assert!(
        APP.matches("grid_text_width(").count() >= 6,
        "android_app 设置页字段/下拉几何量宽必须全走 grid_text_width"
    );
    assert!(
        !APP.contains(".text_width("),
        "android_app 不许回潮 text_width 实量宽（旧六处形态）"
    );
    // ④ 30/36 层级差常量立账（与 modal::MODAL_LABEL_SCALE 同值互指）
    assert!(
        TERMVIEW.contains("pub const GRID_META_SCALE: f32 = 30.0 / 36.0;"),
        "GRID_META_SCALE 常量必须在（30/36 层级差唯一源）"
    );
}
