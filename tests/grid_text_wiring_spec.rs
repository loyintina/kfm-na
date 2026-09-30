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
/// 输入栏涂装面（BAR-197，0017 #10 面）
const PROMPT: &str = include_str!("../src/ui/prompt_bar.rs");
/// 输入栏状态核（BAR-197 行距格化取数口钉）
const INPUTBAR: &str = include_str!("../src/input_bar.rs");
/// 字形图集（BAR-198 字号类退役钉）
const ATLAS: &str = include_str!("../src/glyph_atlas.rs");

/// 函数体切片（BAR-198：AI 面旧件 draw_items_left 在全册仍合法存活
/// ——配置页四版在用，裸全册查会误伤；接线守卫咬到函数体粒度）。
/// 从 fn 定义行起到下一个 `    ///`  doc 注释头止（本册函数全带文档）
fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let b = src.find(sig).unwrap_or_else(|| panic!("{sig} 定义必须在"));
    src[b..]
        .find("\n    ///")
        .map_or(&src[b..], |e| &src[b..b + e])
}

#[test]
fn spec_bar198_ai页_网格引擎接线守卫() {
    // ① 字面量字号退役：AI_PAGE_PX 三面（CPU 渲染/GPU 收集/壳侧装载）
    // 零残留——字形 = grid_fit 实例格（pinch 联动）
    assert!(
        !TERMVIEW.contains("AI_PAGE_PX"),
        "AI 页字面量字号 AI_PAGE_PX 已退役（字号吃 grid_fit 实例格）"
    );
    assert!(
        !APP.contains("AI_PAGE_PX"),
        "壳侧不许回潮 AI_PAGE_PX（装载走 rasterize_for_atlas 终端同一件）"
    );
    // ② 旧字号类退役：GLYPH_SIZE_AI 零残留——AI 页字形并入终端同册
    // GLYPH_SIZE_TERM，pinch 变格走 sync_term_glyph_size 整册重建
    assert!(
        !ATLAS.contains("GLYPH_SIZE_AI"),
        "GLYPH_SIZE_AI 已退役（glyph_atlas 全册只剩网格引擎字号类）"
    );
    assert!(
        !APP.contains("GLYPH_SIZE_AI"),
        "壳侧 AI 字形键/装载必须走 GLYPH_SIZE_TERM 同册"
    );
    // ②b 壳侧槽位查找双键都必须咬 TERM 类（裸「不许 GLYPH_SIZE_AI」
    // 咬不住字面量类号偷换——2026-09-30 变异实证：size 改字面量 1
    // 宿主测试全绿，ai_slot_of 只跑在机上。咬函数体粒度双键计数）
    let slot = fn_body(APP, "fn ai_slot_of(");
    assert!(
        slot.matches("GLYPH_SIZE_TERM").count() >= 2,
        "ai_slot_of 双键（主槽+兜底槽）都必须走 GLYPH_SIZE_TERM 同册"
    );
    // ③ 行距格化取数口在（咬定义行）+ 设计格常量重定义为格化读数
    assert!(
        TERMVIEW.contains("pub const fn ai_line_step("),
        "termview::ai_line_step 必须在（行距 = 16/9 格高唯一源）"
    );
    assert!(
        TERMVIEW.contains("pub const AI_PAGE_LINE_H: u32 = ai_line_step(CELL_H);"),
        "AI_PAGE_LINE_H 必须重定义为 ai_line_step(CELL_H)（设计格口径）"
    );
    // ④ AI 面函数体粒度：格引擎件在、旧件零残留（旧件全册合法存活，
    // 只能咬函数体——fn_body 切片）
    let render = fn_body(TERMVIEW, "pub fn render_ai_page(");
    assert!(
        render.contains("draw_grid_text_left("),
        "render_ai_page 必须走 draw_grid_text_left（格落笔）"
    );
    assert!(
        !render.contains("draw_items_left("),
        "render_ai_page 不许回潮 draw_items_left（旧自然步进落笔件）"
    );
    let build = fn_body(TERMVIEW, "fn build_ai_rows<'a>(");
    assert!(
        build.contains("measure_items_grid("),
        "build_ai_rows 必须吃 measure_items_grid（格量宽）"
    );
    assert!(
        !build.contains("measure_items("),
        "build_ai_rows 不许回潮 measure_items（旧自然步进量宽件）"
    );
    let wrap = fn_body(TERMVIEW, "fn wrap_ai_lines<'a>(");
    assert!(
        wrap.contains("measure_items_grid("),
        "wrap_ai_lines 折行尺必须吃格量宽（折行点随格尺）"
    );
    let collect = fn_body(TERMVIEW, "pub fn ai_page_glyphs(");
    assert!(
        collect.contains("ai_line_step(self.cell_h)"),
        "ai_page_glyphs 行距必须吃运行期 step（pinch 联动）"
    );
    // ⑤ 布局/渲染同一份运行期 step + 壳侧手势换算运行期化
    assert!(
        fn_body(TERMVIEW, "fn ai_page_layout<'a>(").contains("ai_page_fit_with_step("),
        "ai_page_layout 必须走 ai_page_fit_with_step（运行期行距）"
    );
    assert!(
        APP.contains("fn ai_page_line_step("),
        "壳侧 AI 页运行期行距取数口必须在（ai_page_line_step）"
    );
    assert!(
        !APP.contains("f64::from(crate::termview::AI_PAGE_LINE_H)"),
        "壳侧手势 px→行换算不许回潮设计格恒值（须吃运行期 step）"
    );
    // ⑥ 退役件零残留：AI 行基线尺/off_y 折算件/泛化供墨的壳侧调用
    assert!(
        !TERMVIEW.contains("ai_text_baseline_off"),
        "ai_text_baseline_off 已退役（格基线归 rasterize_for_atlas 同一件）"
    );
    assert!(
        !TERMVIEW.contains("ai_glyph_off_y"),
        "ai_glyph_off_y 已退役（off_y 由终端装载件烤进槽位）"
    );
    assert!(
        !APP.contains("rasterize_for_atlas_px("),
        "壳侧 AI 装载不许回潮泛化字号件（rasterize_for_atlas 同册同件）"
    );
}

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

#[test]
fn spec_bar197_输入栏_网格引擎接线守卫() {
    // ① 格量宽件定义在（'\n' 零宽条目保 item==char 1:1 的输入栏专用变体）。
    // 咬 `fn measure_bar_items_grid(` 定义行（pub(crate) 前缀若被摘，
    // 调用点照样编译过——与 spec_bar196 咬 pub fn 前缀同族教训）
    assert!(
        TERMVIEW.contains("fn measure_bar_items_grid("),
        "TermView::measure_bar_items_grid 定义必须在（BAR-197 新增胶水）"
    );
    // ② 输入栏面（prompt_bar）旧件零残留：旧量宽件（新名含旧名子串，
    // 先抹新名再查——spec_bar196 同款写法）/字面量字号/三个自然步进
    // 落笔件全部清零
    let prompt = PROMPT.replace("measure_bar_items_grid(", "");
    assert!(
        !prompt.contains("measure_bar_items("),
        "输入栏不许回潮 measure_bar_items（旧自然步进量宽件）"
    );
    assert!(
        !PROMPT.contains("BAR_TEXT_PX"),
        "输入栏字面量字号常量已退役（字号吃实例格 pinch 联动）"
    );
    for old in ["draw_items_left(", "draw_text_left(", "draw_text_centered("] {
        assert!(
            !PROMPT.contains(old),
            "输入栏不许回潮 {old}（旧自然步进落笔件）"
        );
    }
    // ③ 格引擎调用面：量宽 ×4（渲染/量行/点按/选择几何）+ 落笔左对齐
    // ×2（占位符+正文行）+ 菜单居中 ×1
    assert!(
        PROMPT.matches("measure_bar_items_grid(").count() >= 4,
        "输入栏量宽四调用点必须全走 measure_bar_items_grid"
    );
    assert!(
        PROMPT.matches("draw_grid_text_left(").count() >= 2,
        "占位符+正文行必须走 draw_grid_text_left"
    );
    assert!(
        PROMPT.matches("draw_grid_text_centered(").count() >= 1,
        "选择菜单四钮必须走 draw_grid_text_centered"
    );
    // ④ 行距格化取数口在（pinch 联动的关键二分之一）
    assert!(
        INPUTBAR.contains("fn line_step("),
        "input_bar::line_step 必须在（行距 = 7/4 格高唯一源）"
    );
    // ⑤ 旧量宽件整件退役：定义+调用零残留（termview 全册，先抹新名）
    assert!(
        !TERMVIEW
            .replace("measure_bar_items_grid(", "")
            .contains("measure_bar_items("),
        "measure_bar_items 整件已退役（定义+调用零残留）"
    );
    // ⑥ 运行期行距渗透双端：渲染面与壳/闸门命中面都传运行期 step
    assert!(
        PROMPT.contains("line_step("),
        "渲染面必须吃 line_step(实例格高)"
    );
    assert!(
        APP.contains("bar_line_step("),
        "壳侧命中/带高必须吃运行期行距（bar_line_step）"
    );
}

// ---- BAR-204 阅读页/md 字号档废除 · 迁网格文字引擎（a 案 pinch 一统）----

/// md 排版层（纯逻辑：尺子换芯 char_cells×实例格步进）
const MD_LAYOUT: &str = include_str!("../src/ui/md_layout.rs");
/// md 绘制层（壳：落笔换网格引擎）
const MD_PAINT: &str = include_str!("../src/ui/md_paint.rs");
/// 设置持久化（render.json 字号档体系废除面）
const SETTINGS: &str = include_str!("../src/settings.rs");

#[test]
fn spec_bar204_md阅读页_网格引擎接线守卫() {
    // ① 字号档体系整族退役：MdStyle 全局态/MdMeasure 通路/旧尺件在
    // 排版+涂装两册字面零残留（doc 注释也不许提——注释回潮 = 下一棒
    // 照注释复活的温床）
    for (name, src) in [("md_layout", MD_LAYOUT), ("md_paint", MD_PAINT)] {
        for gone in [
            "MdStyle",
            "MdMeasure",
            "MD_STYLE",
            "set_md_style",
            "md_style",
            "line_h_styled",
            "text_width",
            "draw_text_left_ex(",
            "draw_text_left(",
        ] {
            assert!(!src.contains(gone), "{name} 旧字号档件 {gone} 必须零残留");
        }
    }
    // ② 排版尺换芯：实例格两维入参 + char_cells 纯函数量宽 + 行高咬实例
    // 半格网 + 排版/涂装同源尺件（涂装 pen 累进直调，不许另写尺子）
    assert!(
        MD_LAYOUT.contains("pub fn layout_md(") && MD_LAYOUT.contains("cell: (u32, u32)"),
        "layout_md 必须吃实例格两维（pinch 唯一可调维）"
    );
    assert!(
        MD_LAYOUT.contains("char_cells"),
        "排版量宽必须走 char_cells（与涂装同源纯函数）"
    );
    assert!(
        MD_LAYOUT.contains("pub fn line_h_grid("),
        "行高件 line_h_grid 必须在（咬实例半格网）"
    );
    assert!(
        MD_LAYOUT.contains("pub fn grid_stepped_w("),
        "同源尺件 grid_stepped_w 必须在（排版行宽 = 涂装 pen 累进）"
    );
    // ③ 引擎新件在（定义行——咬 fn 前缀同 spec_bar196/197 教训）
    assert!(
        TERMVIEW.contains("fn measure_items_grid_stepped("),
        "引擎块必须有 measure_items_grid_stepped（自定步进+字形缩放）"
    );
    assert!(
        TERMVIEW.contains("fn draw_grid_text_stepped("),
        "引擎块必须有 draw_grid_text_stepped（自定步进落笔）"
    );
    // ④ 涂装同源（函数体粒度）：md 单行/段排两件的 pen 累进吃同一
    // grid_stepped_w、落笔走 stepped 引擎件；旧尺件函数体内零残留
    let line = fn_body(MD_PAINT, "fn md_text_line(");
    assert!(
        line.contains("measure_items_grid_stepped(") && line.contains("draw_grid_text_stepped("),
        "md_text_line 必须走 stepped 网格引擎（量宽+落笔）"
    );
    assert!(
        line.contains("grid_stepped_w("),
        "md_text_line pen 累进必须吃 grid_stepped_w（排版同源尺）"
    );
    assert!(
        !line.contains("text_width(") && !line.contains("draw_text_left_ex("),
        "md_text_line 旧尺件零残留"
    );
    let spans = fn_body(MD_PAINT, "fn md_spans_line(");
    assert!(
        spans.contains("grid_stepped_w("),
        "md_spans_line pen 累进必须吃 grid_stepped_w（排版同源尺）"
    );
    assert!(!spans.contains("text_width("), "md_spans_line 旧尺件零残留");
    // ⑤ 阅读页占位相/页脚迁网格居中件（切片粒度：占位相闭包与 capped
    // 页脚段——顶栏文件名/返回钮是别面存量，本单不动；先抹新名再查，
    // 同 spec_bar196 写法）
    let rd = fn_body(TERMVIEW, "pub(crate) fn paint_reader_content_impl(");
    let ph_at = rd.find("let placeholder").expect("占位相闭包必须在");
    let ph_end = rd[ph_at..]
        .find("match &page.phase")
        .expect("phase 分发必须在")
        + ph_at;
    let ph = &rd[ph_at..ph_end];
    assert!(
        ph.contains("draw_grid_text_centered("),
        "占位相必须走 draw_grid_text_centered（网格引擎居中件）"
    );
    assert!(
        !ph.replace("draw_grid_text_centered(", "")
            .contains("draw_text_centered("),
        "占位相旧居中件零残留"
    );
    let ft_at = rd.find("if page.capped").expect("capped 页脚必须在");
    let ft = &rd[ft_at..];
    assert!(
        ft.contains("draw_grid_text_centered("),
        "capped 页脚必须走 draw_grid_text_centered"
    );
    let ft_old = ft.replace("draw_grid_text_centered(", "");
    assert!(
        !ft_old.contains("draw_text_centered(") && !ft_old.contains("draw_text_centered_yclip("),
        "capped 页脚旧居中件零残留"
    );
    // ⑥ 壳侧字号档零残留 + sig/缓存键换实例格两维
    for gone in [
        "render_cfg",
        "render.json",
        "RenderConfig",
        "MD_FONT_STOPS",
        "MD_RATIO_STOPS",
        "set_md_style",
        "md_style",
        "apply_render",
    ] {
        assert!(
            !APP.contains(gone),
            "android_app 字号档件 {gone} 必须零残留"
        );
    }
    assert!(
        !SETTINGS.contains("RenderConfig") && !SETTINGS.contains("render.json"),
        "settings.rs 字号档持久化件必须零残留（settings_spec 有专钉，双保险）"
    );
    assert!(
        fn_body(APP, "fn poll_reader(&mut self)").contains("cell_size()"),
        "poll_reader 缓存键必须含实例格两维（pinch 变格 = 重排版）"
    );
    assert!(
        APP.contains("cell: (u32, u32)"),
        "VeilSig 必须含实例格维（pinch 变格 = 查看器版面变必须重烘）"
    );
}

// ---- BAR-210：prompt_bar 切片钳制收单源（2026-09-30 承影 panic.log 挖到
// 「slice index starts at N but ends at M」陈尸×4——时戳实证是 2026-09-01
// 旧核（BAR-045 当夜已修+BAR-197 重写七处各加 ad-hoc 钳），本 BAR 把七处
// 钳收口成 clamp_slice 单源并接线钉死，永不回潮）----

#[test]
fn spec_bar210_切片钳制纯函数() {
    use kfm_na::ui::prompt_bar::clamp_slice;
    let v = [10, 20, 30, 40];
    assert_eq!(clamp_slice(&v, 1, 3), &[20, 30]); // 正常区间原样
    assert_eq!(clamp_slice(&v, 3, 1), &[] as &[i32]); // 倒挂=空片（陈尸族死因）
    assert_eq!(clamp_slice(&v, 2, 99), &[30, 40]); // 终越界钳 len
    assert_eq!(clamp_slice(&v, 99, 100), &[] as &[i32]); // 始越界=空片
    assert_eq!(clamp_slice(&v, 0, 4), &[10, 20, 30, 40]); // 全量
    assert_eq!(clamp_slice(&v, 0, 0), &[] as &[i32]); // 空区间
}

#[test]
fn spec_bar210_输入栏裸区间切片清零接线守卫() {
    // prompt_bar.rs 内不许再有裸 `items[… .. …]` 区间切片（单点索引
    // items[row_end-1] 上界由 row_end≤len 构造守，不在禁列）——
    // 一切区间切片走 clamp_slice（定义 1 + 调用点 7）
    let mut bare = 0;
    for line in PROMPT.lines() {
        // 注释行不算（防 prose 毒——BAR-192 同族教训：本钉自己的 doc 里
        // 就写着 `items[始..终]` 字样）
        if line.trim_start().starts_with("//") {
            continue;
        }
        if let Some(pos) = line.find("items[")
            && line[pos..].contains("..")
        {
            bare += 1;
        }
    }
    assert_eq!(
        bare, 0,
        "prompt_bar 裸 items[..] 区间切片必须清零（走 clamp_slice）"
    );
    assert!(
        PROMPT.matches("clamp_slice(").count() >= 8,
        "clamp_slice 单源（定义+七调用点）必须在"
    );
}
