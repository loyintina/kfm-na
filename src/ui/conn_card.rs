//! conn_card.rs — 连接/服务卡内容核（2026-09-20 用户立项：解析页第
//! 二张二级卡，纵排在 tmux 卡下；数据面拍板 = tunnel.rs 全局快照直读，
//! 免穿 App plumbing）
//!
//! 本册 = 几何/命中/文案映射（A 档纯逻辑）；涂装在 termview（眼手同尺：
//! 两边吃本册同一份 layout）；数据走 crate::tunnel（supervisor 写、
//! 本册读）。卡内容 v1（用户拍板）：卡头「连接 · 状态词」→ 四字段行
//! （目标/本地口/重拉/错误）→ 分隔线 → [重连] 钮（杀娃重拉，不等退避）。
//!
//! 布局（网格制，与 tmux 卡同池区）：卡 = tmux 卡同宽、接其正下方
//! （间距 CONN_GAP），卡高 = 内容定（恒定——四字段行固定，不随文案
//! 伸缩）。tmux 卡侧按 INSET_EXTRA 预留底部带（= 间距 + 本卡实高，
//! 同源钉死：两处各写一份必漂移 = 两卡相叠/底部空洞鬼影）。

use crate::termview::CELL_H;
use crate::tunnel::{self, TunnelSnap, TunnelState};
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 两卡间距（1 格，与卡内行距同档起步——观感待用户微调）
pub const CONN_GAP: u32 = CELL_H;
/// 字段行高 = 卡头行高（2 格）
pub const FIELD_H: u32 = pp::ROW_H;
/// 字段行距
pub const FIELD_GAP: u32 = pp::ROW_GAP;
/// 字段行数（目标/本地口/重拉/错误——恒定，卡高才恒定）
pub const N_FIELDS: usize = 4;
/// 卡高 = PAD_V·2 + 卡头 + 行距 + N 字段行 + (N-1) 行距 + 分隔线带 + 钮
pub const CARD_H: u32 = pp::CARD_PAD_V * 2
    + pp::ROW_H
    + pp::ROW_GAP
    + N_FIELDS as u32 * FIELD_H
    + (N_FIELDS as u32 - 1) * FIELD_GAP
    + pp::DIVIDER_ZONE
    + pp::BTN_H;
/// tmux 卡为本卡预留的底部带（pp::layout 的 bottom_inset 加这层）——
/// 与本卡实高同源（spec_预留量_与实高同源 钉死）
pub const INSET_EXTRA: u32 = CONN_GAP + CARD_H;

/// 字段标签（涂装/命中唯一源——两处各写一份必漂移）
pub const FIELD_LABELS: [&str; N_FIELDS] = ["目标", "本地口", "重拉", "错误"];

/// 卡文案（涂装快照）：隧道快照 → 卡行的唯一映射（A 档）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnSnap {
    /// 状态词（tunnel::state_word 同一源）
    pub word: String,
    /// user@host:port
    pub target: String,
    /// 127.0.0.1:本地口
    pub local: String,
    /// 重拉次数（×N；非退避态不背旧账 → —）
    pub attempts: String,
    /// 最近错误（退避/缺件态有字；在线 → —）
    pub error: String,
}

/// 隧道快照 → 卡文案（None = 看门狗没起：L3 未装/无服务器条目同相）
pub fn from_tunnel(t: Option<&TunnelSnap>) -> ConnSnap {
    let (word, target, local, attempts, error) = match t {
        None => (
            tunnel::state_word(&TunnelState::Down {
                attempts: 0,
                last_error: String::new(),
            }),
            "—".into(),
            "—".into(),
            "—".into(),
            "—".into(),
        ),
        Some(s) => {
            let (attempts, error) = match &s.state {
                TunnelState::Down {
                    attempts,
                    last_error,
                } if *attempts > 0 => (format!("×{attempts}"), last_error.clone()),
                TunnelState::Down { last_error, .. } if !last_error.is_empty() => {
                    ("—".into(), last_error.clone())
                }
                _ => ("—".into(), "—".into()),
            };
            (
                tunnel::state_word(&s.state),
                s.target.clone(),
                format!("127.0.0.1:{}", s.local_port),
                attempts,
                error,
            )
        }
    };
    ConnSnap {
        word,
        target,
        local,
        attempts,
        error,
    }
}

/// 读当前卡文案（涂装每烘焙拍一张；全局快照锁短）
pub fn current() -> ConnSnap {
    let g = tunnel::snap();
    let g = g.as_ref().map(|s| s.lock().unwrap().clone());
    from_tunnel(g.as_ref())
}

/// 命中结果（v1 只有 [重连] 可点；字段行/卡头/分隔线 = 纯展示）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnHit {
    Reconnect,
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct ConnLayout {
    pub card: PoolRect,
    pub header: PoolRect,
    pub fields: [PoolRect; N_FIELDS],
    pub divider: PoolRect,
    pub button: PoolRect,
}

/// 布局纯函数：与 tmux 卡同宽、接其正下方（几何只从 tmux 卡推——
/// 屏寸/池区都已在 tmux 卡里约过，本卡不二次揣度）
pub fn layout(tmux_card: &PoolRect) -> ConnLayout {
    let card = PoolRect {
        x: tmux_card.x,
        y: tmux_card.y + i64::from(tmux_card.h) + i64::from(CONN_GAP),
        w: tmux_card.w,
        h: CARD_H,
    };
    let cx = card.x + i64::from(pp::CARD_PAD_H);
    let cw = card.w.saturating_sub(pp::CARD_PAD_H * 2);
    let mut y = card.y + i64::from(pp::CARD_PAD_V);
    let header = PoolRect {
        x: cx,
        y,
        w: cw,
        h: pp::ROW_H,
    };
    y += i64::from(pp::ROW_H + pp::ROW_GAP);
    let fields: [PoolRect; N_FIELDS] = std::array::from_fn(|i| PoolRect {
        x: cx,
        y: y + (FIELD_H + FIELD_GAP) as i64 * i as i64,
        w: cw,
        h: FIELD_H,
    });
    y += i64::from(FIELD_H * N_FIELDS as u32 + FIELD_GAP * (N_FIELDS as u32 - 1));
    let divider = PoolRect {
        x: cx,
        y: y + i64::from(pp::DIVIDER_ZONE - pp::DIVIDER_H) / 2,
        w: cw,
        h: pp::DIVIDER_H,
    };
    y += i64::from(pp::DIVIDER_ZONE);
    let button = PoolRect {
        x: cx,
        y,
        w: cw,
        h: pp::BTN_H,
    };
    ConnLayout {
        card,
        header,
        fields,
        divider,
        button,
    }
}

/// 命中（x/y 屏坐标 i64，与涂装同一份 ConnLayout）
pub fn hit(l: &ConnLayout, x: i64, y: i64) -> Option<ConnHit> {
    let b = &l.button;
    if x >= b.x && x < b.x + i64::from(b.w) && y >= b.y && y < b.y + i64::from(b.h) {
        return Some(ConnHit::Reconnect);
    }
    None
}
