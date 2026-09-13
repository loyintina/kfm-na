//! tests/cfg_page_spec.rs — A 档考题：配置页三层目录状态核（src/ui/cfg_page.rs）
//!
//! 契约真相源：docs/active/theme.md §五「双池的目录语义」（2026-09-13 修宪）。
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::ui::cfg_page::{CfgPage, POOL_ROW_H, RowView};
use kfm_na::ui::dual_pool::PoolRect;

fn rows(n: usize) -> Vec<RowView> {
    // [全局] + 服务器×n + [+ 新增]（壳喂表同款构造）
    let mut v = vec![RowView {
        title: "全局".into(),
        meta: "默认终端 / 切换快捷键".into(),
    }];
    for i in 0..n {
        v.push(RowView {
            title: format!("服务器{i}"),
            meta: format!("10.0.0.{i}"),
        });
    }
    v.push(RowView {
        title: "+ 新增服务器".into(),
        meta: String::new(),
    });
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

// ---- 双向联动（宪法 §五.4 核心钉）----

#[test]
fn select_moves_focus_and_bumps_epoch() {
    let mut p = CfgPage::new();
    p.set_rows(rows(2));
    let e0 = p.epoch();
    p.select(2);
    assert_eq!(p.focus(), 2);
    assert!(p.epoch() > e0, "聚焦变更必须 bump 代际（sig 防鬼影）");
    let e1 = p.epoch();
    p.select(2); // 同标重点不重掷
    assert_eq!(p.epoch(), e1);
}

#[test]
fn dropdown_pick_syncs_focus_and_closes() {
    // 上→下半向：下拉框换选 = 下池聚焦同步 + panel 收
    let mut p = CfgPage::new();
    p.set_rows(rows(3));
    p.toggle_dropdown();
    assert!(p.dropdown_open());
    p.dropdown_pick(3);
    assert_eq!(p.focus(), 3);
    assert!(!p.dropdown_open(), "点选后 panel 必须收");
}

#[test]
fn dismiss_on_outside_tap() {
    let mut p = CfgPage::new();
    p.set_rows(rows(1));
    p.toggle_dropdown();
    p.dismiss_dropdown();
    assert!(!p.dropdown_open());
    // 收着时 dismiss 不空涨代际
    let e = p.epoch();
    p.dismiss_dropdown();
    assert_eq!(p.epoch(), e);
}

// ---- 行表与越界 ----

#[test]
fn focus_clamps_when_rows_shrink() {
    let mut p = CfgPage::new();
    p.set_rows(rows(3)); // 5 行
    p.select(4);
    p.set_rows(rows(0)); // 缩到 2 行
    assert_eq!(p.focus(), 1, "行表缩水 focus 必须 clamp 到末行");
}

#[test]
fn select_clamps_to_last_row() {
    let mut p = CfgPage::new();
    p.set_rows(rows(2)); // 4 行
    p.select(99);
    assert_eq!(p.focus(), 3);
}

// ---- 几何（眼手同尺：涂装/命中同一份）----

#[test]
fn row_rect_stacks_by_pool_row_h() {
    let p = CfgPage::new();
    let r0 = p.row_rect(0, &LOWER);
    let r1 = p.row_rect(1, &LOWER);
    assert_eq!(r0.h, POOL_ROW_H);
    assert_eq!(r1.y - r0.y, POOL_ROW_H as i64, "逐行 2 格叠放");
    assert!(r0.x > LOWER.x && r0.y > LOWER.y, "内容内缩池框缘 1 格");
    assert!(r0.x + r0.w as i64 <= LOWER.x + LOWER.w as i64);
}

#[test]
fn row_at_y_roundtrip() {
    let mut p = CfgPage::new();
    p.set_rows(rows(2)); // 4 行
    let r2 = p.row_rect(2, &LOWER);
    assert_eq!(p.row_at_y(r2.y + 5, &LOWER), Some(2));
    assert_eq!(p.row_at_y(r2.y + POOL_ROW_H as i64 + 5, &LOWER), Some(3));
    assert_eq!(p.row_at_y(LOWER.y, &LOWER), None, "池缘内缩带不算行");
    // 行表之外（第 5 行位置）= None
    let r9 = p.row_rect(9, &LOWER);
    assert_eq!(p.row_at_y(r9.y, &LOWER), None);
}

#[test]
fn trigger_and_fields_stack_in_upper() {
    let p = CfgPage::new();
    let t = p.trigger_rect(&UPPER);
    assert_eq!(t.h, POOL_ROW_H);
    let f0 = p.field_rect(0, &UPPER);
    assert_eq!(f0.y, t.y + POOL_ROW_H as i64, "字段行在触发器之下");
}

#[test]
fn dropdown_panel_opens_downward() {
    // 宪法 §六：顶部栏向下弹（反了弹出屏外——kfmv4 教训）
    let mut p = CfgPage::new();
    p.set_rows(rows(3)); // 5 行
    let t = p.trigger_rect(&UPPER);
    let panel = p.dropdown_panel_rect(&UPPER, 10_000);
    assert_eq!(panel.y, t.y + POOL_ROW_H as i64);
    assert_eq!(panel.h, 5 * POOL_ROW_H);
    // max_h 钳制
    let clamped = p.dropdown_panel_rect(&UPPER, 3 * POOL_ROW_H);
    assert_eq!(clamped.h, 3 * POOL_ROW_H);
}

#[test]
fn dropdown_item_hit() {
    let mut p = CfgPage::new();
    p.set_rows(rows(3));
    let panel = p.dropdown_panel_rect(&UPPER, 10_000);
    assert_eq!(p.dropdown_item_at_y(panel.y + 1, &UPPER, 10_000), Some(0));
    assert_eq!(
        p.dropdown_item_at_y(panel.y + 4 * POOL_ROW_H as i64 + 1, &UPPER, 10_000),
        Some(4)
    );
    assert_eq!(p.dropdown_item_at_y(panel.y - 1, &UPPER, 10_000), None);
    assert_eq!(
        p.dropdown_item_at_y(panel.y + panel.h as i64 + 1, &UPPER, 10_000),
        None
    );
}

// ---- 上池内容高（喂双池数学钉）----

#[test]
fn upper_content_h_counts_trigger_plus_fields() {
    let mut p = CfgPage::new();
    p.set_rows(rows(1));
    let inset = p.upper_content_h() - POOL_ROW_H; // 只有触发器时 = inset + 1 行
    p.set_fields(vec![("a".into(), "b".into()), ("c".into(), "d".into())]);
    assert_eq!(p.upper_content_h(), inset + 3 * POOL_ROW_H);
}

#[test]
fn fields_change_bumps_epoch() {
    let mut p = CfgPage::new();
    p.set_rows(rows(1));
    let e = p.epoch();
    p.set_fields(vec![("x".into(), "y".into())]);
    assert!(p.epoch() > e);
}
