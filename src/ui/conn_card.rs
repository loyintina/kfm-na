//! conn_card.rs — 连接卡文案面（2026-09-20 用户立项；同日晚合并裁决：
//! 连接卡 + 服务卡合并成 link_card 一张二级卡两竖列，本册**退役为
//! 文案面**——几何/命中归 link_card，涂装归 termview 合并段；本册
//! 只剩 隧道快照 → 卡文案 的映射与字段标签唯一源）
//!
//! 数据面拍板 = tunnel.rs 全局快照直读，免穿 App plumbing。文案 v1
//! （用户拍板，合并后原位沿用）：mini 卡头「连接 · 状态词」→ 四字段
//! 行（目标/本地口/重拉/错误）→ [重连] 钮（杀娃重拉，不等退避）。

use crate::tunnel::{self, TunnelSnap, TunnelState};
use crate::ui::parser_page as pp;

/// 字段行高 = mini 卡头行高（2 格）
pub const FIELD_H: u32 = pp::ROW_H;
/// 字段行距
pub const FIELD_GAP: u32 = pp::ROW_GAP;
/// 字段行数（目标/本地口/重拉/错误——恒定）
pub const N_FIELDS: usize = 4;

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
