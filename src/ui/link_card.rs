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
//! 连接段 = mini 卡头「连接 · 状态词」+ 两字段行（目标/本地口——
//! 2026-09-30 BAR-212 用户拍板：「重拉」「错误」两行提示没用，退役）
//! 加 [重连] 钮（杀娃重拉，不等退避）；**通道段**（2026-09-24
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
//! PAD_V·2 + 连接段 + 段距 + 通道段（2026-09-24 起不吃会话数——
//! 会话行表退役；2026-10-01 起吃折行账——BAR-206 打回重做：字段值
//! 格折行往下长，行高随内容长，全单行 = 旧恒定几何）。

use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 字段行高 = mini 卡头行高（2 格，合并前两卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 连接段字段行数（目标/本地口——2026-09-30 BAR-212：重拉/错误两行退役）
pub const L_FIELDS: usize = 2;
/// 通道段字段行数（数据9021/反连9022/QUIC62633/QUIC62694——2026-09-24
/// 通道段改造，恒定四行不变）
pub const R_FIELDS: usize = 4;
/// 调试钮间横距（通道段尾两钮并排：半宽 + 一距）
pub const BTN_GAP: u32 = pp::ROW_GAP;

/// 字段行块高（N 行 + (N-1) 行距）
const fn fields_block(n: u32) -> u32 {
    n * FIELD_H + (n - 1) * FIELD_GAP
}

/// 连接段字段块高（两行）
const LFIELDS_BLOCK: u32 = fields_block(L_FIELDS as u32);
/// 通道段字段块高（四行）
const RFIELDS_BLOCK: u32 = fields_block(R_FIELDS as u32);

/// 连接段内容高（恒定）：mini 卡头 + 行距 + 字段块 + 行距 + [重连] 钮
pub const LEFT_H: u32 = pp::ROW_H + pp::ROW_GAP + LFIELDS_BLOCK + pp::ROW_GAP + pp::BTN_H;

/// 通道段内容高（恒定，2026-09-24 通道段改造：会话行表退役——四口
/// 状态恒定四行 + 调试钮行）：mini 卡头 + 行距 + 字段块 + 行距 +
/// 钮行（[跳闸/投 QUIC] + [重启] 半宽并排）
pub const RIGHT_H: u32 = pp::ROW_H + pp::ROW_GAP + RFIELDS_BLOCK + pp::ROW_GAP + pp::BTN_H;

/// 字段值折行数账（BAR-206 打回重做，2026-10-01 用户裁定：省略 = 信息
/// 丢失——长值格折行往下长，行高随内容长，永不删字）：每字段 ≥1 行，
/// 全 1 = 旧恒定几何（LEFT_H/RIGHT_H 参考值不变）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkLines {
    /// 连接段两字段（目标/本地口）各自折行数
    pub l: [u32; L_FIELDS],
    /// 通道段四字段各自折行数
    pub r: [u32; R_FIELDS],
}

impl LinkLines {
    /// 全单行（旧恒定几何；缓存未写入时的缺省）
    pub const SINGLE: Self = Self {
        l: [1; L_FIELDS],
        r: [1; R_FIELDS],
    };
}

/// 字段行高（折行往下长）：n 行 = FIELD_H + (n−1) × 行推进（adv =
/// 涂装侧 meta 档行高 ceil(cell_h × scale × 4/3)，与
/// draw_field_lines_grid 同一份账——行高账不长 = 折行第二行纵溢
/// 压盖下行（BAR-206 原症回潮））
fn field_row_h(lines: u32, adv: u32) -> u32 {
    FIELD_H + lines.saturating_sub(1) * adv
}

/// 折行账缓存（涂装烘焙拍写入一次，涂装/命中/合成同读一份——眼手
/// 同尺；None = 还没烘过，缺省 SINGLE + adv 0 = 旧恒定几何）
static LAY_LINES: std::sync::Mutex<Option<(LinkLines, u32)>> = std::sync::Mutex::new(None);

/// 写入当前折行账（涂装侧在 parser_chain::heights 之前调用）
pub fn set_lay_lines(lines: LinkLines, adv: u32) {
    *LAY_LINES.lock().unwrap() = Some((lines, adv));
}

/// 读当前折行账（layout_in/card_h_now 调用方同取这一份）
pub fn lay_lines_now() -> (LinkLines, u32) {
    LAY_LINES.lock().unwrap().unwrap_or((LinkLines::SINGLE, 0))
}

/// 卡高账（A 档纯函数）：PAD_V·2 + 连接段 + 段距 + 通道段；字段行高
/// 随折行数长（全 1 行 = LEFT_H/RIGHT_H 恒定参考值）
pub fn card_h(lines: &LinkLines, adv: u32) -> u32 {
    let lblock: u32 = lines.l.iter().map(|&n| field_row_h(n, adv)).sum::<u32>()
        + FIELD_GAP * (L_FIELDS as u32 - 1);
    let rblock: u32 = lines.r.iter().map(|&n| field_row_h(n, adv)).sum::<u32>()
        + FIELD_GAP * (R_FIELDS as u32 - 1);
    let left = pp::ROW_H + pp::ROW_GAP + lblock + pp::ROW_GAP + pp::BTN_H;
    let right = pp::ROW_H + pp::ROW_GAP + rblock + pp::ROW_GAP + pp::BTN_H;
    pp::CARD_PAD_V * 2 + left + pp::ROW_GAP + right
}

/// 当前卡高（排布器 heights 用：读折行账缓存，涂装/命中同一份）
pub fn card_h_now() -> u32 {
    let (lines, adv) = lay_lines_now();
    card_h(&lines, adv)
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct LinkLayout {
    pub card: PoolRect,
    /// 连接段 mini 卡头「连接 · 状态词」
    pub lheader: PoolRect,
    /// 连接段两字段行（目标/本地口）
    pub lfields: [PoolRect; L_FIELDS],
    /// [重连] 钮（连接段尾）
    pub button: PoolRect,
    /// 通道段 mini 卡头「通道 · 状态词」
    pub rheader: PoolRect,
    /// 通道段四字段行（四口状态）
    pub rfields: [PoolRect; R_FIELDS],
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
/// 2026-09-24 通道段改造：自报高恒定（n_sessions 参数退役）；
/// 2026-10-01 BAR-206 打回重做：自报高改吃折行账（字段值格折行
/// 往下长，全 1 行 = 旧恒定几何）
pub fn layout_in(card: PoolRect, scroll: i64, lines: &LinkLines, adv: u32) -> LinkLayout {
    let full_h = card_h(lines, adv);
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
    // 连接段
    let lheader = header(y0);
    let mut fy = y0 + i64::from(pp::ROW_H + pp::ROW_GAP);
    let lfields: [PoolRect; L_FIELDS] = std::array::from_fn(|i| {
        let h = field_row_h(lines.l[i], adv);
        let r = PoolRect {
            x: cx,
            y: fy,
            w: cw,
            h,
        };
        fy += i64::from(h + FIELD_GAP);
        r
    });
    let by = fy - i64::from(FIELD_GAP) + i64::from(pp::ROW_GAP);
    let button = PoolRect {
        x: cx,
        y: by,
        w: cw,
        h: pp::BTN_H,
    };
    // 通道段（段距一行）：四字段行 + 调试钮行（半宽并排）
    let ry = by + i64::from(pp::BTN_H + pp::ROW_GAP);
    let rheader = header(ry);
    let mut rfy = ry + i64::from(pp::ROW_H + pp::ROW_GAP);
    let rfields: [PoolRect; R_FIELDS] = std::array::from_fn(|i| {
        let h = field_row_h(lines.r[i], adv);
        let r = PoolRect {
            x: cx,
            y: rfy,
            w: cw,
            h,
        };
        rfy += i64::from(h + FIELD_GAP);
        r
    });
    let by2 = rfy - i64::from(FIELD_GAP) + i64::from(pp::ROW_GAP);
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
