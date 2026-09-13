//! tests/cfg_page_spec.rs — A 档考题：配置页三层目录状态核（src/ui/cfg_page.rs）
//!
//! 契约真相源：docs/active/theme.md §五「双池的目录语义」二版（2026-09-13
//! 修宪：下拉服务上池内容自身选项集不与下池联动/下池行框/上池字段框行
//! 标签列+值框）+ 四版（同日：字段框行 4 格/下池行 4.5 格/行隙 1.5 格/
//! 内边距 2 格/标签列 12 格/上池像素滚动）+ 六修（同日：值框 3 格居中
//! 于行无左粗条/字档反转标签亮值灰/下拉 panel 从值框下缘起弹）+ 七修
//! （同日：上池行隙减半格=1 格/字号反转标签 36 值 30/三级框角部渐细
//! 同源页环）。纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是
//! 考题，生成器不许改。

use kfm_na::ui::cfg_page::{
    CfgPage, FIELD_BOX_H, FIELD_ROW_GAP, FIELD_ROW_H, LABEL_COL_W, LOWER_ROW_H, ROW_GAP, RowView,
    UpperRow, lower_row_rect, upper_row_rect, value_box_rect,
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
fn lower_rows_stack_4_5_cells_with_gap() {
    let r0 = lower_row_rect(0, &LOWER);
    let r1 = lower_row_rect(1, &LOWER);
    assert_eq!(r0.h, LOWER_ROW_H, "下池行 = 4.5 格高（四版 ×1.5）");
    assert_eq!(
        r1.y - r0.y,
        LOWER_ROW_H as i64 + ROW_GAP,
        "逐行 4.5 格 + 留隙叠放"
    );
    assert!(r0.x > LOWER.x && r0.y > LOWER.y, "内容内缩池框缘 2 格");
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
fn upper_field_rows_stack_4_cells_with_gap() {
    let r0 = upper_row_rect(0, &UPPER, 0);
    let r1 = upper_row_rect(1, &UPPER, 0);
    assert_eq!(r0.h, FIELD_ROW_H, "字段框行 = 4 格高（四版 ×2）");
    assert_eq!(
        r1.y - r0.y,
        FIELD_ROW_H as i64 + FIELD_ROW_GAP,
        "上池行隙 = 1 格（七修减半格）"
    );
}

#[test]
fn upper_rows_shift_with_scroll() {
    let r0 = upper_row_rect(0, &UPPER, 0);
    let r0s = upper_row_rect(0, &UPPER, 40);
    assert_eq!(r0s.y, r0.y - 40, "滚动 = 内容整体上移同额 px");
    assert_eq!(
        (r0s.x, r0s.w, r0s.h),
        (r0.x, r0.w, r0.h),
        "滚动不动横向与行高"
    );
}

#[test]
fn trigger_is_first_rows_value_box_right_of_label_col() {
    let p = CfgPage::new();
    let t = p.trigger_rect(&UPPER);
    let row0 = upper_row_rect(0, &UPPER, 0);
    assert_eq!(t, value_box_rect(&row0));
    assert!(t.x > row0.x, "值框在标签列之右");
    assert_eq!(t.x + t.w as i64, row0.x + row0.w as i64, "值框右缘贴行右缘");
}

#[test]
fn value_box_is_3_cells_centered_in_row() {
    // 六修：值框上下线各往中心缩半格——3 格高居中于 4 格行，横向不动
    let row = upper_row_rect(0, &UPPER, 0);
    let vb = value_box_rect(&row);
    assert_eq!(vb.h, FIELD_BOX_H, "值框 = 3 格高");
    assert_eq!(
        vb.y,
        row.y + (FIELD_ROW_H - FIELD_BOX_H) as i64 / 2,
        "垂直居中：上缩 = 下缩 = 半格"
    );
    assert_eq!(
        row.y + row.h as i64 - (vb.y + vb.h as i64),
        vb.y - row.y,
        "上下呼吸位等宽"
    );
    assert_eq!(
        (vb.x, vb.w),
        (row.x + LABEL_COL_W, row.w - LABEL_COL_W as u32),
        "横向不动"
    );
}

#[test]
fn dropdown_panel_opens_downward() {
    // 宪法 §六：顶部栏向下弹（反了弹出屏外——kfmv4 教训）
    let mut p = CfgPage::new();
    p.set_options(opts(3), 0); // 4 项
    let t = p.trigger_rect(&UPPER);
    let panel = p.dropdown_panel_rect(&UPPER, 10_000);
    assert_eq!(panel.y, t.y + t.h as i64, "panel 从值框下缘起弹（六修）");
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
    assert_eq!(h4 - h2, 2 * FIELD_ROW_H + 2 * FIELD_ROW_GAP as u32);
}

#[test]
fn upper_change_bumps_epoch() {
    let mut p = CfgPage::new();
    let e = p.epoch();
    p.set_upper(upper(0));
    assert!(p.epoch() > e);
}

// ---- 上池滚动（四版 §五「超出部分上池内滚动」兑现）----

#[test]
fn scroll_clamps_to_content_and_bumps_epoch() {
    let mut p = CfgPage::new();
    p.set_upper(upper(8)); // 10 行：内容高 >> 池高
    let pool_h = 400;
    let max = (p.upper_content_h() - pool_h) as i64;
    assert!(max > 0, "夹具内容必须溢出池高");
    assert!(!p.scroll_upper_by(-50, pool_h), "顶再往上 = 到位不动");
    assert_eq!(p.upper_scroll(), 0);
    let e0 = p.epoch();
    assert!(p.scroll_upper_by(120, pool_h));
    assert_eq!(p.upper_scroll(), 120);
    assert!(p.epoch() > e0, "滚动必须 bump 代际（sig 防鬼影）");
    let e1 = p.epoch();
    assert!(p.scroll_upper_by(999_999, pool_h), "越底 clamp 到 max");
    assert_eq!(p.upper_scroll(), max);
    let _ = e1;
    let e2 = p.epoch();
    assert!(!p.scroll_upper_by(1, pool_h), "已在底 = 到位不空涨代际");
    assert_eq!(p.epoch(), e2);
}

#[test]
fn scroll_zero_when_content_fits() {
    let mut p = CfgPage::new();
    p.set_upper(upper(0)); // 2 行装得下
    assert!(!p.scroll_upper_by(120, 10_000), "装得下 = 恒 0 不动");
    assert_eq!(p.upper_scroll(), 0);
}

#[test]
fn trigger_and_dropdown_follow_scroll() {
    let mut p = CfgPage::new();
    p.set_upper(upper(8));
    p.set_options(opts(3), 0);
    let pool_h = 400;
    let t0 = p.trigger_rect(&UPPER);
    p.scroll_upper_by(120, pool_h);
    let t1 = p.trigger_rect(&UPPER);
    assert_eq!(t1.y, t0.y - 120, "触发器随内容一起滚");
    // 下拉命中同尺：panel 跟触发器走，项命中必须带滚动维
    let panel = p.dropdown_panel_rect(&UPPER, 10_000);
    assert_eq!(panel.y, t1.y + t1.h as i64);
    assert_eq!(p.dropdown_item_at_y(panel.y + 1, &UPPER, 10_000), Some(0));
    assert_eq!(
        p.dropdown_item_at_y(t1.y + 1, &UPPER, 10_000),
        None,
        "panel 上方一格（触发器位）不算 panel 项"
    );
}

// ---- 九修：tab 维（宪法 §五 目录语义 7 组件池页）----

#[test]
fn set_tab_switches_and_resets_page_state() {
    let mut p = CfgPage::new();
    p.set_rows(rows());
    p.set_upper(upper(8));
    p.set_options(opts(2), 0);
    p.select(0);
    p.scroll_upper_by(120, 400);
    p.toggle_dropdown();
    p.open_modal(3);
    let e0 = p.epoch();
    p.set_tab(1);
    assert_eq!(p.tab(), 1);
    assert!(p.epoch() > e0, "切页必须 bump 代际（sig 防鬼影）");
    assert_eq!(p.upper_scroll(), 0, "切页上池滚动归零——新页不继承旧滚动");
    assert!(!p.dropdown_open(), "切页下拉收");
    assert_eq!(p.modal(), None, "切页跳框收");
    let e1 = p.epoch();
    p.set_tab(1); // 同页重点不空涨
    assert_eq!(p.epoch(), e1);
}

#[test]
fn set_tab_back_and_forth() {
    let mut p = CfgPage::new();
    p.set_tab(1);
    p.set_tab(0);
    assert_eq!(p.tab(), 0);
    assert_eq!(p.focus(), 0, "切页聚焦归首行（壳重建前的安全态）");
}

// ---- 九修：跳框开合（§六 跳框条款）----

#[test]
fn modal_open_close_bumps_epoch_idempotent() {
    let mut p = CfgPage::new();
    assert_eq!(p.modal(), None);
    let e0 = p.epoch();
    p.open_modal(2);
    assert_eq!(p.modal(), Some(2));
    assert!(p.epoch() > e0, "开框必须 bump 代际");
    let e1 = p.epoch();
    p.open_modal(2); // 同框重开不空涨
    assert_eq!(p.epoch(), e1);
    p.open_modal(5); // 换框 = 变更
    assert_eq!(p.modal(), Some(5));
    assert!(p.epoch() > e1);
    let e2 = p.epoch();
    p.close_modal();
    assert_eq!(p.modal(), None);
    assert!(p.epoch() > e2, "收框必须 bump 代际");
    let e3 = p.epoch();
    p.close_modal(); // 关着再关不空涨
    assert_eq!(p.epoch(), e3);
}

// ---- 九修：上池行命中（组件池页点行开跳框）----

#[test]
fn upper_row_hit_roundtrip_with_scroll() {
    let mut p = CfgPage::new();
    p.set_upper(upper(3)); // 5 行
    let r1 = upper_row_rect(1, &UPPER, 0);
    assert_eq!(p.upper_row_at_y(r1.y + 5, &UPPER, 0), Some(1));
    assert_eq!(
        p.upper_row_at_y(r1.y - FIELD_ROW_GAP / 2, &UPPER, 0),
        None,
        "行间隙不算行"
    );
    // 滚动后命中必须带 scroll 维（眼手同尺：行随内容上移）
    let r1s = upper_row_rect(1, &UPPER, 40);
    assert_eq!(p.upper_row_at_y(r1s.y + 5, &UPPER, 40), Some(1));
    // 同一个屏 y：scroll=0 是行间隙，scroll=40 落进上移后的行 1
    let gap_y = r1.y - FIELD_ROW_GAP / 2;
    assert_eq!(p.upper_row_at_y(gap_y, &UPPER, 0), None);
    assert_eq!(
        p.upper_row_at_y(gap_y, &UPPER, 40),
        Some(1),
        "滚动 40px 后间隙位已变成行 1 的行体"
    );
    assert!(
        p.upper_row_at_y(UPPER.y - 1, &UPPER, 0).is_none(),
        "池外不算行"
    );
}

#[test]
fn snap_carries_tab_and_modal_dims() {
    let mut p = CfgPage::new();
    p.set_tab(1);
    p.open_modal(4);
    let s = p.snap();
    assert_eq!(s.tab, 1, "快照必须带 tab 维（涂装分流读它）");
    assert_eq!(s.modal, Some(4), "快照必须带 modal 维（跳框涂装读它）");
}
