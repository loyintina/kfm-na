//! sys_card.rs — 环境卡内容核（2026-09-20 用户立项：解析页第四张二级
//! 卡，纵排在连接服务合并卡下；「中央终端所在环境的自身体征」可视化——与设备
//! 无关的通用面：服务器/手机/任何设备同一张卡同一组字段）。v2 六字段
//! 两竖列（同日用户拍板「服务器的信息能不能更详细一些」）：卡头
//! 「环境 · 对象词」→ 三行六字段（行主序：负载/进程 | 内存/交换 |
//! 磁盘/在线），卡高不变。
//!
//! 本册 = 文案映射 + 几何（A 档纯逻辑）；涂装在 termview（眼手同尺：
//! 两边吃本册同一份 layout）；数据 = svc_health 的 SysSnap（轮询器与
//! health 同拍）；体征解析/格式与 na-server 同一份 na-sys crate
//! （双端同构第二面）。本地相（第二刀）= collect("/data") 直读，
//! 卡面零改动。
//!
//! 布局（网格制，与合并卡同池区）：卡外框由卡链排布器配给
//! （parser_chain::slot_rect——两轴契约 §四：间距/落位归排布器，
//! 本卡不再知道「我接在谁下面」），卡高 = 恒定（三行固定，无列表
//! 无按钮）。六字段两竖列：列宽 = (内容宽 − COL_GAP)/2，奇偶分列。
//! tmux 卡侧预留带 = 排布器 reserved_below_tmux（同源钉死）。

use crate::na_server_sup::{self, SupSnap};
use crate::settings::Backend;
use crate::svc_health::{self, Phase};
use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 两卡间距归排布器（parser_chain CHAIN 注册槽——排布元数据不再是
/// 卡的私有财产）。字段行高 = 卡头行高（2 格，服务卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 字段行数（行主序两竖列：负载/进程 | 内存/交换 | 磁盘/在线——
/// 恒定三行六件，卡高才恒定）
pub const N_FIELDS: usize = 6;
/// 卡高 = PAD_V·2 + 卡头 + 行距 + 三字段行 + 两行距（无列表无按钮）
pub const CARD_H: u32 = pp::CARD_PAD_V * 2
    + pp::ROW_H
    + pp::ROW_GAP
    + (N_FIELDS as u32 / 2) * FIELD_H
    + (N_FIELDS as u32 / 2 - 1) * FIELD_GAP;

/// 字段标签（涂装唯一源——两处各写一份必漂移；行主序两竖列）
pub const FIELD_LABELS: [&str; N_FIELDS] = ["负载", "进程", "内存", "交换", "磁盘", "在线"];

/// 卡文案（涂装快照）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysCardSnap {
    /// 对象词（卡头「环境 · {word}」）：目标主机 / kfmv4 托管 / 确认中
    pub word: String,
    /// 负载 "0.42 0.38 0.35"（无数据 —）
    pub load: String,
    /// 进程 "2/123"（running/total；无数据 —）
    pub procs: String,
    /// 内存 "7.8G/15.6G 50%"（无数据 —）
    pub mem: String,
    /// 交换 "2.9G/3.9G 75%"（无数据 —）
    pub swap: String,
    /// 磁盘 "45G/100G 45%"（无数据 —）
    pub disk: String,
    /// 在线 "90天20时"（无数据 —）
    pub uptime: String,
}

/// 三源合成（A 档纯函数）：对象相 × 后端 × nasup 快照（对象词）×
/// sys 快照 → 卡文案。本地相（第 6 步③）：对象词 = 注册表静态词
/// 「本地」，字段吃本地直读的 sys 快照，后端相不掺和（本机体征
/// 不需要任何服务器）；服务器相照旧：kfmv4 = 托管态全占位；
/// na-server 无数据 = 字段占位不编造；单路 None = 该字段占位
/// （Android 拒 loadavg 合法常态，显形不连坐）
pub fn compose(
    kind: crate::endpoint::EndpointKind,
    backend: Backend,
    sup: Option<&SupSnap>,
    sys: Option<&na_sys::SysInfo>,
) -> SysCardSnap {
    if kind == crate::endpoint::EndpointKind::Local {
        let (load, procs, mem, swap, disk, uptime) = fields_of(sys);
        return SysCardSnap {
            word: crate::endpoint::def(kind).display.into(),
            load,
            procs,
            mem,
            swap,
            disk,
            uptime,
        };
    }
    if backend != Backend::NaServer {
        return SysCardSnap {
            word: "kfmv4 托管".into(),
            load: "—".into(),
            procs: "—".into(),
            mem: "—".into(),
            swap: "—".into(),
            disk: "—".into(),
            uptime: "—".into(),
        };
    }
    let word = match sup {
        Some(s) => s.target.clone(),
        None => "确认中".into(),
    };
    let (load, procs, mem, swap, disk, uptime) = fields_of(sys);
    SysCardSnap {
        word,
        load,
        procs,
        mem,
        swap,
        disk,
        uptime,
    }
}

/// sys 快照 → 六字段文案（本地/服务器相共用的字段映射唯一源——
/// 相不同 = 数据源不同，字段形状与格式同一份）
fn fields_of(sys: Option<&na_sys::SysInfo>) -> (String, String, String, String, String, String) {
    match sys {
        Some(i) => (
            i.load
                .map(|l| na_sys::fmt_load(&l))
                .unwrap_or_else(|| "—".into()),
            i.load
                .and_then(|l| l.procs)
                .map(|(r, t)| format!("{r}/{t}"))
                .unwrap_or_else(|| "—".into()),
            i.mem
                .map(|m| {
                    na_sys::fmt_usage(
                        m.total_kb.saturating_sub(m.avail_kb) * 1024,
                        m.total_kb * 1024,
                    )
                })
                .unwrap_or_else(|| "—".into()),
            i.mem
                .and_then(|m| m.swap)
                .map(|(t, f)| na_sys::fmt_usage(t.saturating_sub(f) * 1024, t * 1024))
                .unwrap_or_else(|| "—".into()),
            i.disk
                .map(|(t, a)| na_sys::fmt_usage(t.saturating_sub(a), t))
                .unwrap_or_else(|| "—".into()),
            i.uptime_s
                .map(na_sys::fmt_uptime)
                .unwrap_or_else(|| "—".into()),
        ),
        None => (
            "—".into(),
            "—".into(),
            "—".into(),
            "—".into(),
            "—".into(),
            "—".into(),
        ),
    }
}

/// 读当前卡文案（涂装每烘焙拍一张；全局快照锁短）。后端相取
/// svc_health 配置（与服务卡同源，不另开一路）；对象相取 endpoint
/// 注册表（两轴契约 §二，对象轴唯一源）
pub fn current() -> SysCardSnap {
    let hs = svc_health::snap();
    let backend = if hs.phase == Phase::Kfmv4 {
        Backend::Kfmv4
    } else {
        Backend::NaServer
    };
    let sup = na_server_sup::snap().map(|s| s.lock().unwrap().clone());
    let sys = svc_health::sys_snap();
    compose(
        crate::endpoint::current(),
        backend,
        sup.as_ref(),
        sys.sys.as_ref(),
    )
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct SysLayout {
    pub card: PoolRect,
    pub header: PoolRect,
    pub fields: [PoolRect; N_FIELDS],
}

/// 布局纯函数：卡外框由卡链排布器配给（parser_chain::slot_rect——
/// 几何只从排布器拿，本卡不二次揣度）
pub fn layout_in(card: PoolRect) -> SysLayout {
    let cx = card.x + i64::from(pp::CARD_PAD_H);
    let cw = card.w.saturating_sub(pp::CARD_PAD_H * 2);
    let col_w = cw.saturating_sub(pp::COL_GAP) / 2;
    let mut y = card.y + i64::from(pp::CARD_PAD_V);
    let header = PoolRect {
        x: cx,
        y,
        w: cw,
        h: pp::ROW_H,
    };
    y += i64::from(pp::ROW_H + pp::ROW_GAP);
    // 行主序两竖列：偶 = 左列（负载/内存/磁盘），奇 = 右列（进程/交换/在线）
    let fields: [PoolRect; N_FIELDS] = std::array::from_fn(|i| {
        let (row, col) = (i / 2, i % 2);
        PoolRect {
            x: cx
                + if col == 0 {
                    0
                } else {
                    i64::from(col_w + pp::COL_GAP)
                },
            y: y + (FIELD_H + FIELD_GAP) as i64 * row as i64,
            w: col_w,
            h: FIELD_H,
        }
    });
    SysLayout {
        card,
        header,
        fields,
    }
}
