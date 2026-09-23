//! link_card.rs — 连接服务合并卡内容核（2026-09-20 用户拍板：连接卡 +
//! 服务卡合并成一张二级卡，「第二张和第三张卡能合并一下吗？占空间太大
//! 了」——省一段卡头+一段卡间距的纵高；同日两竖列。2026-09-21 三区
//! 排布 v2：本卡归**右上滚动区**（右列窄区）——两竖列改**两段纵排**：
//! 连接段在上、服务段在下，合并契约不变。同日 v3：右列放宽为页全区
//! 1/2 比例制 + **钉顶钳高卡内滚**——「弹起后右上区域做卡片的压缩，
//! 里面内容的滑动，不要做卡片的滑动」（卡框钉区顶、高钳进区窗归
//! parser_chain::slot_rect；内容随 scroll 卡内平移归本册——框不动
//! 内容动，与 tmux 卡内滚同语言））。
//!
//! 连接段 = mini 卡头「连接 · 状态词」+ 四字段行（目标/本地口/重拉/
//! 错误）+ [重连] 钮（杀娃重拉，不等退避）；**通道段**（2026-09-24
//! 用户裁决：原「服务」段——后端/在线/会话/错误+会话行——「没什么用」
//! 退役，原位换四口连接状况 + 调试钮）= mini 卡头「通道 · 状态词」+
//! 四字段行（数据9021/反连9022/QUIC62633/QUIC62694）+ 钮行半宽并排
//! [跳闸/投 QUIC]（裁决归 svc_card::toggle_verdict，执行归 tunnel
//! TunnelCmd::TripQuic/HealQuic）+ [重启] 钮（2026-09-23 用户立项：
//! 完全自重启，两段确认与执行归 self_restart——本册只给几何/命中），
//! 无分隔线。**文案面零改动**：继续吃 conn_card::current() /
//! svc_card::current() 同两份快照，本册只重写几何/命中；涂装在
//! termview（眼手同尺：两边吃本册同一份 layout）。
//!
//! 布局（网格制）：卡外框由三区排布器配给（parser_chain::slot_rect——
//! 两轴契约 §四 v2：本卡不再知道「我接在谁下面」「我在哪个区」，落位/
//! 间距/滚动/裁剪归排布器）。段内全宽（窄列不分列）。卡高 =
//! PAD_V·2 + 连接段 + 段距 + 通道段（2026-09-24 起恒定——会话行表
//! 退役，卡高不再吃会话数）。

use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 字段行高 = mini 卡头行高（2 格，合并前两卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 每列字段行数（左：目标/本地口/重拉/错误；右：数据9021/反连9022/
/// QUIC62633/QUIC62694——2026-09-24 通道段改造，恒定四行不变）
pub const N_FIELDS: usize = 4;
/// 调试钮间横距（通道段尾两钮并排：半宽 + 一距）
pub const BTN_GAP: u32 = pp::ROW_GAP;

/// 字段行块高（N 行 + (N-1) 行距）
const FIELDS_BLOCK: u32 = N_FIELDS as u32 * FIELD_H + (N_FIELDS as u32 - 1) * FIELD_GAP;

/// 连接段内容高（恒定）：mini 卡头 + 行距 + 字段块 + 行距 + [重连] 钮
pub const LEFT_H: u32 = pp::ROW_H + pp::ROW_GAP + FIELDS_BLOCK + pp::ROW_GAP + pp::BTN_H;

/// 通道段内容高（恒定，2026-09-24 通道段改造：会话行表退役——四口
/// 状态恒定四行 + 调试钮行）：mini 卡头 + 行距 + 字段块 + 行距 +
/// 钮行（[跳闸/投 QUIC] + [重启] 半宽并排）
pub const RIGHT_H: u32 = pp::ROW_H + pp::ROW_GAP + FIELDS_BLOCK + pp::ROW_GAP + pp::BTN_H;

/// 卡高账（A 档纯函数，恒定）：PAD_V·2 + 连接段 + 段距 + 通道段
pub fn card_h() -> u32 {
    pp::CARD_PAD_V * 2 + LEFT_H + pp::ROW_GAP + RIGHT_H
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
    /// 通道段 mini 卡头「通道 · 状态词」
    pub rheader: PoolRect,
    /// 通道段四字段行（四口状态）
    pub rfields: [PoolRect; N_FIELDS],
    /// [跳闸/投 QUIC] 调试钮（通道段尾左半；钮面/可点裁决归
    /// svc_card::toggle_verdict——本册只给几何/命中）
    pub qbutton: PoolRect,
    /// [重启] 钮（通道段尾右半，2026-09-23：完全自重启，两段确认归
    /// self_restart::tap——本册只给几何/命中，确认态不进 layout）
    pub rbutton: PoolRect,
    /// 卡内芯纵裁剪带（v3 卡内滚：涂装断墨/命中闸门同一份）——
    /// 内容滚动后画出带外的件只显不点（卡框本身不吃这条带：
    /// 框 = 区窗内静物，带 = 卡内沿 − PAD_V 的内容区纵段）
    pub content_clip: (i64, i64),
}

/// 布局纯函数：卡外框由三区排布器配给（parser_chain::slot_rect——
/// v3 钉顶钳高：card.h = min(自报高, 区窗高)，本卡不二次揣度屏寸/
/// 区窗）。内容随 scroll 卡内平移（壳手势喂入的右账位移；钳制语义
/// 唯一在本函数——上限 = 自报高 − 框高，与 parser_chain::scroll_max
/// 同一份账的两种吃法：区滑时代它平移槽位，卡内滚时代它平移内容）。
/// 2026-09-24 通道段改造：自报高恒定（n_sessions 参数退役）
pub fn layout_in(card: PoolRect, scroll: i64) -> LinkLayout {
    let full_h = card_h();
    let scroll_max = i64::from(full_h.saturating_sub(card.h));
    let eff_scroll = scroll.clamp(0, scroll_max.max(0));
    let cx = card.x + i64::from(pp::CARD_PAD_H);
    let cw = card.w.saturating_sub(pp::CARD_PAD_H * 2);
    let y0 = card.y + i64::from(pp::CARD_PAD_V) - eff_scroll;
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
    // 通道段（段距一行）：四字段行 + 调试钮行（半宽并排）
    let ry = by + i64::from(pp::BTN_H + pp::ROW_GAP);
    let rheader = header(ry);
    let rfy = ry + i64::from(pp::ROW_H + pp::ROW_GAP);
    let rfields = fields(rfy);
    let by2 = rfy + i64::from(FIELDS_BLOCK + pp::ROW_GAP);
    let half_w = cw.saturating_sub(BTN_GAP) / 2;
    let qbutton = PoolRect {
        x: cx,
        y: by2,
        w: half_w,
        h: pp::BTN_H,
    };
    let rbutton = PoolRect {
        x: cx + i64::from(half_w + BTN_GAP),
        y: by2,
        w: cw - half_w - BTN_GAP,
        h: pp::BTN_H,
    };
    let content_clip = (
        card.y + i64::from(pp::CARD_PAD_V),
        (card.y + i64::from(card.h) - i64::from(pp::CARD_PAD_V))
            .max(card.y + i64::from(pp::CARD_PAD_V)),
    );
    LinkLayout {
        card,
        lheader,
        lfields,
        button,
        rheader,
        rfields,
        qbutton,
        rbutton,
        content_clip,
    }
}

/// 命中结果（[重连]/[跳闸投 QUIC]/[重启] 可点；字段行/卡头 = 纯展示）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkHit {
    Reconnect,
    /// [跳闸/投 QUIC] 调试钮——跳闸还是投票归 svc_card::toggle_verdict
    QuicToggle,
    /// [重启] = 完全自重启（两段确认归 self_restart::tap）
    Restart,
}

/// 命中（x/y 屏坐标 i64，与涂装同一份 LinkLayout）。卡内滚后画出
/// 内芯带的件只显不点——命中闸门与涂装断墨同一份 content_clip
pub fn hit(l: &LinkLayout, x: i64, y: i64) -> Option<LinkHit> {
    if y < l.content_clip.0 || y >= l.content_clip.1 {
        return None;
    }
    let in_rect =
        |b: &PoolRect| x >= b.x && x < b.x + i64::from(b.w) && y >= b.y && y < b.y + i64::from(b.h);
    if in_rect(&l.button) {
        return Some(LinkHit::Reconnect);
    }
    if in_rect(&l.qbutton) {
        return Some(LinkHit::QuicToggle);
    }
    if in_rect(&l.rbutton) {
        return Some(LinkHit::Restart);
    }
    None
}
