//! link_card.rs — 连接服务合并卡内容核（2026-09-20 用户拍板：连接卡 +
//! 服务卡合并成一张二级卡两竖列，「第二张和第三张卡能合并一下吗？
//! 占空间太大了，可以做成两竖列」——省一段卡头+一段卡间距的纵高）。
//!
//! 左列 = 连接（mini 卡头「连接 · 状态词」+ 四字段行：目标/本地口/
//! 重拉/错误 + [重连] 钮钉左列底）；右列 = 服务（mini 卡头
//! 「服务 · 状态词」+ 四字段行：后端/在线/会话/错误 + 会话行表，
//! 无分隔线）。**文案面零改动**：继续吃 conn_card::current() /
//! svc_card::current() 同两份快照，本册只重写几何/命中；涂装在
//! termview（眼手同尺：两边吃本册同一份 layout）。
//!
//! 布局（网格制，与 tmux 卡同池区）：卡 = tmux 卡同宽、接其正下方
//! （间距 LINK_GAP）。两竖列：列宽 = (内容宽 − COL_GAP)/2。卡高 =
//! PAD_V·2 + max(左列高, 右列高(n 会话))——左列恒定（钮钉列底），
//! 右列随会话数伸缩。tmux 卡侧按 inset_extra_live() 预留底部带
//! （与实高同源钉死：两处各写一份必漂移 = 两卡相叠/底部空洞鬼影）。

use crate::termview::CELL_H;
use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;
use crate::ui::svc_card as sc;

/// 与 tmux 卡的间距（与合并前两卡同档）
pub const LINK_GAP: u32 = CELL_H;
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

/// 左列内容高（恒定）：mini 卡头 + 行距 + 字段块 + 行距 + [重连] 钮
pub const LEFT_H: u32 = pp::ROW_H + pp::ROW_GAP + FIELDS_BLOCK + pp::ROW_GAP + pp::BTN_H;

/// 右列内容高（n 会话）：mini 卡头 + 行距 + 字段块 + 会话行区
/// （n>0 时 行距 + n 行 + (n-1) 行距）
pub fn right_h(n_sessions: usize) -> u32 {
    let base = pp::ROW_H + pp::ROW_GAP + FIELDS_BLOCK;
    if n_sessions == 0 {
        base
    } else {
        base + pp::ROW_GAP + n_sessions as u32 * SESS_H + (n_sessions as u32 - 1) * SESS_GAP
    }
}

/// 卡高账（A 档纯函数）：PAD_V·2 + max(左列, 右列(n))
pub fn card_h(n_sessions: usize) -> u32 {
    pp::CARD_PAD_V * 2 + LEFT_H.max(right_h(n_sessions))
}

/// tmux 卡为本卡预留的底部带（n 会话时）：与实高同源钉死
pub fn inset_extra(n_sessions: usize) -> u32 {
    LINK_GAP + card_h(n_sessions)
}

/// tmux 卡为本卡预留的底部带：与实高同源钉死（活件——会话数换代
/// → 壳脏帧 → 几何同步换代，眼手同尺不断代）
pub fn inset_extra_live() -> u32 {
    inset_extra(sc::current().lines.len())
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct LinkLayout {
    pub card: PoolRect,
    /// 左列 mini 卡头「连接 · 状态词」
    pub lheader: PoolRect,
    /// 左列四字段行
    pub lfields: [PoolRect; N_FIELDS],
    /// [重连] 钮（钉左列底）
    pub button: PoolRect,
    /// 右列 mini 卡头「服务 · 状态词」
    pub rheader: PoolRect,
    /// 右列四字段行
    pub rfields: [PoolRect; N_FIELDS],
    /// 右列会话行（n 行）
    pub sessions: Vec<PoolRect>,
}

/// 布局纯函数：与 tmux 卡同宽、接其正下方（几何只从 tmux 卡推——
/// 屏寸/池区都已在 tmux 卡里约过，本卡不二次揣度）
pub fn layout(tmux_card: &PoolRect, n_sessions: usize) -> LinkLayout {
    let card = PoolRect {
        x: tmux_card.x,
        y: tmux_card.y + i64::from(tmux_card.h) + i64::from(LINK_GAP),
        w: tmux_card.w,
        h: card_h(n_sessions),
    };
    let cx = card.x + i64::from(pp::CARD_PAD_H);
    let cw = card.w.saturating_sub(pp::CARD_PAD_H * 2);
    let col_w = cw.saturating_sub(pp::COL_GAP) / 2;
    let lx = cx;
    let rx = cx + i64::from(col_w + pp::COL_GAP);
    let y0 = card.y + i64::from(pp::CARD_PAD_V);
    let header = |x: i64| PoolRect {
        x,
        y: y0,
        w: col_w,
        h: pp::ROW_H,
    };
    let fy = y0 + i64::from(pp::ROW_H + pp::ROW_GAP);
    let fields = |x: i64| -> [PoolRect; N_FIELDS] {
        std::array::from_fn(|i| PoolRect {
            x,
            y: fy + (FIELD_H + FIELD_GAP) as i64 * i as i64,
            w: col_w,
            h: FIELD_H,
        })
    };
    // [重连] 钮钉左列底（卡底 − PAD_V − BTN_H）：右列长时钮跟底走，
    // 不许吊在字段行后悬空
    let button = PoolRect {
        x: lx,
        y: card.y + i64::from(card.h) - i64::from(pp::CARD_PAD_V + pp::BTN_H),
        w: col_w,
        h: pp::BTN_H,
    };
    let sy = fy + i64::from(FIELDS_BLOCK + pp::ROW_GAP);
    let sessions = (0..n_sessions)
        .map(|i| PoolRect {
            x: rx,
            y: sy + (SESS_H + SESS_GAP) as i64 * i as i64,
            w: col_w,
            h: SESS_H,
        })
        .collect();
    LinkLayout {
        card,
        lheader: header(lx),
        lfields: fields(lx),
        button,
        rheader: header(rx),
        rfields: fields(rx),
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
