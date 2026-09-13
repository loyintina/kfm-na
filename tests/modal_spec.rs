//! tests/modal_spec.rs — A 档考题：跳框几何核（src/ui/modal.rs）
//!
//! 契约真相源：docs/active/theme.md §六「跳框（modal）」（2026-09-13 九修：
//! 压暗层+居中卡+题注上内容下+全宽关闭钮+点框外/关闭钮收起）+ §七
//! 登记表跳框行（标定值）。纪律：先验证红，答案生成到绿，绿后变异
//! 抽检。本文件是考题，生成器不许改。

use kfm_na::termview::{CELL_H, CELL_W};
use kfm_na::ui::comp_registry::COMPONENTS;
use kfm_na::ui::modal::{
    MODAL_CLOSE_H, MODAL_FIELD_GAP, MODAL_LINE_H, MODAL_PAD_X, MODAL_PAD_Y, MODAL_SIDE_MARGIN,
    MODAL_TITLE_H, ModalHit, card_rect, close_btn_rect, content_cells, fields_of, hit, wrap_text,
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
    // 垂直居中（偶奇差 ≤1）
    let top = card.y;
    let bottom = SCR_H as i64 - (card.y + card.h as i64);
    assert!((top - bottom).abs() <= 1, "上下余量相等（居中）");
}

#[test]
fn card_height_grows_with_content_and_caps() {
    let entry = &COMPONENTS[0];
    let few = fields_of(entry, content_cells(SCR_W));
    let c1 = card_rect(SCR_W, SCR_H, &few);
    // 塞 30 条长字段必然超封顶（屏高 − 8 格）
    let many: Vec<_> = (0..30)
        .map(|i| kfm_na::ui::modal::ModalField {
            label: format!("字段{i}"),
            lines: wrap_text("一段足够长的说明文字用来撑高度测试封顶行为", 12),
        })
        .collect();
    let c2 = card_rect(SCR_W, SCR_H, &many);
    assert!(c2.h > c1.h, "高随内容生长");
    assert_eq!(c2.h, SCR_H - CELL_H * 8, "封顶 = 屏高 − 8 格");
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
}
