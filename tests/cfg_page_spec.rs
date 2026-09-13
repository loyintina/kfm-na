//! tests/cfg_page_spec.rs — A 档考题：配置页三层目录状态核（src/ui/cfg_page.rs）
//!
//! 契约真相源：docs/active/theme.md §五「双池的目录语义」二版（2026-09-13
//! 修宪：下拉服务上池内容自身选项集不与下池联动/下池行 3 格框/上池字段框
//! 行 2 格标签列+值框）。纪律：先验证红，答案生成到绿，绿后变异抽检。
//! 本文件是考题，生成器不许改。

use kfm_na::ui::cfg_page::{
    CfgPage, FIELD_ROW_H, LOWER_ROW_H, ROW_GAP, RowView, UpperRow, lower_row_rect, upper_row_rect,
    value_box_rect,
};
use kfm_na::ui::dual_pool::PoolRect;

fn rows() -> Vec<RowView> {
    // 系统管理大类目前仅一行（壳喂表同款构造）
    vec![RowView {
        title: "系统管理".into(),
        meta: "服务器配置".into(),
    }]
}

fn upper(n_fields: usize) -> Vec<UpperRow> {
    let mut v = vec![
        UpperRow {
            label: "默认服务器".into(),
            value: "本地终端".into(),
            is_dropdown: true,
        },
        UpperRow {
            label: "服务器切换".into(),
            value: "Ctrl+]".into(),
            is_dropdown: false,
        },
    ];
    for i in 0..n_fields {
        v.push(UpperRow {
            label: format!("字段{i}"),
            value: format!("值{i}"),
            is_dropdown: false,
        });
    }
    v
}

fn opts(n_servers: usize) -> Vec<String> {
    let mut v = vec!["本地终端".to_string()];
    for i in 0..n_servers {
        v.push(format!("服务器{i}"));
    }
    v
}

const LOWER: PoolRect = PoolRect {
    x: 61,
    y: 500,
    w: 1139,
    h: 1200,
};
const UPPER: PoolRect = PoolRect {
    x: 61,
    y: 200,
    w: 1139,
    h: 264,
};

// ---- 下池聚焦 ----

#[test]
fn select_moves_focus_and_bumps_epoch() {
    let mut p = CfgPage::new();
    p.set_rows(vec![
        RowView {
            title: "a".into(),
            meta: String::new(),
        },
        RowView {
            title: "b".into(),
            meta: String::new(),
        },
    ]);
    let e0 = p.epoch();
    p.select(1);
    assert_eq!(p.focus(), 1);
    assert!(p.epoch() > e0, "聚焦变更必须 bump 代际（sig 防鬼影）");
    let e1 = p.epoch();
    p.select(1); // 同标重点不重掷
    assert_eq!(p.epoch(), e1);
}

// ---- 下拉（二版核心钉：选项集服务上池内容，不与下池联动）----

#[test]
fn dropdown_pick_sets_option_and_closes_without_touching_lower_focus() {
    let mut p = CfgPage::new();
    p.set_rows(rows());
    p.set_options(opts(2), 0);
    p.toggle_dropdown();
    assert!(p.dropdown_open());
    p.dropdown_pick(2);
    assert_eq!(p.option_sel(), 2, "点选 = 选项选中位变更");
    assert_eq!(p.focus(), 0, "下拉换选不许动下池聚焦（二版：不联动）");
    assert!(!p.dropdown_open(), "点选后 panel 必须收");
}

#[test]
fn dropdown_pick_clamps_and_same_pick_no_extra_bump() {
    let mut p = CfgPage::new();
    p.set_options(opts(1), 0); // 2 项
    p.toggle_dropdown();
    let e0 = p.epoch();
    p.dropdown_pick(99);
    assert_eq!(p.option_sel(), 1, "越界点选 clamp 到末项");
    assert!(p.epoch() > e0);
    // 同项重点：只收 panel 时 bump 一次，sel 不空涨——再点同项（panel
    // 已收）必须零代际变化
    let e1 = p.epoch();
    p.dropdown_pick(1);
    assert_eq!(p.epoch(), e1);
}

#[test]
fn dismiss_on_outside_tap() {
    let mut p = CfgPage::new();
    p.set_options(opts(1), 0);
    p.toggle_dropdown();
    p.dismiss_dropdown();
    assert!(!p.dropdown_open());
    let e = p.epoch();
    p.dismiss_dropdown();
    assert_eq!(p.epoch(), e, "收着时 dismiss 不空涨代际");
}

#[test]
fn set_options_clamps_sel() {
    let mut p = CfgPage::new();
    p.set_options(opts(3), 3); // 4 项，sel=3 合法
    assert_eq!(p.option_sel(), 3);
    p.set_options(opts(0), 3); // 缩到 1 项
    assert_eq!(p.option_sel(), 0, "选项表缩水 sel 必须 clamp");
}

// ---- 几何（眼手同尺：涂装/命中同一份）----

#[test]
fn lower_rows_stack_3_cells_with_gap() {
    let r0 = lower_row_rect(0, &LOWER);
    let r1 = lower_row_rect(1, &LOWER);
    assert_eq!(r0.h, LOWER_ROW_H, "下池行 = 3 格高");
    assert_eq!(
        r1.y - r0.y,
        LOWER_ROW_H as i64 + ROW_GAP,
        "逐行 3 格 + 留隙叠放"
    );
    assert!(r0.x > LOWER.x && r0.y > LOWER.y, "内容内缩池框缘 1 格");
    assert!(r0.x + r0.w as i64 <= LOWER.x + LOWER.w as i64);
}

#[test]
fn lower_row_hit_roundtrip() {
    let mut p = CfgPage::new();
    p.set_rows(vec![
        RowView {
            title: "a".into(),
            meta: String::new(),
        },
        RowView {
            title: "b".into(),
            meta: String::new(),
        },
    ]);
    let r1 = lower_row_rect(1, &LOWER);
    assert_eq!(p.lower_row_at_y(r1.y + 5, &LOWER), Some(1));
    assert_eq!(
        p.lower_row_at_y(r1.y - ROW_GAP / 2, &LOWER),
        None,
        "行间隙不算行"
    );
    assert_eq!(p.lower_row_at_y(LOWER.y, &LOWER), None, "池缘内缩带不算行");
}

#[test]
fn upper_field_rows_stack_2_cells_with_gap() {
    let r0 = upper_row_rect(0, &UPPER);
    let r1 = upper_row_rect(1, &UPPER);
    assert_eq!(r0.h, FIELD_ROW_H, "字段框行 = 2 格高");
    assert_eq!(r1.y - r0.y, FIELD_ROW_H as i64 + ROW_GAP);
}

#[test]
fn trigger_is_first_rows_value_box_right_of_label_col() {
    let p = CfgPage::new();
    let t = p.trigger_rect(&UPPER);
    let row0 = upper_row_rect(0, &UPPER);
    assert_eq!(t, value_box_rect(&row0));
    assert!(t.x > row0.x, "值框在标签列之右");
    assert_eq!(t.x + t.w as i64, row0.x + row0.w as i64, "值框右缘贴行右缘");
}

#[test]
fn dropdown_panel_opens_downward() {
    // 宪法 §六：顶部栏向下弹（反了弹出屏外——kfmv4 教训）
    let mut p = CfgPage::new();
    p.set_options(opts(3), 0); // 4 项
    let t = p.trigger_rect(&UPPER);
    let panel = p.dropdown_panel_rect(&UPPER, 10_000);
    assert_eq!(panel.y, t.y + FIELD_ROW_H as i64);
    assert_eq!(panel.x, t.x, "panel 与触发器同宽同左缘");
    assert_eq!(panel.h, 4 * FIELD_ROW_H);
    let clamped = p.dropdown_panel_rect(&UPPER, 3 * FIELD_ROW_H);
    assert_eq!(clamped.h, 3 * FIELD_ROW_H, "max_h 钳制");
}

#[test]
fn dropdown_item_hit() {
    let mut p = CfgPage::new();
    p.set_options(opts(3), 0);
    let panel = p.dropdown_panel_rect(&UPPER, 10_000);
    assert_eq!(p.dropdown_item_at_y(panel.y + 1, &UPPER, 10_000), Some(0));
    assert_eq!(
        p.dropdown_item_at_y(panel.y + 3 * FIELD_ROW_H as i64 + 1, &UPPER, 10_000),
        Some(3)
    );
    assert_eq!(p.dropdown_item_at_y(panel.y - 1, &UPPER, 10_000), None);
    assert_eq!(
        p.dropdown_item_at_y(panel.y + panel.h as i64 + 1, &UPPER, 10_000),
        None
    );
}

// ---- 上池内容高（喂双池数学钉）----

#[test]
fn upper_content_h_counts_rows_and_gaps() {
    let mut p = CfgPage::new();
    assert_eq!(p.upper_content_h(), 0, "空上池内容高 0（占位归双池）");
    p.set_upper(upper(0)); // 2 行
    let h2 = p.upper_content_h();
    p.set_upper(upper(2)); // 4 行
    let h4 = p.upper_content_h();
    assert_eq!(h4 - h2, 2 * FIELD_ROW_H + 2 * ROW_GAP as u32);
}

#[test]
fn upper_change_bumps_epoch() {
    let mut p = CfgPage::new();
    let e = p.epoch();
    p.set_upper(upper(0));
    assert!(p.epoch() > e);
}
