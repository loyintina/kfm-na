//! svc_card.rs — 服务卡内容核（2026-09-20 用户立项：解析页第三张二级
//! 卡，纵排在连接卡下；na-server 会话层的可视化面，docs/active/
//! na-server.md §四）。v1 纯展示：卡头「服务 · 状态词」→ 四字段行
//! （后端/在线/会话/错误）→ 分隔线 → 每会话一行；无按钮。
//!
//! 本册 = 文案映射 + 几何（A 档纯逻辑）；涂装在 termview（眼手同尺：
//! 两边吃本册同一份 layout）；数据 = svc_health 全局快照 + nasup
//! 全局快照 + settings Backend 三源合成（compose 唯一映射）。
//!
//! 布局（网格制，与连接卡同池区）：卡 = 连接卡同宽、接其正下方
//! （间距 SVC_GAP），卡高 = 内容定（会话数定——动态高卡）。tmux 卡
//! 侧按 inset_extra_live() 预留底部带（= 间距 + 本卡实高，与涂装
//! 吃同一份快照同源钉死：两处各写一份必漂移 = 两卡相叠/底部空洞）。

use crate::na_server_sup::{self, SupSnap, SupState};
use crate::settings::Backend;
use crate::svc_health::{self, HealthSnap, Phase};
use crate::termview::CELL_H;
use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 两卡间距（与连接卡同档）
pub const SVC_GAP: u32 = CELL_H;
/// 字段行高 = 卡头行高（2 格，连接卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 字段行数（后端/在线/会话/错误——恒定四行，连接卡同尺）
pub const N_FIELDS: usize = 4;
/// 会话行高 = 字段行高
pub const SESS_H: u32 = FIELD_H;
/// 会话行距
pub const SESS_GAP: u32 = FIELD_GAP;

/// 字段标签（涂装唯一源——两处各写一份必漂移）
pub const FIELD_LABELS: [&str; N_FIELDS] = ["后端", "在线", "会话", "错误"];

/// 卡高账（A 档纯函数）：PAD_V·2 + 卡头 + 行距 + 四字段行 + 三行距 +
/// 分隔线带 + 会话行区（n>0 时 n 行 + (n-1) 行距）
pub fn card_h(n_sessions: usize) -> u32 {
    let fixed = pp::CARD_PAD_V * 2
        + pp::ROW_H
        + pp::ROW_GAP
        + N_FIELDS as u32 * FIELD_H
        + (N_FIELDS as u32 - 1) * FIELD_GAP
        + pp::DIVIDER_ZONE;
    if n_sessions == 0 {
        fixed
    } else {
        fixed + n_sessions as u32 * SESS_H + (n_sessions as u32 - 1) * SESS_GAP
    }
}

/// tmux 卡为本卡预留的底部带（n 会话时）：与实高同源钉死
pub fn inset_extra(n_sessions: usize) -> u32 {
    SVC_GAP + card_h(n_sessions)
}

/// 活预留量（涂装/命中/inset 三处的唯一入口）：吃当前快照的会话数——
/// 与卡面同源，快照换代 → 壳脏帧 → 几何同步换代（眼手同尺不断代）
pub fn inset_extra_live() -> u32 {
    inset_extra(current().lines.len())
}

/// 卡文案（涂装快照）：三源合成后的唯一产物
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvcSnap {
    /// 状态词（卡头「服务 · {word}」）
    pub word: String,
    /// 后端词（na-server / kfmv4）
    pub backend: String,
    /// 在线时长（fmt_duration；无数据 —）
    pub uptime: String,
    /// 会话数（无数据 —）
    pub sess_n: String,
    /// 错误（无 —）
    pub error: String,
    /// 每会话一行（svc_health::session_line 同件）
    pub lines: Vec<String>,
}

/// 三源合成（A 档纯函数）：后端 × nasup 快照 × health 快照 → 卡文案。
/// 相位机：kfmv4 托管态全占位；na-server 相 word 取 nasup 状态词
/// （自持在线/外部借用/确认中/待隧道/退避×N/未启动），字段行只在
/// health Ready 时有数据，Error 保留旧数据 + 错误上字段
pub fn compose(backend: Backend, sup: Option<&SupSnap>, hs: &HealthSnap) -> SvcSnap {
    if backend != Backend::NaServer {
        return SvcSnap {
            word: "kfmv4 托管".into(),
            backend: "kfmv4".into(),
            uptime: "—".into(),
            sess_n: "—".into(),
            error: "—".into(),
            lines: Vec::new(),
        };
    }
    let word = match sup {
        Some(s) => na_server_sup::state_word(&s.state),
        None => "未启动".into(),
    };
    let sup_err = match sup {
        Some(SupSnap {
            state: SupState::Down { last_error, .. },
            ..
        }) if !last_error.is_empty() => last_error.clone(),
        _ => "—".into(),
    };
    let (uptime, sess_n, lines) = match &hs.info {
        Some(info) => (
            svc_health::fmt_duration(info.uptime_s),
            info.sessions.len().to_string(),
            info.sessions.iter().map(svc_health::session_line).collect(),
        ),
        None => ("—".into(), "—".into(), Vec::new()),
    };
    let error = match &hs.phase {
        Phase::Error(e) => e.clone(),
        _ => sup_err,
    };
    SvcSnap {
        word,
        backend: "na-server".into(),
        uptime,
        sess_n,
        error,
        lines,
    }
}

/// 读当前卡文案（涂装每烘焙拍一张；全局快照锁短）。后端相取
/// svc_health 配置（壳设置加载时喂入——与轮询器同源，不许另开一路
/// 读 settings 两源漂移）：Kfmv4 相 = 托管态，其余 = na-server
pub fn current() -> SvcSnap {
    let hs = svc_health::snap();
    let backend = if hs.phase == Phase::Kfmv4 {
        Backend::Kfmv4
    } else {
        Backend::NaServer
    };
    let sup = svc_health_sup_snap();
    compose(backend, sup.as_ref(), &hs)
}

/// nasup 快照借读（看门狗没起 = None——后端非 na-server 同相）
fn svc_health_sup_snap() -> Option<SupSnap> {
    na_server_sup::snap().map(|s| s.lock().unwrap().clone())
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct SvcLayout {
    pub card: PoolRect,
    pub header: PoolRect,
    pub fields: [PoolRect; N_FIELDS],
    pub divider: PoolRect,
    pub sessions: Vec<PoolRect>,
}

/// 布局纯函数：与连接卡同宽、接其正下方（几何只从连接卡推——屏寸/
/// 池区都已在 tmux 卡里约过，本卡不二次揣度）
pub fn layout(conn_card: &PoolRect, n_sessions: usize) -> SvcLayout {
    let card = PoolRect {
        x: conn_card.x,
        y: conn_card.y + i64::from(conn_card.h) + i64::from(SVC_GAP),
        w: conn_card.w,
        h: card_h(n_sessions),
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
    let sessions = (0..n_sessions)
        .map(|i| PoolRect {
            x: cx,
            y: y + (SESS_H + SESS_GAP) as i64 * i as i64,
            w: cw,
            h: SESS_H,
        })
        .collect();
    SvcLayout {
        card,
        header,
        fields,
        divider,
        sessions,
    }
}
