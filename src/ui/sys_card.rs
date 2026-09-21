//! sys_card.rs — 环境卡内容核（2026-09-20 用户立项：解析页二级卡；
//! 2026-09-21 三区 v2 入住左滚动区；同日「环境卡重做」——复刻 kfmv4
//! 中央面板的动态负载统计表，**单竖列**）。
//!
//! 「中央终端所在环境的自身体征」可视化——与设备无关的通用面：服务器/
//! 手机/任何设备同一张卡同一组字段。v3 单竖列（用户拍板「也一样做成整个
//! 单竖列的，不要并排的」）：卡头「环境 · 对象词」→ 四轨（**负载/内存/
//! 交换/磁盘**：文字行【标签左锚 + 值右锚】+ 行下滚动柱状图，kfmv4 SYS
//! 面板同构）→ 两尾部文字行（进程/在线——无百分比可判（/proc/uptime 与
//! 进程数是计数不是占比），只出文字行不留空轨）。
//!
//! 本册 = 文案映射 + 几何（A 档纯逻辑）；涂装在 termview（眼手同尺：
//! 两边吃本册同一份 layout）；数据 = svc_health 的 SysSnap（轮询器与
//! health 同拍）+ SysHist（历史环形账，2s 一拍——柱轨的数据面）；
//! 体征解析/格式与 na-server 同一份 na-sys crate（双端同构第二面）。
//! 本地相（第二刀）= collect("/data") 直读，卡面零改动。
//!
//! 布局（网格制，与合并卡同池区）：卡外框由卡链排布器配给
//! （parser_chain::slot_rect——两轴契约 §四：间距/落位归排布器，
//! 本卡不再知道「我接在谁下面」），卡高 = 恒定（四轨 + 两尾部行固定）。

use crate::na_server_sup::{self, SupSnap};
use crate::settings::Backend;
use crate::svc_health::{self, Phase};
use crate::sys_hist;
use crate::ui::conn_card as cc;
use crate::ui::dual_pool::PoolRect;
use crate::ui::parser_page as pp;

/// 两卡间距归排布器（parser_chain CHAIN 注册槽——排布元数据不再是
/// 卡的私有财产）。字段行高 = 卡头行高（2 格，服务卡同件）
pub const FIELD_H: u32 = cc::FIELD_H;
/// 字段行距
pub const FIELD_GAP: u32 = cc::FIELD_GAP;
/// 柱轨（四轨：负载/内存/交换/磁盘）——轨序唯一源在 sys_hist::METRICS，
/// 本册标签数组同序
pub const N_METRICS: usize = sys_hist::METRICS.len();
/// 尾部文字行（进程/在线——无占比可判，只出行）
pub const N_TAILS: usize = 2;
/// 柱轨高 = 1 格（sys_hist::TRACK_H 单源）
pub const TRACK_H: u32 = sys_hist::TRACK_H;
/// 卡高 = PAD_V·2 + 卡头 + 行距 + 四轨（文字行 + 柱轨 + 轨后隙）
/// + 两尾部行 + 一行距（恒定，无列表无按钮）
pub const CARD_H: u32 = pp::CARD_PAD_V * 2
    + pp::ROW_H
    + pp::ROW_GAP
    + N_METRICS as u32 * (FIELD_H + TRACK_H + pp::ROW_GAP)
    + N_TAILS as u32 * FIELD_H
    + (N_TAILS as u32 - 1) * FIELD_GAP;

/// 柱轨标签（涂装唯一源——两处各写一份必漂移；轨序 = sys_hist::METRICS）
pub const METRIC_LABELS: [&str; N_METRICS] = ["负载", "内存", "交换", "磁盘"];
/// 尾部行标签
pub const TAIL_LABELS: [&str; N_TAILS] = ["进程", "在线"];

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

/// 四轨值文案（涂装/命中同一把尺——轨序与 METRIC_LABELS 咬合）
pub fn metric_values(s: &SysCardSnap) -> [&str; N_METRICS] {
    [&s.load, &s.mem, &s.swap, &s.disk]
}

/// 尾部行值文案（同序咬合 TAIL_LABELS）
pub fn tail_values(s: &SysCardSnap) -> [&str; N_TAILS] {
    [&s.procs, &s.uptime]
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
            // 进程行（2026-09-21 用户问「那个数字是什么意思」后自证化）：
            // /proc/loadavg 第 4 段 = 「可运行调度实体 / 系统调度实体总数」
            // （内核口径 = 进程+线程）。紧写 "2/123" 读不出是啥，故写明
            i.load
                .and_then(|l| l.procs)
                .map(|(r, t)| format!("{r} 运行 / {t} 总"))
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

/// 一轨布局（文字行 + 柱轨）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricLayout {
    pub row: PoolRect,
    pub track: PoolRect,
}

/// 一卡布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct SysLayout {
    pub card: PoolRect,
    pub header: PoolRect,
    pub metrics: [MetricLayout; N_METRICS],
    pub tails: [PoolRect; N_TAILS],
}

/// 柱几何（一轨）：轨内柱列参数——轨宽/柱距/柱数/柱高上限（涂装与
/// 合成期柱层同吃一份，眼手同尺）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarGeom {
    /// 轨左缘 x（页坐标）
    pub x: i64,
    /// 柱轨顶 y（页坐标）
    pub y: i64,
    /// 轨宽（可见窗口宽——uv 窗口的横尺）
    pub w: u32,
    /// 柱距（步长）
    pub step: u32,
    /// 可见柱数（进口柱另算：涂装取 tail(bars+1)）
    pub bars: usize,
    /// 柱高上限
    pub max_h: u32,
}

/// 柱层几何（合成期柱层与层内直涂同一份）：四轨柱几何 + 层画布尺。
/// 画布宽 = 轨宽 + 一柱距（合成期 uv 窗口滑到稳态位时右缘要取到
/// 轨宽 + 一柱距 处的底——kfmv4 恒渲染「柱数+1」根同规）；画布 = 四轨
/// **紧凑排布**（轨间文字行不归本层——层只覆盖轨矩形，文字行留页烘焙）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandGeom {
    pub tracks: [BarGeom; N_METRICS],
    pub canvas_w: u32,
    /// 单轨层内高（= 轨高）
    pub track_h: u32,
}

impl BandGeom {
    /// 层画布高 = 四轨紧凑叠放
    pub fn canvas_h(&self) -> u32 {
        self.track_h * N_METRICS as u32
    }

    /// 轨 i 的层内顶（紧凑排布：i × 轨高）
    pub fn local_y(&self, i: usize) -> i64 {
        i as i64 * i64::from(self.track_h)
    }

    /// 轨 i 在层内的 v 段（0..1 归一；合成期取源窗用）
    pub fn uv_v(&self, i: usize) -> (f32, f32) {
        let ch = self.canvas_h().max(1) as f32;
        (
            self.local_y(i) as f32 / ch,
            (self.local_y(i) + i64::from(self.track_h)) as f32 / ch,
        )
    }
}

/// 一轨柱几何：轨矩 → 柱列参数
pub fn bar_geom(track: &PoolRect) -> BarGeom {
    BarGeom {
        x: track.x,
        y: track.y,
        w: track.w,
        step: sys_hist::STEP,
        bars: sys_hist::bars_for(track.w, sys_hist::STEP),
        max_h: sys_hist::BAR_MAX_H.min(track.h),
    }
}

/// 柱层几何（涂装/合成期同一份——单一源）
pub fn band_of(lay: &SysLayout) -> BandGeom {
    let tracks: [BarGeom; N_METRICS] = std::array::from_fn(|i| bar_geom(&lay.metrics[i].track));
    BandGeom {
        tracks,
        canvas_w: lay.metrics[0].track.w + sys_hist::STEP,
        track_h: lay.metrics[0].track.h,
    }
}

/// 单轨合成放置（屏坐标 dest 矩形 + 源 uv 窗口，A 档纯函数——考题钉
/// 滑入位移/裁剪/uv 三者咬合；涂装侧靠合成期这条尺呈现滑动）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPlace {
    /// dest 矩形 (x, y, w, h)——页坐标（面板偏移由合成期加）
    pub rect: (f32, f32, f32, f32),
    /// 源 uv 窗口 = **(原点 u0, 原点 v0, 尺寸 uw, 尺寸 vh)**（0..1 归一，
    /// 与图层 shader 的 `a_uv = 原点 + c*尺寸` 同尺——**含尺寸不是远角**：
    /// 首版把远角当尺寸传，源窗纵段翻倍 = 柱带错位越轨，redroid 截屏
    /// 定罪后改成尺寸语义并钉在 sys_hist_spec）。u 原点随滑入位移滑，
    /// v 段取纵向裁剪后的行段
    pub uv: (f32, f32, f32, f32),
    /// 该轨是否可见（纵向裁剪后为空 = 不画）
    pub visible: bool,
}

/// 柱层合成放置（四轨；页坐标，不含面板偏移/层位移）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandPlace {
    pub tracks: [TrackPlace; N_METRICS],
}

/// 柱层合成放置（A 档纯函数）：**滑动全在合成期**——层内容烘一次
/// （稳态位：柱 j 在层内 x = j·柱距），滑入位移 = 源窗口 u 起点；
/// 纵向裁剪带（左区窗）逐轨求交，越界轨不画（框不动内容断墨——
/// 与页烘焙的裁剪带同语义）
pub fn band_place(band: &BandGeom, offset: u32, clip: (i64, i64)) -> BandPlace {
    let cw = band.canvas_w.max(1) as f32;
    let ch = band.canvas_h().max(1) as f32;
    let u0 = (offset as f32 / cw).clamp(0.0, 1.0);
    let tracks: [TrackPlace; N_METRICS] = std::array::from_fn(|i| {
        let t = &band.tracks[i];
        let y0 = t.y.max(clip.0);
        let y1 = (t.y + i64::from(band.track_h)).min(clip.1);
        if y1 <= y0 || t.w == 0 {
            return TrackPlace {
                rect: (0.0, 0.0, 0.0, 0.0),
                uv: (0.0, 0.0, 0.0, 0.0),
                visible: false,
            };
        }
        let (lv0, _) = band.uv_v(i);
        let row_h = band.track_h.max(1) as f32;
        let skip = (y0 - t.y) as f32 / row_h; // 裁剪掉的顶部比例
        let span = (y1 - y0) as f32 / row_h;
        let lspan = band.track_h.max(1) as f32 / ch;
        TrackPlace {
            rect: (t.x as f32, y0 as f32, t.w as f32, (y1 - y0) as f32),
            // 源窗尺寸恒 = (轨宽, 裁剪后轨高) 归一——**不随位移变**：
            // 位移只挪原点（尺寸随位移变 = 把远角当尺寸的旧病复辟）
            uv: (
                u0,
                lv0 + skip * lspan,
                (t.w as f32 / cw).clamp(0.0, 1.0),
                span * lspan,
            ),
            visible: true,
        }
    });
    BandPlace { tracks }
}

/// 布局纯函数：卡外框由卡链排布器配给（parser_chain::slot_rect——
/// 几何只从排布器拿，本卡不二次揣度）。单竖列：卡头 → 四轨（文字行 +
/// 柱轨）→ 两尾部行，各行距 = ROW_GAP
pub fn layout_in(card: PoolRect) -> SysLayout {
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
    let metrics: [MetricLayout; N_METRICS] = std::array::from_fn(|i| {
        let row = PoolRect {
            x: cx,
            y: y + i64::from((FIELD_H + TRACK_H + pp::ROW_GAP) * i as u32),
            w: cw,
            h: FIELD_H,
        };
        let track = PoolRect {
            x: cx,
            y: row.y + i64::from(FIELD_H),
            w: cw,
            h: TRACK_H,
        };
        MetricLayout { row, track }
    });
    y += i64::from((FIELD_H + TRACK_H + pp::ROW_GAP) * N_METRICS as u32);
    let tails: [PoolRect; N_TAILS] = std::array::from_fn(|j| PoolRect {
        x: cx,
        y: y + i64::from((FIELD_H + FIELD_GAP) * j as u32),
        w: cw,
        h: FIELD_H,
    });
    SysLayout {
        card,
        header,
        metrics,
        tails,
    }
}
