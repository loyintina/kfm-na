//! svc_card.rs — 通道卡文案面（2026-09-24 用户裁决：原「服务」段
//! ——后端/在线/会话/错误 + 会话行——「实际看了一下没什么用」，退役；
//! 原位换更实际的：**四口连接状况**（数据 9021 / 反连 9022 /
//! QUIC 62633 / QUIC 62694）+ **调试钮**（[跳闸 QUIC]/[投 QUIC]
//! 切换 + [重启]）。本册 = 隧道快照 → 卡文案 的映射与字段标签唯一源；
//! 几何归 link_card，涂装归 termview，跳闸/投票执行归 tunnel
//! （TunnelCmd::TripQuic/HealQuic））。

use crate::svc_health::{self, Phase};
use crate::tunnel::{self, Leg, QUIC_FAIL_TRIP, TunnelSnap, TunnelState};
use crate::ui::conn_card as cc;

/// 字段行高 = mini 卡头行高（2 格，连接卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 字段行数（四口——恒定四行，连接卡同尺）
pub const N_FIELDS: usize = 4;

/// 字段标签（涂装唯一源——两处各写一份必漂移）：数据 9021 = 本地
/// 数据口；反连 9022 = 推送+调试路；QUIC 62633 = UDP 数据腿；
/// QUIC 62694 = UDP 反连腿（M4 已启用）
pub const FIELD_LABELS: [&str; N_FIELDS] = ["数据 9021", "反连 9022", "QUIC 62633", "QUIC 62694"];

/// 卡文案（涂装快照）：隧道快照 → 卡行的唯一产物
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvcSnap {
    /// 状态词（卡头「通道 · {word}」）
    pub word: String,
    /// 四字段值（与 FIELD_LABELS 同序）
    pub vals: [String; N_FIELDS],
}

/// 卡头状态词（A 档纯函数）：数据腿的当前形态一句话
pub fn head_word(st: &TunnelState) -> String {
    match st {
        TunnelState::QuicUp => "QUIC 在线".into(),
        TunnelState::Up => "ssh 在线".into(),
        TunnelState::ExternalUp => "外部借用".into(),
        TunnelState::Starting => "连接中".into(),
        TunnelState::Down { .. } => "断".into(),
    }
}

/// 「数据 9021」行（A 档纯函数）：本地数据口谁在供
pub fn data_row(st: &TunnelState) -> String {
    match st {
        TunnelState::QuicUp => "QUIC 桥在线".into(),
        TunnelState::Up => "ssh 正连在线".into(),
        TunnelState::ExternalUp => "外部借用".into(),
        TunnelState::Starting => "起手中".into(),
        TunnelState::Down { .. } => "断".into(),
    }
}

/// 「反连 9022」行（A 档纯函数）：QUIC 反连腿在 = 9022 归 na-server
/// QUIC 桥（M4）；否则进程在 = 在线；数据路断了它必断（同一条
/// ssh/同一场重拉）；其余 = 重拉中（封锁闸/死亡审理窗口）
pub fn reverse_row(s: &TunnelSnap) -> String {
    if s.rev_quic_up {
        "QUIC 反连在线".into()
    } else if s.reverse_up {
        "在线".into()
    } else if matches!(s.state, TunnelState::Down { .. }) {
        "断".into()
    } else {
        "重拉中".into()
    }
}

/// 「QUIC 62633」行（A 档纯函数）：未配置 > 跳闸降级 > 在线 > 握手
/// > 挂账 > 待起——跳闸账满 = 已降级 ssh 兜底（自动或手动同相）
pub fn quic_row(s: &TunnelSnap) -> String {
    if !s.quic_configured {
        "未配置".into()
    } else if s.quic_fails >= QUIC_FAIL_TRIP {
        "跳闸降级 ssh".into()
    } else if s.leg == Some(Leg::Quic) && matches!(s.state, TunnelState::QuicUp) {
        "在线".into()
    } else if s.leg == Some(Leg::Quic) {
        "握手中".into()
    } else if s.quic_fails > 0 {
        format!("挂×{}", s.quic_fails)
    } else {
        "待起".into()
    }
}

/// 「QUIC 62694」行（A 档纯函数，M4）：未配置 > 在线 > 挂账 > 待起。
/// 反连腿无跳闸（ssh 兜底永远欢迎，挂账只报数不降级）；「在线」含
/// 握手窗——客户端只能死信驱动，握手 8s 内腿对象在但尚未注册，
/// 无法与真在线区分（设计 docs/active/quic隧道.md §九 M4 段）
pub fn rev_quic_row(s: &TunnelSnap) -> String {
    if !s.quic_configured {
        "未配置".into()
    } else if s.rev_quic_up {
        "在线".into()
    } else if s.rev_quic_fails > 0 {
        format!("挂×{}", s.rev_quic_fails)
    } else {
        "待起".into()
    }
}

/// 隧道快照 → 卡文案（None = 看门狗没起：L3 未装/无服务器条目同相）
pub fn compose(t: Option<&TunnelSnap>) -> SvcSnap {
    match t {
        None => SvcSnap {
            word: "未启动".into(),
            vals: ["—".into(), "—".into(), "—".into(), "—".into()],
        },
        Some(s) => SvcSnap {
            word: head_word(&s.state),
            vals: [
                data_row(&s.state),
                reverse_row(s),
                quic_row(s),
                rev_quic_row(s),
            ],
        },
    }
}

/// QUIC 调试钮语义（A 档纯函数）：钮面/点按的唯一裁决。跳闸账未满
/// =「跳闸 QUIC」（一键降级 ssh——UDP 黑洞/腿疑难时不等三次自动
/// 跳闸，手动确认「是不是 QUIC 的锅」）；已满 =「投 QUIC」（再投
/// 一票恢复）；未配置 = None（钮面显示「QUIC 未配置」，点按不动作）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicToggle {
    Trip,
    Heal,
}

/// 调试钮裁决（None = 未配置，钮不可点）
pub fn toggle_verdict(t: Option<&TunnelSnap>) -> Option<QuicToggle> {
    let s = t?;
    if !s.quic_configured {
        return None;
    }
    if s.quic_fails >= QUIC_FAIL_TRIP {
        Some(QuicToggle::Heal)
    } else {
        Some(QuicToggle::Trip)
    }
}

/// 调试钮面文案（涂装唯一源）：与 toggle_verdict 同一份账
pub fn toggle_label(t: Option<&TunnelSnap>) -> String {
    match toggle_verdict(t) {
        Some(QuicToggle::Trip) => "跳闸 QUIC".into(),
        Some(QuicToggle::Heal) => "投 QUIC".into(),
        None => "QUIC 未配置".into(),
    }
}

/// 隧道快照借读（看门狗没起 = None）
fn tunnel_snap() -> Option<TunnelSnap> {
    tunnel::snap().map(|s| s.lock().unwrap().clone())
}

/// 读当前卡文案（涂装每烘焙拍一张；全局快照锁短）。Kfmv4 托管相 =
/// 全占位（后端非 na-server 时四口无意义）；相判定吃 svc_health
/// 配置源（壳设置加载时喂入——不许另开一路读 settings 两源漂移）
pub fn current() -> SvcSnap {
    if svc_health::snap().phase == Phase::Kfmv4 {
        return SvcSnap {
            word: "kfmv4 托管".into(),
            vals: ["—".into(), "—".into(), "—".into(), "—".into()],
        };
    }
    compose(tunnel_snap().as_ref())
}
