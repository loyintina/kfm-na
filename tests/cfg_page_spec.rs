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

use kfm_na::ui::accent::AccentPair;
use kfm_na::ui::cfg_page::{
    CfgPage, FIELD_BOTTOM_PAD, FIELD_BOX_GAP, FIELD_BOX_H, FIELD_ROW_GAP, FIELD_ROW_H,
    FIELD_TEXT_INSET, FIELD_TRIANGLE_PAD, FIELD_VALUE_MIN_W, LOWER_ROW_H, PAN_GAP_PAGE, PAN_MS,
    POOL_CONTENT_INSET, PanScope, ROW_GAP, RowView, UpperRow, dropdown_panel_rect,
    field_label_rect, field_value_rect, lower_row_rect, upper_row_rect, wrap_field_lines,
};
use kfm_na::ui::dual_pool::{DualPoolSnap, PoolRect};

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

/// 十七修：set_tab/select 吃旧代冻结用的池几何+页色——考题 stub 单源
fn pool_stub() -> DualPoolSnap {
    DualPoolSnap {
        upper: UPPER,
        lower: LOWER,
        upper_scroll: false,
    }
}

fn acc() -> AccentPair {
    AccentPair {
        c1: 0x0011_2233,
        c2: 0x0044_5566,
    }
}

fn acc2() -> AccentPair {
    AccentPair {
        c1: 0x00AA_BBCC,
        c2: 0x00DD_EEFF,
    }
}

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
    p.select(1, 1000, pool_stub(), acc());
    assert_eq!(p.focus(), 1);
    assert!(p.epoch() > e0, "聚焦变更必须 bump 代际（sig 防鬼影）");
    let e1 = p.epoch();
    p.select(1, 2000, pool_stub(), acc()); // 同标重点不重掷
    assert_eq!(p.epoch(), e1);
}

// ---- 下拉（二版核心钉：选项集服务上池内容，不与下池联动）----

#[test]
fn dropdown_pick_sets_option_and_closes_without_touching_lower_focus() {
    let mut p = CfgPage::new();
    p.set_rows(rows());
    p.set_options(opts(2), 0);
    p.toggle_dropdown(1000);
    assert!(p.dropdown_open());
    p.dropdown_pick(2, 2000);
    assert_eq!(p.option_sel(), 2, "点选 = 选项选中位变更");
    assert_eq!(p.focus(), 0, "下拉换选不许动下池聚焦（二版：不联动）");
    assert!(!p.dropdown_open(), "点选后 panel 必须收");
}

#[test]
fn dropdown_pick_clamps_and_same_pick_no_extra_bump() {
    let mut p = CfgPage::new();
    p.set_options(opts(1), 0); // 2 项
    p.toggle_dropdown(1000);
    let e0 = p.epoch();
    p.dropdown_pick(99, 2000);
    assert_eq!(p.option_sel(), 1, "越界点选 clamp 到末项");
    assert!(p.epoch() > e0);
    // 同项重点：只收 panel 时 bump 一次，sel 不空涨——再点同项（panel
    // 已收）必须零代际变化
    let e1 = p.epoch();
    p.dropdown_pick(1, 3000);
    assert_eq!(p.epoch(), e1);
}

#[test]
fn dismiss_on_outside_tap() {
    let mut p = CfgPage::new();
    p.set_options(opts(1), 0);
    p.toggle_dropdown(1000);
    p.dismiss_dropdown(2000);
    assert!(!p.dropdown_open());
    let e = p.epoch();
    p.dismiss_dropdown(3000);
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

// 夹具量宽（十四修动态宽度：几何吃实量宽参数；测试喂定值，涂装侧
// measure_items 实量）——「默认服务器」按 5×30、「本地终端」按 4×30
const LW: u32 = 150;
const VW: u32 = 120;

#[test]
fn trigger_is_first_rows_value_box() {
    let mut p = CfgPage::new();
    p.set_upper(upper(0)); // 首行 is_dropdown = true
    let t = p.trigger_rect(&UPPER, LW, VW);
    let row0 = upper_row_rect(0, &UPPER, 0);
    let lb = field_label_rect(&row0, LW);
    assert_eq!(t, field_value_rect(&row0, lb.w, VW, true));
    assert_eq!(
        t.x + t.w as i64,
        row0.x + row0.w as i64,
        "值框锚行右缘（十四修）"
    );
}

// ---- 字段框行动态宽度（十四修 §五：宽随文字，标签锚左/值锚右，
// 间隔 ≥3 格，超长换行 ≤2 行）----

#[test]
fn field_label_rect_anchors_left_hugging_text() {
    let row = upper_row_rect(0, &UPPER, 0);
    let lb = field_label_rect(&row, LW);
    assert_eq!(lb.x, row.x, "标签块锚行左缘");
    assert_eq!(
        lb.w,
        LW + FIELD_TEXT_INSET * 2,
        "宽 = 实量宽 + 双侧文内边距（固定 12 格列退役）"
    );
    assert_eq!(lb.h, FIELD_BOX_H, "标签块与值框同高 = 3 格");
    assert_eq!(
        lb.y,
        row.y + (FIELD_ROW_H - FIELD_BOX_H) as i64 / 2,
        "3 格高居中于 4 格行（上下各缩半格）"
    );
}

#[test]
fn field_value_rect_anchors_right_hugging_text() {
    let row = upper_row_rect(0, &UPPER, 0);
    let lb = field_label_rect(&row, LW);
    let vb = field_value_rect(&row, lb.w, VW, false);
    assert_eq!(vb.x + vb.w as i64, row.x + row.w as i64, "值框锚行右缘");
    assert_eq!(
        vb.w,
        VW + FIELD_TEXT_INSET * 2,
        "宽 = 实量宽 + 双侧文内边距"
    );
    assert!(
        vb.x - (lb.x + lb.w as i64) >= FIELD_BOX_GAP as i64,
        "两框间隔 ≥3 格（十四修拍板）"
    );
    // 上下呼吸位等宽（六修居中不变）
    assert_eq!(
        row.y + row.h as i64 - (vb.y + vb.h as i64),
        vb.y - row.y,
        "上下呼吸位等宽"
    );
}

#[test]
fn field_value_rect_dropdown_adds_triangle_pad() {
    let row = upper_row_rect(0, &UPPER, 0);
    let a = field_value_rect(&row, 204, VW, false);
    let b = field_value_rect(&row, 204, VW, true);
    assert_eq!(b.w, a.w + FIELD_TRIANGLE_PAD, "下拉行值宽 +▼ 三角位");
    assert_eq!(b.x + b.w as i64, a.x + a.w as i64, "加宽向左吃，右缘不动");
}

#[test]
fn field_value_rect_min_width_and_label_caps() {
    let row = upper_row_rect(0, &UPPER, 0);
    // 空值 = 值框最小宽（收缩顺序：先保值框下限）
    let vb = field_value_rect(&row, 204, 0, false);
    assert_eq!(vb.w, FIELD_VALUE_MIN_W, "值框最小 = 4 格 + 双侧文内边距");
    // 标签超长：让到上限 = 行宽 − 3 格间隔 − 值框最小宽
    let lb = field_label_rect(&row, 100_000);
    assert_eq!(
        lb.w,
        row.w - FIELD_BOX_GAP - FIELD_VALUE_MIN_W,
        "标签块上限 = 给值框留最小位 + 3 格间隔"
    );
    // 值超长：上限 = 行宽 − 3 格间隔 − 标签块实际宽
    let vb2 = field_value_rect(&row, lb.w, 100_000, false);
    assert_eq!(
        vb2.w,
        row.w - FIELD_BOX_GAP - lb.w,
        "值框上限 = 行宽 − 间隔 − 标签块实际宽"
    );
    assert_eq!(
        vb2.x,
        lb.x + lb.w as i64 + FIELD_BOX_GAP as i64,
        "顶满时间隔恰好 3 格"
    );
}

#[test]
fn wrap_field_lines_greedy_max_two() {
    // 10 字各 20px、单行容量 105 → 5 字/行
    let widths = [20.0; 10];
    let lines = wrap_field_lines(&widths, 105.0);
    assert_eq!(lines.len(), 2, "最多 2 行（十四修拍板）");
    assert_eq!(lines[0], (0, 5, 100.0), "首行贪心装满");
    assert_eq!(lines[1], (5, 10, 100.0), "余量全进末段（再超 = 涂装裁剪）");
    // 装得下不换行
    assert_eq!(wrap_field_lines(&widths[..4], 105.0), vec![(0, 4, 80.0)]);
    // 空串 = 零行
    assert!(wrap_field_lines(&[], 105.0).is_empty());
    // 单字超宽也成行（裁剪归涂装）
    assert_eq!(wrap_field_lines(&[200.0], 105.0), vec![(0, 1, 200.0)]);
}

#[test]
fn dropdown_panel_opens_downward() {
    // 宪法 §六：顶部栏向下弹（反了弹出屏外——kfmv4 教训）
    let mut p = CfgPage::new();
    p.set_upper(upper(0));
    p.set_options(opts(3), 0); // 4 项
    let t = p.trigger_rect(&UPPER, LW, VW);
    let panel = p.dropdown_panel_rect(&UPPER, 10_000, LW, VW, 0);
    assert_eq!(panel.y, t.y + t.h as i64, "panel 从值框下缘起弹（六修）");
    assert_eq!(panel.x, t.x, "panel 与触发器同宽同左缘");
    assert_eq!(panel.h, 4 * FIELD_ROW_H);
    let clamped = p.dropdown_panel_rect(&UPPER, 3 * FIELD_ROW_H, LW, VW, 0);
    assert_eq!(clamped.h, 3 * FIELD_ROW_H, "max_h 钳制");
}

#[test]
fn dropdown_item_hit() {
    let mut p = CfgPage::new();
    p.set_upper(upper(0));
    p.set_options(opts(3), 0);
    let panel = p.dropdown_panel_rect(&UPPER, 10_000, LW, VW, 0);
    assert_eq!(
        p.dropdown_item_at_y(panel.y + 1, &UPPER, 10_000, LW, VW, 0),
        Some(0)
    );
    assert_eq!(
        p.dropdown_item_at_y(
            panel.y + 3 * FIELD_ROW_H as i64 + 1,
            &UPPER,
            10_000,
            LW,
            VW,
            0
        ),
        Some(3)
    );
    assert_eq!(
        p.dropdown_item_at_y(panel.y - 1, &UPPER, 10_000, LW, VW, 0),
        None
    );
    assert_eq!(
        p.dropdown_item_at_y(panel.y + panel.h as i64 + 1, &UPPER, 10_000, LW, VW, 0),
        None
    );
}

// ---- 上池内容高（喂双池数学钉）----

#[test]
fn upper_content_h_exact_with_bottom_pad() {
    // 十四修：末行距池底框线 1 格——内容高末尾 +CELL_H（变异：删掉即红）
    let mut p = CfgPage::new();
    assert_eq!(p.upper_content_h(), 0, "空上池内容高 0（占位归双池）");
    p.set_upper(upper(0)); // 2 行
    assert_eq!(
        p.upper_content_h(),
        POOL_CONTENT_INSET as u32 + 2 * FIELD_ROW_H + FIELD_ROW_GAP as u32 + FIELD_BOTTOM_PAD,
        "内容高 = 内边距 + 行 + 隙 + 末行底距 1 格"
    );
    assert_eq!(FIELD_BOTTOM_PAD, kfm_na::termview::CELL_H, "底距 = 1 格");
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
    let t0 = p.trigger_rect(&UPPER, LW, VW);
    p.scroll_upper_by(120, pool_h);
    let t1 = p.trigger_rect(&UPPER, LW, VW);
    assert_eq!(t1.y, t0.y - 120, "触发器随内容一起滚");
    // 下拉命中同尺：panel 跟触发器走，项命中必须带滚动维
    let panel = p.dropdown_panel_rect(&UPPER, 10_000, LW, VW, 0);
    assert_eq!(panel.y, t1.y + t1.h as i64);
    assert_eq!(
        p.dropdown_item_at_y(panel.y + 1, &UPPER, 10_000, LW, VW, 0),
        Some(0)
    );
    assert_eq!(
        p.dropdown_item_at_y(t1.y + 1, &UPPER, 10_000, LW, VW, 0),
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
    p.select(0, 1000, pool_stub(), acc());
    p.scroll_upper_by(120, 400);
    p.toggle_dropdown(1000);
    p.open_modal(3);
    let e0 = p.epoch();
    p.set_tab(1, 1000, pool_stub(), acc());
    assert_eq!(p.tab(), 1);
    assert!(p.epoch() > e0, "切页必须 bump 代际（sig 防鬼影）");
    assert_eq!(p.upper_scroll(), 0, "切页上池滚动归零——新页不继承旧滚动");
    assert!(!p.dropdown_open(), "切页下拉收");
    assert_eq!(p.modal(), None, "切页跳框收");
    let e1 = p.epoch();
    p.set_tab(1, 1000, pool_stub(), acc()); // 同页重点不空涨
    assert_eq!(p.epoch(), e1);
}

#[test]
fn set_tab_back_and_forth() {
    let mut p = CfgPage::new();
    p.set_tab(1, 1000, pool_stub(), acc());
    p.set_tab(0, 2000, pool_stub(), acc());
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
    p.set_tab(1, 1000, pool_stub(), acc());
    p.open_modal(4);
    let s = p.snap(1000);
    assert_eq!(s.tab, 1, "快照必须带 tab 维（涂装分流读它）");
    assert_eq!(s.modal, Some(4), "快照必须带 modal 维（跳框涂装读它）");
}

// ---- 十五修：下池光标滑行（宪法 §五 池区动画条款；BAR-094 改缓动核）----

fn three_rows() -> Vec<RowView> {
    ["a", "b", "c"]
        .iter()
        .map(|t| RowView {
            title: t.to_string(),
            meta: String::new(),
        })
        .collect()
}

#[test]
fn select_cursor_eases_slides_and_settles() {
    let mut p = CfgPage::new();
    p.set_rows(three_rows());
    p.select(2, 1000, pool_stub(), acc());
    // 缓动起步：瞬时值在起点（重定基连续性——elapsed 0 位置 = from）
    assert_eq!(p.cursor_row(1000), 0.0, "点选瞬间光标在旧行不跳变");
    assert!(p.cursor_fx_active(1000), "未收敛 = 活性探针真（帧泵闸）");
    let mid = p.cursor_row(1050);
    assert!(mid > 0.0 && mid != 2.0, "滑行中途是中间值（瞬移回潮钉）");
    // 快照必须吃同一维（涂装选中框的唯一读数口）
    assert_eq!(p.snap(1050).cursor_row, mid, "快照与探针同一份读数");
    // BAR-094：PAN_MS 同钟——250ms 整点贴死（弹簧 600ms 兜底已废）
    assert_eq!(p.cursor_row(1250), 2.0, "缓动时长 = PAN_MS，贴死 == focus");
    assert!(!p.cursor_fx_active(1250), "收敛停脏（零空烧）");
}

/// BAR-094 钉：光标缓动**无过冲** + 与上池平移**同钟同曲线**。
/// 欠阻尼弹簧会越过目标 ≈2.5% 再回摆（用户真机逐帧判「瞬移+过冲」，
/// 未经用户要求——除名）；缓动全程单调夹在 [from, target] 内，且
/// 相对进度与 pan t 每一采样点恒等（select 同刻挂账）
#[test]
fn bar094_cursor_no_overshoot_and_in_sync_with_pan() {
    let mut p = CfgPage::new();
    p.set_rows(three_rows());
    p.select(2, 1000, pool_stub(), acc());
    // 无过冲：全程 10ms 步进采样，行号恒 ∈ [0, 2]（弹簧过冲会 >2）
    for ms in (0..=250).step_by(10) {
        let row = p.cursor_row(1000 + ms);
        assert!(
            (0.0..=2.0).contains(&row),
            "t={ms}ms 光标出界（过冲回潮）：{row}"
        );
    }
    // 同步钉：任一时刻光标相对进度 == 平移 t（同刻挂账同曲线）
    for ms in [25, 62, 125, 187, 240] {
        let Some(pan) = p.snap(1000 + ms).pan else {
            panic!("平移账 {ms}ms 内必须在场");
        };
        let progress = p.cursor_row(1000 + ms) / 2.0; // 0→2 行的相对进度
        assert_eq!(
            progress, pan.t,
            "t={ms}ms 光标进度与平移进度必须恒等（同步律）"
        );
    }
}

/// BAR-095 分域探针钉：Upper 平移在场 → pan_upper_active 真（壳层喂
/// glide 缓动）；Page 平移/无平移/贴死 → 假（壳层喂 set 直通——新页
/// 池高起步帧就位）
#[test]
fn bar095_pan_upper_scope_probe() {
    let mut p = CfgPage::new();
    p.set_rows(three_rows());
    p.select(2, 1000, pool_stub(), acc()); // Upper 域挂账
    assert!(p.pan_upper_active(1000), "Upper 平移期内 = 真（glide 域）");
    assert!(p.pan_upper_active(1249), "贴死前一刻仍真");
    assert!(!p.pan_upper_active(1250), "贴死 = 假（回直通域）");
    p.set_tab(1, 2000, pool_stub(), acc()); // Page 域挂账
    assert!(!p.pan_upper_active(2000), "Page 平移期内也是假（直通域）");
    assert!(p.pan_active(2000), "但平移账本身在场（活性探针不混淆）");
}

#[test]
fn select_cursor_rebase_no_jump() {
    let mut p = CfgPage::new();
    p.set_rows(three_rows());
    p.select(2, 1000, pool_stub(), acc());
    let mid = p.cursor_row(1050);
    p.select(1, 1050, pool_stub(), acc()); // 滑行中途改目标
    assert_eq!(
        p.cursor_row(1050),
        mid,
        "重定基瞬间位置连续（来回狂点不跳变）"
    );
    assert_eq!(p.cursor_row(1300), 1.0, "续滑（PAN_MS 内重定基）收敛新目标");
}

#[test]
fn set_tab_cursor_lands_first_row_directly() {
    let mut p = CfgPage::new();
    p.set_rows(three_rows());
    p.select(2, 1000, pool_stub(), acc());
    p.set_tab(1, 1000, pool_stub(), acc()); // 滑行中途切标签页
    assert_eq!(
        p.cursor_row(1001),
        0.0,
        "切标签页光标直接落首行不滑行（十五修拍板）"
    );
    assert!(!p.cursor_fx_active(1001));
}

#[test]
fn set_rows_clamp_cursor_follows_without_animation() {
    let mut p = CfgPage::new();
    p.set_rows(three_rows());
    p.select(2, 1000, pool_stub(), acc());
    assert_eq!(p.cursor_row(1700), 2.0);
    p.set_rows(vec![RowView {
        title: "only".into(),
        meta: String::new(),
    }]);
    assert_eq!(p.focus(), 0, "行表缩水 focus clamp");
    assert_eq!(
        p.cursor_row(1701),
        0.0,
        "clamp 光标同步落点不动画（非用户点选）"
    );
}

// ---- 十五修：下拉开合动画（宪法 §六 开合两件）----

#[test]
fn dropdown_progress_grows_ease_out_and_settles() {
    let mut p = CfgPage::new();
    p.set_options(opts(2), 0);
    assert_eq!(p.dropdown_progress(500), 0.0, "未开过 = 进度 0");
    p.toggle_dropdown(1000);
    assert_eq!(p.dropdown_progress(1000), 0.0, "开合瞬间进度 0（生长起点）");
    assert!(p.dropdown_fx_active(1000));
    let mid = p.dropdown_progress(1125); // 半程 125/250
    assert!(
        (mid - 0.875).abs() < 1e-4,
        "展开半程 = ease_out(0.5) = 0.875（三次缓出精确值），实得 {mid}"
    );
    assert_eq!(p.dropdown_progress(1250), 1.0, "250ms 贴死全高");
    assert!(!p.dropdown_fx_active(1250), "展开毕活性探针假");
    assert_eq!(p.snap(1125).dropdown_progress, mid, "快照同一份读数");
}

#[test]
fn dropdown_pick_two_phase_sel_slide_then_panel_close() {
    // 2026-09-14 用户拍板两段时序（宪法 §六②）：点他行 = Ⅰ段选中
    // 细框 160ms 滑行（面板冻结等它）→ Ⅱ段面板 180ms ease-in 收
    let mut p = CfgPage::new();
    p.set_options(opts(2), 0); // 3 项
    p.toggle_dropdown(1000);
    assert_eq!(p.dropdown_progress(1250), 1.0);
    p.dropdown_pick(2, 1300);
    assert_eq!(p.option_sel(), 2, "换选即时生效（触发器值即换）");
    assert!(
        !p.dropdown_open(),
        "点选即翻有效关态——收敛后点触发器必须能重开，账面不许骗人"
    );
    // Ⅰ段：面板全程冻结，选中细框滑行
    assert_eq!(p.dropdown_progress(1300), 1.0, "Ⅰ段起点面板冻结在全开");
    assert_eq!(p.dropdown_progress(1459), 1.0, "Ⅰ段全程面板冻结");
    assert!(p.dropdown_fx_active(1400), "Ⅰ段活性在");
    let sel_mid = p.option_sel_f(1380); // 半程 80/160
    assert!(
        (sel_mid - 1.75).abs() < 1e-4,
        "细框滑行半程 = 2×ease_out(0.5) = 1.75（精确值），实得 {sel_mid}"
    );
    assert_eq!(p.option_sel_f(1460), 2.0, "160ms 细框贴死新行");
    assert_eq!(p.snap(1380).option_sel_f, sel_mid, "快照同一份滑行读数");
    // Ⅱ段：面板 ease-in 收
    let mid = p.dropdown_progress(1550); // 半程 90/180
    assert!(
        (mid - 0.875).abs() < 1e-4,
        "Ⅱ段半程 = 1-ease_in(0.5) = 0.875，实得 {mid}"
    );
    assert_eq!(p.dropdown_progress(1640), 0.0, "160+180ms 贴死全收");
    assert!(!p.dropdown_fx_active(1640), "两段全毕活性探针假");
    // 收敛后点触发器 = 重开 fresh（账面烂账不许把重开误判成收）
    p.toggle_dropdown(2000);
    assert!(p.dropdown_open(), "收敛后重开必须真开");
    assert_eq!(p.dropdown_progress(2000), 0.0, "重开从 0 长（续自烂账）");
}

#[test]
fn dropdown_dismiss_mid_pick_move_cancels_and_closes() {
    // Ⅰ段滑行中点他处 = 取消滑行从冻结进度直接收（换选不换回答案）
    let mut p = CfgPage::new();
    p.set_options(opts(2), 0);
    p.toggle_dropdown(1000);
    p.dropdown_pick(1, 1300);
    p.dismiss_dropdown(1400); // Ⅰ段中
    assert_eq!(p.option_sel(), 1, "取消滑行不换回——换选已生效");
    assert_eq!(
        p.dropdown_progress(1400),
        1.0,
        "取消点进度 = 冻结值续收（不瞬消）"
    );
    assert_eq!(p.dropdown_progress(1580), 0.0, "180ms 收尽");
    assert!(!p.dropdown_fx_active(1580));
    let e = p.epoch();
    p.dismiss_dropdown(2000); // 收敛后再 dismiss = 不空涨代际
    assert_eq!(p.epoch(), e, "挂账收敛后 dismiss 必须零代际变化");
}

#[test]
fn dropdown_pick_retarget_mid_move_rebases() {
    // Ⅰ段滑行中改主意点别的行 = 从当时滑行位置重定基滑向新目标
    let mut p = CfgPage::new();
    p.set_options(opts(3), 0); // 4 项
    p.toggle_dropdown(1000);
    p.dropdown_pick(2, 1300);
    p.dropdown_pick(3, 1380); // Ⅰ段半程改选
    assert_eq!(p.option_sel(), 3);
    let s = p.option_sel_f(1380);
    assert!(
        s > 0.0 && s < 2.0,
        "重定基起点 = 当时滑行位置（0..2 之间），实得 {s}"
    );
    assert_eq!(p.dropdown_progress(1380), 1.0, "面板仍冻结等滑行");
    assert_eq!(p.option_sel_f(1540), 3.0, "重定基 160ms 贴死新行");
    assert_eq!(p.dropdown_progress(1720), 0.0, "Ⅱ段 180ms 收尽");
}

#[test]
fn dropdown_reopen_mid_close_continues_from_current() {
    let mut p = CfgPage::new();
    p.set_options(opts(2), 0);
    p.toggle_dropdown(1000);
    p.dismiss_dropdown(1300); // 展开满后点外收
    let mid = p.dropdown_progress(1390);
    assert!(mid > 0.0 && mid < 1.0, "收起中途是中间进度");
    p.toggle_dropdown(1390); // 收起中途再点触发器 = 重开
    assert!(p.dropdown_open());
    assert_eq!(
        p.dropdown_progress(1390),
        mid,
        "重开瞬间进度连续（从余影处长）"
    );
    assert_eq!(p.dropdown_progress(1640), 1.0, "续长 250ms 贴死全高");
}

#[test]
fn dropdown_dismiss_now_zeroes_afterimage() {
    let mut p = CfgPage::new();
    p.set_options(opts(2), 0);
    p.toggle_dropdown(1000);
    p.dismiss_dropdown(1300);
    assert!(p.dropdown_progress(1390) > 0.0, "收起中途余影在");
    p.dropdown_dismiss_now();
    assert_eq!(
        p.dropdown_progress(1390),
        0.0,
        "余影点按即时清零（不穿透触摸）"
    );
    assert!(!p.dropdown_fx_active(1390));
    let e = p.epoch();
    p.dropdown_dismiss_now(); // 已清零重点不空涨
    assert_eq!(p.epoch(), e);
}

#[test]
fn set_tab_clears_dropdown_afterimage() {
    let mut p = CfgPage::new();
    p.set_options(opts(2), 0);
    p.toggle_dropdown(1000);
    p.dismiss_dropdown(1300);
    p.set_tab(1, 1000, pool_stub(), acc()); // 收起中途切页
    assert_eq!(
        p.dropdown_progress(1301),
        0.0,
        "切标签页下拉余影即时清零（新页不继承浮层）"
    );
}

// ---- 十七修 §六「面与内容一体」：视口平移切页 ----

#[test]
fn set_tab_hangs_page_pan_with_dir_and_frozen_epoch() {
    let mut p = CfgPage::new();
    p.set_rows(rows());
    p.set_upper(upper(0));
    p.set_options(opts(1), 1);
    p.scroll_upper_by(50, UPPER.h);
    let before_opts = opts(1);
    let before_scroll = p.upper_scroll();
    // 0 → 1 = 前进 = dir +1（内容左移）
    p.set_tab(1, 1000, pool_stub(), acc());
    let s = p.snap(1000);
    let pan = s.pan.as_ref().expect("切标签必须挂页面级平移账");
    assert_eq!(pan.scope, PanScope::Page);
    assert_eq!(pan.dir, 1, "标签右移 = 前进 = dir +1（方向律）");
    assert_eq!(pan.t, 0.0, "起点帧 t=0");
    // 旧代冻结：选项/滚动/页色/池几何全是切换瞬间的封存
    assert_eq!(pan.old.options, before_opts);
    assert_eq!(pan.old.upper_scroll, before_scroll);
    assert_eq!(pan.old.accent, acc());
    assert_eq!(pan.old.pool.upper, UPPER);
    // 时序（十八修：ease-in-out cubic——起步收步皆柔，取代 ease-out 的
    // 起步满速「太块」读感）：1/4 程 = 0.0625、半程 = 0.5，贴死后出 None
    let q = p.snap(1000 + PAN_MS / 4).pan.expect("前段账在");
    assert!(
        (q.t - 0.0625).abs() < 0.01,
        "ease-in-out cubic 1/4 程 = 0.0625（t={}）",
        q.t
    );
    let mid = p.snap(1000 + PAN_MS / 2).pan.expect("中帧账在");
    assert!(
        (mid.t - 0.5).abs() < 0.01,
        "ease-in-out cubic 半程 = 0.5（t={}）",
        mid.t
    );
    assert!(
        p.snap(1000 + PAN_MS).pan.is_none(),
        "250ms 贴死 = 稳态单代（pan None）"
    );
}

#[test]
fn set_tab_backward_dir_negative_and_same_tab_no_pan() {
    let mut p = CfgPage::new();
    p.set_tab(1, 1000, pool_stub(), acc());
    p.set_tab(0, 2000, pool_stub(), acc2());
    let pan = p.snap(2000).pan.expect("回切也挂账");
    assert_eq!(pan.dir, -1, "标签左移 = 后退 = dir −1（内容右移）");
    assert_eq!(
        pan.old.accent,
        acc2(),
        "旧代封的是回切前的页色（调用方喂入的当前页色）"
    );
    // 同页重点：不挂账不空涨
    let settled = 2000 + PAN_MS + 100;
    let e = p.epoch();
    p.set_tab(0, settled + 1000, pool_stub(), acc());
    assert_eq!(p.epoch(), e);
    assert!(p.snap(settled + 1000).pan.is_none());
}

#[test]
fn select_hangs_upper_pan_and_same_focus_no_pan() {
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
        RowView {
            title: "c".into(),
            meta: String::new(),
        },
    ]);
    p.select(2, 1000, pool_stub(), acc());
    let s = p.snap(1000);
    let pan = s.pan.as_ref().expect("选行必须挂上池级平移账");
    assert_eq!(pan.scope, PanScope::Upper, "下池选行 = 上池级平移");
    assert_eq!(pan.dir, 1, "光标下移 = 前进 = dir +1");
    // 上溯 = 后退
    p.select(0, 2000, pool_stub(), acc());
    assert_eq!(p.snap(2000).pan.as_ref().unwrap().dir, -1);
    // 同标重点：不挂账（账贴死后）
    let settled = 2000 + PAN_MS + 100;
    let e = p.epoch();
    p.select(0, settled, pool_stub(), acc());
    assert_eq!(p.epoch(), e);
    assert!(p.snap(settled).pan.is_none());
}

#[test]
fn pan_active_probe_drives_frame_pump() {
    let mut p = CfgPage::new();
    p.set_tab(1, 1000, pool_stub(), acc());
    assert!(p.pan_active(1000), "账起 = 活性（帧泵必须续帧）");
    assert!(p.pan_active(1000 + PAN_MS - 1));
    assert!(!p.pan_active(1000 + PAN_MS), "贴死 = 活性灭（零空烧）");
}

// ---- 十七修 BAR-090：下拉面板宽 = max(触发器, 最长选项文+边距)，钳右缘 ----

#[test]
fn bar090_dropdown_panel_widens_to_longest_option() {
    // 触发器宽按实量；内容最小宽 480 → 面板加宽到 480——右缘与触发器
    // 右缘对齐（值框锚行右缘 ≡ 池内容内缘，加宽只能向左长）
    let p = dropdown_panel_rect(3, &UPPER, 10_000, 0, true, 100, 50, 480);
    let t = kfm_na::ui::cfg_page::trigger_rect(&UPPER, 0, true, 100, 50);
    assert_eq!(p.x + p.w as i64, t.x + t.w as i64, "右缘与触发器右缘对齐");
    assert_eq!(p.w, t.w.max(480), "宽 = max(触发器, 内容最小宽)");
    assert!(p.x < t.x, "加宽向左长（左缘 < 触发器左缘）");
}

#[test]
fn bar090_dropdown_panel_not_narrower_than_trigger() {
    // 内容最小宽小于触发器 → 面板 = 触发器宽（不收缩），左右缘全对齐
    let p = dropdown_panel_rect(2, &UPPER, 10_000, 0, true, 100, 50, 10);
    let t = kfm_na::ui::cfg_page::trigger_rect(&UPPER, 0, true, 100, 50);
    assert_eq!(p.w, t.w);
    assert_eq!(p.x, t.x);
}

#[test]
fn bar090_dropdown_panel_left_edge_clamped_to_pool_inner() {
    // 内容最小宽天价 → 左缘钳上池内容左内缘（不越池框），右缘不动
    let p = dropdown_panel_rect(2, &UPPER, 10_000, 0, true, 100, 50, 100_000);
    let t = kfm_na::ui::cfg_page::trigger_rect(&UPPER, 0, true, 100, 50);
    assert_eq!(p.x, UPPER.x + POOL_CONTENT_INSET, "左缘钳池内容左内缘");
    assert_eq!(p.x + p.w as i64, t.x + t.w as i64, "右缘不动");
}

// ---- 十九修 D8：平移升合成期——偏移纯函数（涂装域与合成域同尺唯一来源） ----

#[test]
fn pan_offsets_forward_backward_and_gap_law() {
    use kfm_na::ui::cfg_page::pan_offsets;
    let pw = 1000_i64;
    let travel = pw + PAN_GAP_PAGE; // 视口宽 + 留隙 G（theme §七留隙律）
    // 前进 dir=+1：起点旧在位/新在 travel 外；终点旧出尽/新在位
    assert_eq!(
        pan_offsets(1, 0.0, travel),
        (0, travel),
        "起点帧旧在位新屏外"
    );
    assert_eq!(
        pan_offsets(1, 1.0, travel),
        (-travel, 0),
        "终点帧旧出尽新就位"
    );
    assert_eq!(
        pan_offsets(1, 0.5, travel),
        (-travel / 2, travel / 2),
        "半程对称各走半程"
    );
    // 后退 dir=−1 镜像（新代从左缘进）
    assert_eq!(pan_offsets(-1, 0.0, travel), (0, -travel));
    assert_eq!(pan_offsets(-1, 1.0, travel), (travel, 0));
    // 留隙律：任意时刻双代左缘距 = travel（= 视口宽 + G），减去池宽
    // 即净隙 G——挤贴 = 元素替换读感，十八修 §七用户实机判非视口平移，
    // 合成域必须保同一律（两方向对称取绝对值）
    for t in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let (d_old, d_new) = pan_offsets(1, t, travel);
        assert_eq!(
            (d_new - d_old).abs() - pw,
            PAN_GAP_PAGE,
            "t={t} 双代净隙恒 G"
        );
        let (d_old, d_new) = pan_offsets(-1, t, travel);
        assert_eq!((d_new - d_old).abs() - pw, PAN_GAP_PAGE, "t={t} 后退同律");
    }
}
