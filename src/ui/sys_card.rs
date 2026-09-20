//! sys_card.rs — 环境卡内容核（2026-09-20 用户立项：解析页第四张二级
//! 卡，纵排在服务卡下；「中央终端所在环境的自身体征」可视化——与设备
//! 无关的通用面：服务器/手机/任何设备同一张卡同一组字段）。v1 纯展示：
//! 卡头「环境 · 对象词」→ 三字段行（负载/内存/磁盘）。
//!
//! 本册 = 文案映射 + 几何（A 档纯逻辑）；涂装在 termview（眼手同尺：
//! 两边吃本册同一份 layout）；数据 = svc_health 的 SysSnap（轮询器与
//! health 同拍）；体征解析/格式与 na-server 同一份 na-sys crate
//! （双端同构第二面）。本地相（第二刀）= collect("/data") 直读，
//! 卡面零改动。
//!
//! 布局（网格制，与服务卡同池区）：卡 = 服务卡同宽、接其正下方
//! （间距 SYS_GAP），卡高 = 恒定（三字段行固定，无列表无按钮）。
//! tmux 卡侧按 INSET_EXTRA 预留底部带（常量同源钉死——恒定高卡
//! 不需要 svc_card 那种活预留）。

use crate::na_server_sup::{self, SupSnap};
use crate::settings::Backend;
use crate::svc_health::{self, Phase};
use crate::termview::CELL_H;
use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 两卡间距（与服务卡同档）
pub const SYS_GAP: u32 = CELL_H;
/// 字段行高 = 卡头行高（2 格，服务卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 字段行数（负载/内存/磁盘——恒定三行，卡高才恒定）
pub const N_FIELDS: usize = 3;
/// 卡高 = PAD_V·2 + 卡头 + 行距 + 三字段行 + 两行距（无列表无按钮）
pub const CARD_H: u32 = pp::CARD_PAD_V * 2
    + pp::ROW_H
    + pp::ROW_GAP
    + N_FIELDS as u32 * FIELD_H
    + (N_FIELDS as u32 - 1) * FIELD_GAP;
/// tmux 卡为本卡预留的底部带：与实高同源钉死（常量——恒定高卡）
pub const INSET_EXTRA: u32 = SYS_GAP + CARD_H;

/// 字段标签（涂装唯一源——两处各写一份必漂移）
pub const FIELD_LABELS: [&str; N_FIELDS] = ["负载", "内存", "磁盘"];

/// 卡文案（涂装快照）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysCardSnap {
    /// 对象词（卡头「环境 · {word}」）：目标主机 / kfmv4 托管 / 确认中
    pub word: String,
    /// 负载 "0.42 0.38 0.35"（无数据 —）
    pub load: String,
    /// 内存 "7.8G/15.6G 50%"（无数据 —）
    pub mem: String,
    /// 磁盘 "45G/100G 45%"（无数据 —）
    pub disk: String,
}

/// 三源合成（A 档纯函数）：后端 × nasup 快照（对象词）× sys 快照 →
/// 卡文案。kfmv4 = 托管态全占位；na-server 无数据 = 字段占位不编造；
/// 单路 None = 该字段占位（Android 拒 loadavg 合法常态，显形不连坐）
pub fn compose(
    backend: Backend,
    sup: Option<&SupSnap>,
    sys: Option<&na_sys::SysInfo>,
) -> SysCardSnap {
    if backend != Backend::NaServer {
        return SysCardSnap {
            word: "kfmv4 托管".into(),
            load: "—".into(),
            mem: "—".into(),
            disk: "—".into(),
        };
    }
    let word = match sup {
        Some(s) => s.target.clone(),
        None => "确认中".into(),
    };
    let (load, mem, disk) = match sys {
        Some(i) => (
            i.load
                .map(|l| na_sys::fmt_load(&l))
                .unwrap_or_else(|| "—".into()),
            i.mem
                .map(|m| {
                    na_sys::fmt_usage(
                        m.total_kb.saturating_sub(m.avail_kb) * 1024,
                        m.total_kb * 1024,
                    )
                })
                .unwrap_or_else(|| "—".into()),
            i.disk
                .map(|(t, a)| na_sys::fmt_usage(t.saturating_sub(a), t))
                .unwrap_or_else(|| "—".into()),
        ),
        None => ("—".into(), "—".into(), "—".into()),
    };
    SysCardSnap {
        word,
        load,
        mem,
        disk,
    }
}

/// 读当前卡文案（涂装每烘焙拍一张；全局快照锁短）。后端相取
/// svc_health 配置（与服务卡同源，不另开一路）
pub fn current() -> SysCardSnap {
    let hs = svc_health::snap();
    let backend = if hs.phase == Phase::Kfmv4 {
        Backend::Kfmv4
    } else {
        Backend::NaServer
    };
    let sup = na_server_sup::snap().map(|s| s.lock().unwrap().clone());
    let sys = svc_health::sys_snap();
    compose(backend, sup.as_ref(), sys.sys.as_ref())
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct SysLayout {
    pub card: PoolRect,
    pub header: PoolRect,
    pub fields: [PoolRect; N_FIELDS],
}

/// 布局纯函数：与服务卡同宽、接其正下方（几何只从服务卡推——屏寸/
/// 池区都已在 tmux 卡里约过，本卡不二次揣度）
pub fn layout(svc_card: &PoolRect) -> SysLayout {
    let card = PoolRect {
        x: svc_card.x,
        y: svc_card.y + i64::from(svc_card.h) + i64::from(SYS_GAP),
        w: svc_card.w,
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
    SysLayout {
        card,
        header,
        fields,
    }
}
