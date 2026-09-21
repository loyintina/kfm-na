//! sys_hist.rs — 环境体征历史账（环境卡滚动柱数据面，A 档纯逻辑）。
//!
//! 由来（2026-09-21 用户拍板「环境卡重做」）：kfmv4 中央面板的 SYS 监控
//! 面板 = 每指标【文字行（标签 + 数值 + 实值对）+ 行下**滚动柱状图**】
//! ——逐样本判色（绿 <70 / 琥珀 70-85 / 红 >85）、新样本右侧匀速滚入
//! （滑动时长 = 采样间隔，速度 = 一柱步长/拍，四轨同拍齐滑）。本册把
//! 那份「逻辑与样式」本地化：kfmv4 的历史在服务端 5s 采样 + 环形 40 点
//! 落盘；na 侧两相（服务器 HTTP / 本机直读）**同吃一个客户端环形账**
//! ——轮询器拍一拍即追加（见 svc_health），翻相清账（对象轴翻相不许串体征）。
//!
//! 分层：本册 = A 档（环形账 + 判色档 + 柱高/滑入算术，tests/sys_hist_spec.rs
//! 钉死）；轮询胶水在 svc_health（与 health 同 2s 拍、同可见性闸）；
//! 涂装在 termview（页内直涂与合成期柱层两条路同一份取件——眼手同尺）。
//!
//! 与 kfmv4 的口径差（诚实登记）：负载轨在 kfmv4 有核数（load/cores 百分
//! 比），na 的 /api/na/sys 不下发 cores → 负载轨走 kfmv4「无百分比指标」
//! 分支（窗内峰值归一 + 中性柱色，不判绿黄红）。判色三档只给内存/交换/
//! 磁盘三条占比轨。

use std::time::Instant;

use crate::termview::{CELL_H, CELL_W};

/// 环形容量（拍数）：2s 一拍 ≈ 6.4 分钟。宽度上限 = CAP−1 柱 × 柱距
/// （≈1700px 内容宽），比任何一台设备的三区左区都宽——超宽屏柱数钳住
/// 后左侧留白（不编造历史）
pub const CAP: usize = 192;
/// 柱距 = 半格（9px：柱宽 7 + 缝 2）——kfmv4 是一柱 5px 的密排小柱；
/// 手机 3x 密度下取半格密排，格律对齐且 18px 步长在 2s 拍下滚速可见
pub const STEP: u32 = CELL_W / 2;
/// 柱宽（缝 2px 归柱距）
pub const BAR_W: u32 = STEP - 2;
/// 柱高上限（内容高 = 1 格 = 36px，满柱留 6px 顶隙不贴上邻文字行）
pub const BAR_MAX_H: u32 = CELL_H - 6;
/// 柱高下限（零值也留一线：读数 = 这一拍有采样）
pub const BAR_MIN_H: u32 = 2;
/// 柱轨内容高 = 1 格
pub const TRACK_H: u32 = CELL_H;
/// 滑入动画时长（ms）——恒等于轮询拍长（kfmv4 同规：速度 = 一柱步长/拍，
/// 新柱恰好随下一拍匀速流入）。未与 svc_health::POLL_SECS 双写：
/// sys_hist_spec 有钉咬合两值（单一源靠考题执法，不靠注释）
pub const ANIM_MS: u64 = 2_000;

/// 一拍样本（四条轨同拍采——四轨同步是 kfmv4 的硬不变式：任一所动
/// 即全体齐滑，各自动会显得随机）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sample {
    /// 1 分钟负载 ×100（0.42 → 42；has_load=false 时无意义）
    pub load_x100: u32,
    /// 负载采到没（0.00 与「采不到」必须分辨——Android 拒 /proc/loadavg
    /// 是合法常态）
    pub has_load: bool,
    /// 内存占比（%，采不到 = None）
    pub mem_pct: Option<u8>,
    /// 交换占比（%，无 swap 或采不到 = None）
    pub swap_pct: Option<u8>,
    /// 磁盘占比（%，采不到 = None）
    pub disk_pct: Option<u8>,
}

/// 判色档（kfmv4 obs.ts 阈值原样：pct > 85 红 / ≥ 70 琥珀 / 其余绿；
/// 无百分比 = 中性）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Ok,
    Warn,
    Bad,
    Neutral,
}

/// 判色（A 档）：阈值唯一源（文字值与柱同吃一档）
pub fn grade(pct: Option<u8>) -> Grade {
    match pct {
        None => Grade::Neutral,
        Some(p) if p > 85 => Grade::Bad,
        Some(p) if p >= 70 => Grade::Warn,
        Some(_) => Grade::Ok,
    }
}

/// 柱轨轴（四条：负载/内存/交换/磁盘——字段序与卡面行序同一把尺，
/// sys_card::METRIC_LABELS 同序）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Load,
    Mem,
    Swap,
    Disk,
}

/// 轨序唯一源（涂装按此序排四行）
pub const METRICS: [MetricKind; 4] = [
    MetricKind::Load,
    MetricKind::Mem,
    MetricKind::Swap,
    MetricKind::Disk,
];

impl MetricKind {
    /// 该拍归一值（None = 该路采不到 → 该位不画柱——显形不编造）
    pub fn value(self, s: &Sample) -> Option<u32> {
        match self {
            MetricKind::Load => s.has_load.then_some(s.load_x100),
            MetricKind::Mem => s.mem_pct.map(u32::from),
            MetricKind::Swap => s.swap_pct.map(u32::from),
            MetricKind::Disk => s.disk_pct.map(u32::from),
        }
    }

    /// 归一底：占比轨恒 100（占比本就有绝对尺，不随窗漂移——kfmv4
    /// pct≠null 分支）；负载轨无核数口径 → 窗内峰值（至少 1 防除零）
    pub fn denom(self, peak: u32) -> u32 {
        match self {
            MetricKind::Load => peak.max(1),
            _ => 100,
        }
    }

    /// 判色档（文字值 + 柱同吃）：负载轨 = 中性（kfmv4 无百分比指标亦然）
    pub fn grade(self, s: &Sample) -> Grade {
        match self {
            MetricKind::Load => Grade::Neutral,
            MetricKind::Mem => grade(s.mem_pct),
            MetricKind::Swap => grade(s.swap_pct),
            MetricKind::Disk => grade(s.disk_pct),
        }
    }
}

/// 占比（A 档）：total = 0 → None（除零防线；used 超 total 钳满）
fn pct_of(total: u64, used: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    Some((used.min(total).saturating_mul(100) / total) as u8)
}

/// 体征快照 → 一拍样本（A 档纯函数，两相共用：服务器 HTTP / 本机直读
/// 同一份 na_sys::SysInfo——采样面的差异在轮询器，不在本函数）
pub fn sample_of(info: &na_sys::SysInfo) -> Sample {
    let (load_x100, has_load) = match info.load {
        Some(l) => ((l.l1.max(0.0) * 100.0).round() as u32, true),
        None => (0, false),
    };
    Sample {
        load_x100,
        has_load,
        mem_pct: info
            .mem
            .and_then(|m| pct_of(m.total_kb, m.total_kb.saturating_sub(m.avail_kb))),
        swap_pct: info
            .mem
            .and_then(|m| m.swap)
            .and_then(|(t, f)| pct_of(t, t.saturating_sub(f))),
        disk_pct: info.disk.and_then(|(t, a)| pct_of(t, t.saturating_sub(a))),
    }
}

/// 柱高（A 档）：value/denom 归一 × max_h 四舍五入，下限 BAR_MIN_H
/// （钳进 [BAR_MIN_H, max_h]，max_h 比下限还小时取 max_h）
pub fn bar_h(value: u32, denom: u32, max_h: u32) -> u32 {
    if denom == 0 {
        return BAR_MIN_H.min(max_h);
    }
    let v = u64::from(value.min(denom));
    let h = (v * u64::from(max_h) + u64::from(denom) / 2) / u64::from(denom);
    (h as u32).clamp(BAR_MIN_H.min(max_h), max_h)
}

/// 滑入位移（A 档）：末拍起 elapsed 毫秒 → 0..=STEP 的匀速平移量
/// （kfmv4 同规：匀速 linear，一柱步长 / 采样间隔；elapsed 钳到 anim_ms
/// 保证到点即稳态位）
pub fn slide_px(elapsed_ms: u64, anim_ms: u64, step: u32) -> u32 {
    if anim_ms == 0 {
        return step;
    }
    ((elapsed_ms.min(anim_ms) * u64::from(step)) / anim_ms) as u32
}

/// 可见柱数（A 档）：轨宽 ÷ 柱距，至少 1 柱、至多 CAP−1（历史不足 =
/// 左侧留白，柱随采样从短到长生长）
pub fn bars_for(track_w: u32, step: u32) -> usize {
    if step == 0 {
        return 0;
    }
    (track_w / step).max(1).min(CAP as u32 - 1) as usize
}

/// 尾窗取件（A 档）：末 n 条（不足则全量）——柱涂装窗口唯一取件口
pub fn tail(samples: &[Sample], n: usize) -> &[Sample] {
    &samples[samples.len().saturating_sub(n)..]
}

/// 窗内峰值（A 档）：负载柱归一底（至少 1 防除零）
pub fn window_peak(samples: &[Sample]) -> u32 {
    samples
        .iter()
        .filter(|s| s.has_load)
        .map(|s| s.load_x100)
        .max()
        .unwrap_or(1)
        .max(1)
}

/// 历史环形账（Vec + 超容丢头：CAP 级规模下一次丢一个的 O(n) 在 2s 拍上
/// 是零税，换 VecDeque 只为省这份不值得的复杂度）
#[derive(Debug, Clone, Default)]
pub struct Hist {
    samples: Vec<Sample>,
    seq: u64,
    last_at: Option<Instant>,
}

impl Hist {
    /// 追一拍（时刻 = 现在）
    pub fn push(&mut self, s: Sample) {
        self.push_at(s, Instant::now());
    }

    /// 追一拍（时刻注入——考题判滑入相位的唯一入口）
    pub fn push_at(&mut self, s: Sample, at: Instant) {
        self.samples.push(s);
        if self.samples.len() > CAP {
            self.samples.remove(0);
        }
        self.seq += 1;
        self.last_at = Some(at);
    }

    /// 清账（对象轴/后端翻相：上一相体征不许带进新相）
    pub fn clear(&mut self) {
        self.samples.clear();
        self.seq = 0;
        self.last_at = None;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 拍序号（每拍 +1——涂装重烘维与「新拍」判定唯一源；值不变也照涨，
    /// kfmv4「时钟驱动滑动」同规）
    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn as_slice(&self) -> &[Sample] {
        &self.samples
    }

    /// 末拍起经过毫秒（滑入相位源；无样本 = 0 = 稳态位——空账无动画）
    pub fn elapsed_ms(&self, now: Instant) -> u64 {
        match self.last_at {
            Some(t) => now.saturating_duration_since(t).as_millis() as u64,
            None => 0,
        }
    }

    /// 当前滑入位移 px（涂装/合成期同一把尺）
    pub fn slide_px_now(&self, now: Instant) -> u32 {
        slide_px(self.elapsed_ms(now), ANIM_MS, STEP)
    }

    /// 最新一拍的某轨判色档（卡面文字值用；无样本 = 中性占位）
    pub fn latest_grade(&self, kind: MetricKind) -> Grade {
        match self.samples.last() {
            Some(s) => kind.grade(s),
            None => Grade::Neutral,
        }
    }
}
