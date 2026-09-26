//! tests/modal_spec.rs — A 档考题：跳框几何核（src/ui/modal.rs）
//!
//! 契约真相源：docs/active/theme.md §六「跳框（modal）」（2026-09-13 九修：
//! 压暗层+居中卡+题注上内容下+全宽关闭钮+点框外/关闭钮收起）+ §七
//! 登记表跳框行（标定值）。纪律：先验证红，答案生成到绿，绿后变异
//! 抽检。本文件是考题，生成器不许改。

use kfm_na::termview::{CELL_H, CELL_W};
use kfm_na::ui::comp_registry::COMPONENTS;
use kfm_na::ui::modal::{
    MODAL_CLOSE_H, MODAL_FIELD_GAP, MODAL_LINE_H, MODAL_MAX_MARGIN_BOTTOM, MODAL_MAX_MARGIN_TOP,
    MODAL_PAD_X, MODAL_PAD_Y, MODAL_PREVIEW_H, MODAL_SIDE_MARGIN, MODAL_TITLE_H, ModalHit,
    card_rect, close_btn_rect, content_cells, fields_of, fields_top, hit, pick_screen_px,
    preview_rect, wrap_text,
};

const SCR_W: u32 = 1221;
const SCR_H: u32 = 2712;

// ---- 折行（格宽尺：CJK 2 格/其余 1 格）----

#[test]
fn wrap_empty_and_short_fit() {
    assert_eq!(wrap_text("", 10), vec![String::new()], "空串 = 一行空占位");
    assert_eq!(wrap_text("abc", 10), vec!["abc".to_string()]);
    assert_eq!(
        wrap_text("测试", 4),
        vec!["测试".to_string()],
        "刚好放下不断"
    );
}

#[test]
fn wrap_cjk_counts_two_cells() {
    // 「测试文本」= 8 格，行宽 4 → 两行各 2 字
    assert_eq!(
        wrap_text("测试文本", 4),
        vec!["测试".to_string(), "文本".to_string()]
    );
    // 混排：「ab测」= 4 格刚好；再一字必断
    assert_eq!(
        wrap_text("ab测试", 4),
        vec!["ab测".to_string(), "试".to_string()]
    );
}

#[test]
fn wrap_zero_width_no_panic() {
    assert_eq!(wrap_text("abc", 0).len(), 1);
}

// ---- 卡片几何（§七 标定值）----

#[test]
fn content_cells_matches_card_inner_width() {
    // 卡宽 = 屏宽 − 左右各 3 格；内容宽 = 卡宽 − 两侧内边距 2 格
    let card_w = SCR_W - MODAL_SIDE_MARGIN * 2;
    let expect = (card_w as i64 - MODAL_PAD_X * 2) as u32 / CELL_W;
    assert_eq!(content_cells(SCR_W), expect);
}

#[test]
fn card_centered_and_within_screen() {
    let entry = &COMPONENTS[0];
    let fields = fields_of(entry, content_cells(SCR_W));
    let card = card_rect(SCR_W, SCR_H, &fields);
    assert_eq!(card.x, i64::from(MODAL_SIDE_MARGIN), "卡距屏左 3 格");
    assert_eq!(card.w, SCR_W - MODAL_SIDE_MARGIN * 2);
    assert!(
        card.y > 0 && card.y + card.h as i64 <= SCR_H as i64,
        "卡不出屏"
    );
    // 安全带内居中（BAR-163 翻案：旧屏心居中在封顶时把卡底拽进输入
    // 栏带；新约 = 顶 4 格 + 底 输入栏带+2 格的带内居中，偶奇差 ≤1）
    let top = card.y - i64::from(MODAL_MAX_MARGIN_TOP);
    let bottom = (SCR_H as i64 - i64::from(MODAL_MAX_MARGIN_BOTTOM)) - (card.y + card.h as i64);
    assert!((top - bottom).abs() <= 1, "安全带内上下余量相等（居中）");
}

#[test]
fn card_height_grows_with_content_and_caps() {
    let entry = &COMPONENTS[0];
    let few = fields_of(entry, content_cells(SCR_W));
    let c1 = card_rect(SCR_W, SCR_H, &few);
    // 塞 30 条长字段必然超封顶（安全带 = 顶 4 格 + 底 输入栏带+2 格）
    let many: Vec<_> = (0..30)
        .map(|i| kfm_na::ui::modal::ModalField {
            label: format!("字段{i}"),
            lines: wrap_text("一段足够长的说明文字用来撑高度测试封顶行为", 12),
        })
        .collect();
    let c2 = card_rect(SCR_W, SCR_H, &many);
    assert!(c2.h > c1.h, "高随内容生长");
    assert_eq!(
        c2.h,
        SCR_H - MODAL_MAX_MARGIN_TOP - MODAL_MAX_MARGIN_BOTTOM,
        "封顶 = 安全带（BAR-163 翻案：让出输入栏带）"
    );
}

#[test]
fn close_btn_full_inner_width_at_bottom() {
    let entry = &COMPONENTS[0];
    let fields = fields_of(entry, content_cells(SCR_W));
    let card = card_rect(SCR_W, SCR_H, &fields);
    let btn = close_btn_rect(&card);
    assert_eq!(btn.h, MODAL_CLOSE_H, "关闭钮 3 格高");
    assert_eq!(btn.x, card.x + MODAL_PAD_X);
    assert_eq!(
        btn.w as i64,
        card.w as i64 - MODAL_PAD_X * 2,
        "全内宽（§六：禁止短按钮）"
    );
    assert_eq!(
        card.y + card.h as i64 - (btn.y + btn.h as i64),
        i64::from(MODAL_PAD_Y),
        "钮底到卡底 = 1 格留白"
    );
}

// ---- 命中分类（模态交互）----

#[test]
fn hit_classification() {
    let entry = &COMPONENTS[0];
    let fields = fields_of(entry, content_cells(SCR_W));
    let card = card_rect(SCR_W, SCR_H, &fields);
    let btn = close_btn_rect(&card);
    assert_eq!(
        hit(card.x - 1, card.y + 10, &card),
        ModalHit::Outside,
        "框外左"
    );
    assert_eq!(
        hit(card.x + 10, card.y + card.h as i64 + 1, &card),
        ModalHit::Outside,
        "框外下"
    );
    assert_eq!(
        hit(card.x + card.w as i64 / 2, card.y + 5, &card),
        ModalHit::Card,
        "卡内标题区 = Card（无操作吃手势）"
    );
    assert_eq!(
        hit(btn.x + btn.w as i64 / 2, btn.y + btn.h as i64 / 2, &card),
        ModalHit::Close,
        "关闭钮心 = Close"
    );
}

// ---- 字段区（题注上内容下，有意反向）----

#[test]
fn fields_of_covers_five_slots() {
    let entry = &COMPONENTS[0];
    let fields = fields_of(entry, content_cells(SCR_W));
    let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels, ["状态", "位置", "规范", "考题", "说明"]);
    for f in &fields {
        assert!(!f.lines.is_empty(), "字段 {} 至少一行内容", f.label);
    }
    // 长说明必须按卡宽折行（折行尺 = content_cells 同源）
    let wide = fields_of(entry, 6);
    let desc = wide.iter().find(|f| f.label == "说明").unwrap();
    assert!(desc.lines.len() > 1, "窄尺下说明必须折行");
}

#[test]
fn line_heights_match_constitution() {
    assert_eq!(MODAL_TITLE_H, CELL_H * 2, "标题 2 格");
    assert_eq!(MODAL_LINE_H, CELL_H, "内容行 1 格");
    assert_eq!(MODAL_FIELD_GAP, CELL_H / 2, "字段隙 0.5 格");
    assert_eq!(MODAL_CLOSE_H, CELL_H * 3, "关闭钮 3 格（最小容量律下限）");
    assert_eq!(MODAL_PAD_X, CELL_W as i64 * 2, "卡内边距 2 格");
    assert_eq!(MODAL_PREVIEW_H, CELL_H * 6, "预览画板 6 格（十修条款）");
}

// ---- 预览画板（§六 十修：画板在分隔线与字段区之间，是内容不是档案）----

#[test]
fn preview_rect_between_divider_and_fields() {
    let entry = &COMPONENTS[0];
    let fields = fields_of(entry, content_cells(SCR_W));
    let card = card_rect(SCR_W, SCR_H, &fields);
    let prev = preview_rect(&card);
    // 画板紧贴分隔线带下缘（上 0.5 格 + 1px + 下 0.5 格之后）
    let divider_bottom =
        card.y + i64::from(MODAL_PAD_Y + MODAL_TITLE_H + MODAL_FIELD_GAP + 1 + MODAL_FIELD_GAP);
    assert_eq!(prev.y, divider_bottom, "画板在分隔线带正下方");
    assert_eq!(prev.h, MODAL_PREVIEW_H, "画板 6 格高");
    assert_eq!(prev.x, card.x + MODAL_PAD_X, "画板吃卡内边距");
    assert_eq!(prev.w as i64, card.w as i64 - MODAL_PAD_X * 2, "画板全内宽");
    // 字段区起点 = 画板下缘 + 0.5 格呼吸
    assert_eq!(
        fields_top(&card),
        prev.y + i64::from(MODAL_PREVIEW_H + MODAL_FIELD_GAP),
        "字段区在画板下 0.5 格"
    );
    // 画板不压关闭钮
    let btn = close_btn_rect(&card);
    assert!(prev.y + i64::from(prev.h) < btn.y, "画板带整体在关闭钮之上");
}

#[test]
fn card_height_includes_preview_band() {
    // 空字段时卡高 = 固定带之和（含 6 格画板带）——抽掉/改矮画板必咬
    let c = card_rect(SCR_W, SCR_H, &[]);
    let expect = MODAL_PAD_Y
        + MODAL_TITLE_H
        + (MODAL_FIELD_GAP + 1 + MODAL_FIELD_GAP)
        + MODAL_PREVIEW_H
        + MODAL_FIELD_GAP
        + MODAL_FIELD_GAP
        + MODAL_CLOSE_H
        + MODAL_PAD_Y;
    assert_eq!(c.h, expect, "卡高公式含画板带（空字段 = 全固定带）");
}

#[test]
fn spec_bar108_屏尺寸取舍_窗口优先() {
    // 窗口活着：实时尺寸优先，缓存再大也不看
    assert_eq!(
        pick_screen_px(Some((1080, 2400)), (1260, 2800)),
        Some((1080, 2400))
    );
}

#[test]
fn spec_bar108_屏尺寸取舍_挂起回退缓存() {
    // 挂起弃窗（BAR-004）：回退末次 Resized 缓存——后台注入手势几何不瞎
    // （BAR-108：跳框关闭臂曾因 window=None 静默跳过）
    assert_eq!(pick_screen_px(None, (1260, 2800)), Some((1260, 2800)));
}

#[test]
fn spec_bar108_屏尺寸取舍_双无则_none() {
    // 窗口死了且从没量过（缓存 (0,0)）= None——宁可无动作不瞎猜；
    // 半缓存（单边 0）同样不算数
    assert_eq!(pick_screen_px(None, (0, 0)), None);
    assert_eq!(pick_screen_px(None, (1260, 0)), None);
    assert_eq!(pick_screen_px(None, (0, 2800)), None);
}

// ---- BAR-163：查看器跳框几何（无预览画板版，会话池）----

#[test]
fn spec_bar163_viewer_fields_多行展开折行() {
    use kfm_na::ui::modal::viewer_fields;
    // 多行正文逐行展开再按卡宽折行；单字段「内容」
    let f = viewer_fields("用户： hi\nagent： 这是一个很长的回答超过十格", 10);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].label, "内容");
    assert_eq!(f[0].lines[0], "用户： hi");
    assert!(f[0].lines.len() >= 3, "长行按 10 格折: {:?}", f[0].lines);
    // 空正文 = 一行空占位不塌
    let f = viewer_fields("", 10);
    assert_eq!(f[0].lines, vec![String::new()]);
}

#[test]
fn spec_bar163_viewer_card_rect_去画板公式() {
    use kfm_na::ui::modal::{
        MODAL_FIELD_GAP, MODAL_MAX_MARGIN_BOTTOM, MODAL_MAX_MARGIN_TOP, MODAL_PAD_Y, MODAL_TITLE_H,
        card_rect, fields_of, viewer_card_rect, viewer_fields, viewer_fields_top,
    };
    // 同字段同屏：查看器卡 = comp 卡去掉画板段（MODAL_PREVIEW_H + 相邻
    // 两个 FIELD_GAP 换成 0）——正好矮 MODAL_PREVIEW_H + FIELD_GAP
    let comp_fields = fields_of(&COMPONENTS[0], content_cells(SCR_W));
    let comp_card = card_rect(SCR_W, SCR_H, &comp_fields);
    let v_fields = viewer_fields("x", content_cells(SCR_W));
    let v_card = viewer_card_rect(SCR_W, SCR_H, &v_fields);
    // 宽/x 与 comp 卡同尺（左右 3 格边距同族）
    assert_eq!(v_card.w, comp_card.w);
    assert_eq!(v_card.x, comp_card.x);
    // 字段区顶 = 分隔线下 0.5 格（无画板）
    let expect_top = v_card.y
        + i64::from(MODAL_PAD_Y)
        + i64::from(MODAL_TITLE_H)
        + i64::from(MODAL_FIELD_GAP)
        + 1
        + i64::from(MODAL_FIELD_GAP);
    assert_eq!(viewer_fields_top(&v_card), expect_top);
    // 高随内容封顶安全带（BAR-163 翻案：旧契约「屏高−8 格」让卡底挨
    // 输入栏 = 症②病灶本体；新约 = 顶 4 格 + 底（输入栏带+2 格））
    let long = "行\n".repeat(500);
    let tall = viewer_card_rect(SCR_W, SCR_H, &viewer_fields(&long, content_cells(SCR_W)));
    assert_eq!(
        tall.h,
        SCR_H - MODAL_MAX_MARGIN_TOP - MODAL_MAX_MARGIN_BOTTOM
    );
}

// ---- BAR-163 翻案钉（2026-09-26 用户真机三症终验报障：缺全屏压暗/
// 卡太大关闭钮差点按不到/下池光标框透出压卡）——观测先行三条款第 4 条
// 「静默判过」活例：BAR-163 真机自验判卷时三症全在但没被报障口径
// 覆盖，用户肉眼终验翻案重开。修复臂 = 压暗层升组件（ChromeSlot::
// ModalVeil 全屏层，z 序 Over 之上）+ 卡高公式让出输入栏带 + 同族
// comp modal 同病同收 ----

#[test]
fn spec_bar163_翻案_卡高上限让出输入栏带() {
    use kfm_na::ui::modal::{
        MODAL_MAX_MARGIN_BOTTOM, MODAL_MAX_MARGIN_TOP, card_rect, close_btn_rect, content_cells,
        fields_of, viewer_card_rect, viewer_fields,
    };
    let (w, h) = (1260u32, 2560u32); // 真机屏（redroid 同尺）
    let long = "行\n".repeat(500);
    let vf = viewer_fields(&long, content_cells(w));
    let vc = viewer_card_rect(w, h, &vf);
    // 上下安全边距：顶 ≥ MODAL_MAX_MARGIN_TOP，卡底 ≤ 屏高 − 底余量
    assert!(
        vc.y >= i64::from(MODAL_MAX_MARGIN_TOP),
        "卡顶越安全边距: y={}",
        vc.y
    );
    assert!(
        vc.y + i64::from(vc.h) <= i64::from(h - MODAL_MAX_MARGIN_BOTTOM),
        "卡底越安全边距: 底={}",
        vc.y + i64::from(vc.h)
    );
    // 关闭钮底 → 输入栏顶（HEIGHT_PX=220）间距 ≥ 2 格——「差点按不到」翻案
    let btn = close_btn_rect(&vc);
    let bar_top = i64::from(h - kfm_na::input_bar::HEIGHT_PX);
    assert!(
        bar_top - (btn.y + i64::from(btn.h)) >= i64::from(CELL_H * 2),
        "关闭钮与输入栏间距不足 2 格: {}",
        bar_top - (btn.y + i64::from(btn.h))
    );
    // comp modal 同公式同收（同族同病）
    let cf = fields_of(&COMPONENTS[0], content_cells(w));
    let cc = card_rect(w, h, &cf);
    assert!(
        cc.y + i64::from(cc.h) <= i64::from(h - MODAL_MAX_MARGIN_BOTTOM),
        "comp 卡底越安全边距"
    );
    // 短内容小卡也落在安全带内（带内居中，不许被旧屏心公式拽向底栏）
    let sf = viewer_fields("短", content_cells(w));
    let sc = viewer_card_rect(w, h, &sf);
    assert!(sc.y >= i64::from(MODAL_MAX_MARGIN_TOP));
    assert!(sc.y + i64::from(sc.h) <= i64::from(h - MODAL_MAX_MARGIN_BOTTOM));
}

#[test]
fn spec_bar163_翻案_压暗层开合判据() {
    use kfm_na::ui::modal::veil_open;
    assert!(!veil_open(false, false), "双无 = 不开");
    assert!(veil_open(true, false), "comp modal 开 = 压暗开");
    assert!(
        veil_open(false, true),
        "查看器开 = 压暗开（BAR-163 欠账本体）"
    );
    assert!(veil_open(true, true));
}

#[test]
fn spec_bar163_翻案_压暗层入册动效预览() {
    use kfm_na::ui::comp_registry::{Preview, preview_is_animated};
    let e = COMPONENTS
        .iter()
        .find(|e| e.name == "压暗层")
        .expect("压暗层必须入 comp_registry（组件不是背景色）");
    assert_eq!(e.cat, "动效引擎", "压暗层归动效引擎大类");
    assert_eq!(
        e.preview,
        Preview::VeilFade,
        "预览 = 压暗淡入淡出语义化演示"
    );
    assert!(
        preview_is_animated(Preview::VeilFade),
        "VeilFade 必须入动画帧泵名单"
    );
}
