//! svc_card.rs — 服务卡文案面（2026-09-20 用户立项；同日晚合并裁决：
//! 连接卡 + 服务卡合并成 link_card 一张二级卡两竖列，本册**退役为
//! 文案面**——几何归 link_card，涂装归 termview 合并段；本册只剩
//! 三源合成（后端 × nasup × health）→ 卡文案 的映射与字段标签唯一源）。
//!
//! na-server 会话层的可视化面（docs/active/na-server.md §四）。文案
//! v1（用户拍板，合并后原位沿用）：mini 卡头「服务 · 状态词」→ 四
//! 字段行（后端/在线/会话/错误）→ 每会话一行；无按钮。数据 =
//! svc_health 全局快照 + nasup 全局快照 + settings Backend 三源合成
//! （compose 唯一映射）。

use crate::na_server_sup::{self, SupSnap, SupState};
use crate::settings::Backend;
use crate::svc_health::{self, HealthSnap, Phase};
use crate::ui::conn_card as cc;

/// 字段行高 = mini 卡头行高（2 格，连接卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 字段行数（后端/在线/会话/错误——恒定四行，连接卡同尺）
pub const N_FIELDS: usize = 4;

/// 字段标签（涂装唯一源——两处各写一份必漂移）
pub const FIELD_LABELS: [&str; N_FIELDS] = ["后端", "在线", "会话", "错误"];

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
