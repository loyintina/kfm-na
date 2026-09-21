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

use crate::endpoint::EndpointKind;
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
    /// 负载占比（= l1/核数×100，可 >100——满载排队）。核数未知（旧版
    /// na-server 或缺键）= None → 负载轨回退窗内峰值归一 + 中性档
    /// （2026-09-21 用户拍板「服务器 4 核，负载也判色」后新立）
    pub load_pct: Option<u8>,
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
    /// 该拍占比（None = 该路采不到；**负载缺核数时恒 None**——原始值
    /// 走 Scale::Peak 分支的 bar_value）
    pub fn pct(self, s: &Sample) -> Option<u8> {
        match self {
            MetricKind::Load => s.load_pct,
            MetricKind::Mem => s.mem_pct,
            MetricKind::Swap => s.swap_pct,
            MetricKind::Disk => s.disk_pct,
        }
    }

    /// 该拍柱值（None = 该位不画柱——显形不编造）：占比轨 = 占比；
    /// 负载无核数（Scale::Peak）= 原始 ×100 按窗内峰值归一
    pub fn bar_value(self, s: &Sample, scale: Scale) -> Option<u32> {
        match (self, scale) {
            (MetricKind::Load, Scale::Peak(_)) => s.has_load.then_some(s.load_x100),
            _ => self.pct(s).map(u32::from),
        }
    }

    /// 判色档（文字值 + 柱同吃）：负载缺核数 = 中性（kfmv4 无百分比
    /// 指标亦然）；有核数 = 占比判色（用户拍板「负载也判色」）
    pub fn grade_in(self, s: &Sample, scale: Scale) -> Grade {
        match (self, scale) {
            (MetricKind::Load, Scale::Peak(_)) => Grade::Neutral,
            _ => grade(self.pct(s)),
        }
    }
}

/// 轨归一/判色模式（窗级一次判——占比轨不随窗漂移；同一轨混窗时以
/// 「有占比」为准，缺占比的拍不画柱）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// 占比轨：柱高 = 值/100，判色照三档
    Pct,
    /// 无占比口径（负载缺核数）：柱高 = 值/窗内峰值（至少 1 防除零），
    /// 中性档不判色
    Peak(u32),
}

impl Scale {
    pub fn denom(self) -> u32 {
        match self {
            Scale::Pct => 100,
            Scale::Peak(p) => p.max(1),
        }
    }
}

/// 轨模式裁决（A 档纯函数）：负载轨有任一拍带占比 = Pct；否则 Peak
/// （窗内峰值归一）；其余三轨恒 Pct
pub fn scale_of(kind: MetricKind, samples: &[Sample]) -> Scale {
    match kind {
        MetricKind::Load => {
            if samples.iter().any(|s| s.load_pct.is_some()) {
                Scale::Pct
            } else {
                Scale::Peak(window_peak(samples))
            }
        }
        _ => Scale::Pct,
    }
}

/// 相索引（A 档纯函数）：对象轴两相各一本历史账（服务器/本地互不清带
/// ——切换环境各自续摊，不是共用一本再清账）
pub fn hist_idx(kind: EndpointKind) -> usize {
    match kind {
        EndpointKind::Server => 0,
        EndpointKind::Local => 1,
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
    // 负载占比：核数已知才算（l1/核数×100，上限钳 255——占比可超 100，
    // 那是「排队比核还多」的合法读数，柱高由满柱钳住、判色归红）
    let load_pct = match (info.load, info.cores) {
        (Some(l), Some(c)) if c > 0 => {
            Some(((l.l1.max(0.0) / f64::from(c) * 100.0).round() as u32).min(255) as u8)
        }
        _ => None,
    };
    Sample {
        load_x100,
        has_load,
        load_pct,
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

    /// 落盘恢复注入（A 档）：整段样本 + 拍序；相位锚清空 = 稳态位
    /// （见 elapsed_ms）。超容保留末尾（落盘件可能是更早版本的更大环）
    pub fn restore(&mut self, samples: Vec<Sample>, seq: u64) {
        self.samples = samples;
        if self.samples.len() > CAP {
            let cut = self.samples.len() - CAP;
            self.samples.drain(0..cut);
        }
        self.seq = seq;
        self.last_at = None;
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

    /// 末拍起经过毫秒（滑入相位源）：**无在播相位（空账/落盘恢复态）
    /// = 拍长 → 位移恒一柱步长 = 稳态位**（恢复的历史没有在播动画，
    /// 必须落在「新柱贴在右缘」的静止相，不是进口条外的起步相）
    pub fn elapsed_ms(&self, now: Instant) -> u64 {
        match self.last_at {
            Some(t) => now.saturating_duration_since(t).as_millis() as u64,
            None => ANIM_MS,
        }
    }

    /// 当前滑入位移 px（涂装/合成期同一把尺）
    pub fn slide_px_now(&self, now: Instant) -> u32 {
        slide_px(self.elapsed_ms(now), ANIM_MS, STEP)
    }

    /// 最新一拍的某轨判色档（卡面文字值用；无样本 = 中性占位）。
    /// 模式按同一条窗裁决（scale_of）——柱与文字同档
    pub fn latest_grade(&self, kind: MetricKind) -> Grade {
        let scale = scale_of(kind, &self.samples);
        match self.samples.last() {
            Some(s) => kind.grade_in(s, scale),
            None => Grade::Neutral,
        }
    }
}

// ---- 落盘（A 档纯逻辑：编码/解码，2026-09-21「默认铺开」） ----
//
// 用户拍板「别做从左长，最好是默认就是铺开的」：柱轨的铺开程度 = 手上
// 有多少拍历史。na 侧历史只在内存 → 每次重启（热更/回滚/系统杀）都从零
// 长。落盘件 + 前台即抢（svc_health）两条合起来，才让「一开页就是满窗」
// 在不编造历史的前提下成立。
//
// 格式（文本行制，人可读、坏件可局部跳过——落盘是缓存不是账本）：
//   kfm-na-sys-hist v1
//   #<kind> <target> <seq>          kind = server|local，target 空格转 %20
//   <load_x100> <has_load> <load_pct> <mem> <swap> <disk>   采不到 = -
//   #local - 0
//   ...

/// 落盘格式版本（不认的版本整份弃——宁可重攒也不误读旧语义）
pub const HIST_FORMAT: &str = "kfm-na-sys-hist v1";

fn enc_pct(v: Option<u8>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".to_string())
}

/// 段头第三列：空归属写 `-`（保留列位——漏占位会让 seq 顶到 target 位，
/// 恢复出 "0" 这种假归属，考题实咬）
fn enc_target(t: &str) -> String {
    if t.is_empty() {
        "-".to_string()
    } else {
        t.replace(' ', "%20")
    }
}

fn dec_target(t: &str) -> String {
    t.replace("%20", " ")
}

/// 编码（A 档纯函数）：两相各一段（空账也写段头——保留 target 账，
/// 恢复时知道这本账属于谁）
pub fn encode_hist(rings: [&Hist; 2], targets: [&str; 2]) -> String {
    let mut out = String::from(HIST_FORMAT);
    out.push('\n');
    for (i, kind) in ["server", "local"].iter().enumerate() {
        out.push_str(&format!(
            "#{} {} {}\n",
            kind,
            enc_target(targets[i]),
            rings[i].seq()
        ));
        for s in rings[i].as_slice() {
            out.push_str(&format!(
                "{} {} {} {} {} {}\n",
                s.load_x100,
                u8::from(s.has_load),
                enc_pct(s.load_pct),
                enc_pct(s.mem_pct),
                enc_pct(s.swap_pct),
                enc_pct(s.disk_pct)
            ));
        }
    }
    out
}

/// 解码（A 档纯函数）：宽容——版本不认 = 两段全空；段头坏 = 该段弃；
/// 单行坏 = 跳该行（不许连坐整份）；缺段头 = 该行归当前段
pub fn decode_hist(text: &str) -> ([Hist; 2], [String; 2]) {
    let mut rings = [Hist::default(), Hist::default()];
    let mut targets = [String::new(), String::new()];
    let mut lines = text.lines();
    match lines.next() {
        Some(l) if l.trim() == HIST_FORMAT => {}
        _ => return (rings, targets), // 版本不认 → 整份弃
    }
    let mut cur: Option<usize> = None;
    let mut seq: [u64; 2] = [0, 0];
    let mut buf: [Vec<Sample>; 2] = [Vec::new(), Vec::new()];
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(head) = t.strip_prefix('#') {
            // 段头：kind target seq
            let mut it = head.split_whitespace();
            let kind = it.next().unwrap_or("");
            let target = dec_target(it.next().unwrap_or("-"));
            let sq = it.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            cur = match kind {
                "server" => Some(0),
                "local" => Some(1),
                _ => None,
            };
            if let Some(i) = cur {
                targets[i] = if target == "-" { String::new() } else { target };
                seq[i] = sq;
            }
            continue;
        }
        let Some(i) = cur else { continue };
        let f: Vec<&str> = t.split_whitespace().collect();
        if f.len() < 6 {
            continue; // 坏行跳过
        }
        let num = |v: &str| v.parse::<u32>().ok();
        let pct = |v: &str| -> Option<u8> { if v == "-" { None } else { v.parse::<u8>().ok() } };
        let Some(load_x100) = num(f[0]) else { continue };
        let Some(has_load) = num(f[1]) else { continue };
        buf[i].push(Sample {
            load_x100,
            has_load: has_load != 0,
            load_pct: pct(f[2]),
            mem_pct: pct(f[3]),
            swap_pct: pct(f[4]),
            disk_pct: pct(f[5]),
        });
    }
    for i in 0..2 {
        rings[i].restore(std::mem::take(&mut buf[i]), seq[i]);
    }
    (rings, targets)
}
