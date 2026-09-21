//! link_card.rs — 连接服务合并卡内容核（2026-09-20 用户拍板：连接卡 +
//! 服务卡合并成一张二级卡，「第二张和第三张卡能合并一下吗？占空间太大
//! 了」——省一段卡头+一段卡间距的纵高；同日两竖列。2026-09-21 三区
//! 排布 v2：本卡归**右上滚动区**（右列窄区，RIGHT_COL_W 16 格）——
//! 两竖列改**两段纵排**：连接段在上、服务段在下，合并契约不变）。
//!
//! 连接段 = mini 卡头「连接 · 状态词」+ 四字段行（目标/本地口/重拉/
//! 错误）+ [重连] 钮（杀娃重拉，不等退避）；服务段 = mini 卡头
//! 「服务 · 状态词」+ 四字段行（后端/在线/会话/错误）+ 会话行表，
//! 无分隔线。**文案面零改动**：继续吃 conn_card::current() /
//! svc_card::current() 同两份快照，本册只重写几何/命中；涂装在
//! termview（眼手同尺：两边吃本册同一份 layout）。
//!
//! 布局（网格制）：卡外框由三区排布器配给（parser_chain::slot_rect——
//! 两轴契约 §四 v2：本卡不再知道「我接在谁下面」「我在哪个区」，落位/
//! 间距/滚动/裁剪归排布器）。段内全宽（窄列不分列）。卡高 =
//! PAD_V·2 + 连接段 + 段距 + 服务段(n 会话)。

use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 字段行高 = mini 卡头行高（2 格，合并前两卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 每列字段行数（左：目标/本地口/重拉/错误；右：后端/在线/会话/错误）
pub const N_FIELDS: usize = 4;
/// 会话行高 = 字段行高
pub const SESS_H: u32 = FIELD_H;
/// 会话行距
pub const SESS_GAP: u32 = FIELD_GAP;

/// 字段行块高（N 行 + (N-1) 行距）
const FIELDS_BLOCK: u32 = N_FIELDS as u32 * FIELD_H + (N_FIELDS as u32 - 1) * FIELD_GAP;

/// 连接段内容高（恒定）：mini 卡头 + 行距 + 字段块 + 行距 + [重连] 钮
pub const LEFT_H: u32 = pp::ROW_H + pp::ROW_GAP + FIELDS_BLOCK + pp::ROW_GAP + pp::BTN_H;

/// 服务段内容高（n 会话）：mini 卡头 + 行距 + 字段块 + 会话行区
/// （n>0 时 行距 + n 行 + (n-1) 行距）
pub fn right_h(n_sessions: usize) -> u32 {
    let base = pp::ROW_H + pp::ROW_GAP + FIELDS_BLOCK;
    if n_sessions == 0 {
        base
    } else {
        base + pp::ROW_GAP + n_sessions as u32 * SESS_H + (n_sessions as u32 - 1) * SESS_GAP
    }
}

/// 卡高账（A 档纯函数）：PAD_V·2 + 连接段 + 段距 + 服务段(n)
/// （2026-09-21 三区 v2：两竖列 → 两段纵排，卡高从「取高列」改「两段
/// 相加」——窄列 stacking 的必然形态）
pub fn card_h(n_sessions: usize) -> u32 {
    pp::CARD_PAD_V * 2 + LEFT_H + pp::ROW_GAP + right_h(n_sessions)
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct LinkLayout {
    pub card: PoolRect,
    /// 连接段 mini 卡头「连接 · 状态词」
    pub lheader: PoolRect,
    /// 连接段四字段行
    pub lfields: [PoolRect; N_FIELDS],
    /// [重连] 钮（连接段尾）
    pub button: PoolRect,
    /// 服务段 mini 卡头「服务 · 状态词」
    pub rheader: PoolRect,
    /// 服务段四字段行
    pub rfields: [PoolRect; N_FIELDS],
    /// 服务段会话行（n 行）
    pub sessions: Vec<PoolRect>,
}

/// 布局纯函数：卡外框由三区排布器配给（parser_chain::slot_rect——
/// 屏寸/区窗/滚动都已在排布器里约过，本卡不二次揣度）。card.h 即
/// 自报高 card_h(n)，调用方（排布器）保证同源。段内全宽纵排：
/// 连接段（头/字段/钮）→ 段距 → 服务段（头/字段/会话行）
pub fn layout_in(card: PoolRect, n_sessions: usize) -> LinkLayout {
    let cx = card.x + i64::from(pp::CARD_PAD_H);
    let cw = card.w.saturating_sub(pp::CARD_PAD_H * 2);
    let y0 = card.y + i64::from(pp::CARD_PAD_V);
    let header = |y: i64| PoolRect {
        x: cx,
        y,
        w: cw,
        h: pp::ROW_H,
    };
    let fields = |y: i64| -> [PoolRect; N_FIELDS] {
        std::array::from_fn(|i| PoolRect {
            x: cx,
            y: y + (FIELD_H + FIELD_GAP) as i64 * i as i64,
            w: cw,
            h: FIELD_H,
        })
    };
    // 连接段
    let lheader = header(y0);
    let fy = y0 + i64::from(pp::ROW_H + pp::ROW_GAP);
    let lfields = fields(fy);
    let by = fy + i64::from(FIELDS_BLOCK + pp::ROW_GAP);
    let button = PoolRect {
        x: cx,
        y: by,
        w: cw,
        h: pp::BTN_H,
    };
    // 服务段（段距一行）
    let ry = by + i64::from(pp::BTN_H + pp::ROW_GAP);
    let rheader = header(ry);
    let rfy = ry + i64::from(pp::ROW_H + pp::ROW_GAP);
    let rfields = fields(rfy);
    let sy = rfy + i64::from(FIELDS_BLOCK + pp::ROW_GAP);
    let sessions = (0..n_sessions)
        .map(|i| PoolRect {
            x: cx,
            y: sy + (SESS_H + SESS_GAP) as i64 * i as i64,
            w: cw,
            h: SESS_H,
        })
        .collect();
    LinkLayout {
        card,
        lheader,
        lfields,
        button,
        rheader,
        rfields,
        sessions,
    }
}

/// 命中结果（只有 [重连] 可点；字段行/卡头/会话行 = 纯展示）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkHit {
    Reconnect,
}

/// 命中（x/y 屏坐标 i64，与涂装同一份 LinkLayout）
pub fn hit(l: &LinkLayout, x: i64, y: i64) -> Option<LinkHit> {
    let b = &l.button;
    if x >= b.x && x < b.x + i64::from(b.w) && y >= b.y && y < b.y + i64::from(b.h) {
        return Some(LinkHit::Reconnect);
    }
    None
}
