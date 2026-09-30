//! mail_card.rs — 信箱入口卡内容核（BAR-212，2026-09-30 用户立项：
//! 「na 页面右边的解析页……左竖列做成信箱的两个入口」→ 同日拍板细化：
//! 入口卡放**左下常驻槽**（「就跟 tmux 窗口一样」——与右下 tmux 卡
//! 镜像的钉底常驻件，永不被滚出屏）。
//!
//! 卡面：卡头「信箱」+ 两个入口行（三级框行主形态，行 = 册标题左锚 +
//! 计数字右锚——「主册 · N 封」/「NA信箱 · N 封」）。点入口行 = 开
//! 页面级信件列表卡（mail_list），不进会话池。
//!
//! 本册 = 自报高 + 卡内几何/命中（A 档纯逻辑）；涂装在 termview
//! （眼手同尺：两边吃本册同一份 layout）；行计数数据面 = mail_feed
//! 快照（涂装直读，本册不揣度）。

use crate::mail_feed::MailKey;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 入口行高 = 钮高（3 格——宪法「最小的框 ≥3 格」）
pub const ENTRY_H: u32 = pp::BTN_H;
/// 入口行数（两册，行序 = MailKey::all() 声明序——位置钉死）
pub const N_ENTRIES: usize = 2;

/// 卡自报高（恒定，A 档账唯一源）：上下留白 + 卡头 + 行距 +
/// N 入口行（行间距 ROW_GAP）
pub fn card_h() -> u32 {
    pp::CARD_PAD_V * 2
        + pp::ROW_H
        + pp::ROW_GAP
        + N_ENTRIES as u32 * ENTRY_H
        + (N_ENTRIES as u32 - 1) * pp::ROW_GAP
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct MailCardLayout {
    pub card: PoolRect,
    /// 卡头「信箱」
    pub header: PoolRect,
    /// 入口行（i → MailKey::all()[i]）
    pub rows: [PoolRect; N_ENTRIES],
}

/// 布局纯函数：卡外框由三区排布器配给（parser_chain::slot_rect——
/// 左下常驻槽钉底归排布器，本卡不二次揣度屏寸）
pub fn layout_in(card: PoolRect) -> MailCardLayout {
    let cx = card.x + i64::from(pp::CARD_PAD_H);
    let cw = card.w.saturating_sub(pp::CARD_PAD_H * 2);
    let header = PoolRect {
        x: cx,
        y: card.y + i64::from(pp::CARD_PAD_V),
        w: cw,
        h: pp::ROW_H,
    };
    let y0 = header.y + i64::from(pp::ROW_H + pp::ROW_GAP);
    let rows = std::array::from_fn(|i| PoolRect {
        x: cx,
        y: y0 + (ENTRY_H + pp::ROW_GAP) as i64 * i as i64,
        w: cw,
        h: ENTRY_H,
    });
    MailCardLayout { card, header, rows }
}

/// 命中（x/y 屏坐标 i64，与涂装同一份 layout）：入口行 = 开该册
/// 列表卡；卡头/空白 = 纯展示不吞（落常驻槽其余件 = 让回面板页，
/// 归壳手势仲裁）
pub fn hit(l: &MailCardLayout, x: i64, y: i64) -> Option<MailKey> {
    for (i, r) in l.rows.iter().enumerate() {
        if x >= r.x && x < r.x + i64::from(r.w) && y >= r.y && y < r.y + i64::from(r.h) {
            return Some(MailKey::all()[i]);
        }
    }
    None
}
