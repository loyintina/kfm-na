//! 环境体征历史账考题（A 档）：环形追拍/拍序/滑入位移/判色阈值/柱高归一
//! /样本合成（SysInfo→Sample）/柱层合成放置（band_place）——2026-09-21
//! 环境卡重做（复刻 kfmv4 SYS 面板滚动柱）的脑侧全钉死。
//!
//! 变异抽检（每题至少一枚，改坏答案考题必须咬）：
//! ①滑入位移到点不钳（elapsed 超拍长仍继续滑 = 柱永远对不上稳态位）
//! 必须咬；②判色阈值边界 ±1（85/86、69/70 档位错 = 危险不报警）
//! 必须咬；③环形超容不丢头（历史无限增长、柱轨画的是远古样本）必须咬；
//! ④柱高归一拿窗内峰值当占比分母（占比轨柱高随窗漂移）必须咬；
//! ⑤band_place uv 起点不随位移走（层内容冻结 = 滑动失效）必须咬；
//! ⑥band_place 纵向裁剪不钳（区窗外仍画 = 柱漏到卡外/压 tmux 卡）必须咬。

use std::time::{Duration, Instant};

use kfm_na::sys_hist::{self, Grade, Hist, MetricKind, Sample, Scale};

/// 夹具：4 核口径（load_pct = load/4，与 na-sys 采集同一算法）——
/// 负载轨走占比模式（用户拍板「服务器 4 核，负载也判色」）
fn sample(load: u32, mem: u8, swap: u8, disk: u8) -> Sample {
    Sample {
        load_x100: load,
        has_load: true,
        load_pct: Some((load as f64 / 4.0).round().min(255.0) as u8),
        mem_pct: Some(mem),
        swap_pct: Some(swap),
        disk_pct: Some(disk),
    }
}

/// 夹具：无核数口径（旧版 na-server 缺 cores 键）——负载轨回退
/// 窗内峰值归一 + 中性档
fn sample_no_cores(load: u32, mem: u8, swap: u8, disk: u8) -> Sample {
    Sample {
        load_x100: load,
        has_load: true,
        load_pct: None,
        mem_pct: Some(mem),
        swap_pct: Some(swap),
        disk_pct: Some(disk),
    }
}

fn sysinfo() -> na_sys::SysInfo {
    na_sys::SysInfo {
        load: Some(na_sys::LoadAvg {
            l1: 2.83,
            l5: 2.69,
            l15: 2.22,
            procs: Some((2, 123)),
        }),
        mem: Some(na_sys::MemInfo {
            total_kb: 15432224,
            avail_kb: 10146796,
            swap: Some((4096000, 1024000)),
        }),
        disk: Some((105_286_258_688, 24_877_244_416)),
        uptime_s: Some(7849375),
        cores: Some(4),
    }
}

// ---- 环形账 ----

#[test]
fn spec_环形_超容丢头() {
    let mut h = Hist::default();
    let t0 = Instant::now();
    for i in 0..(sys_hist::CAP + 7) {
        h.push_at(sample(i as u32, 0, 0, 0), t0);
    }
    assert_eq!(h.len(), sys_hist::CAP, "环形超容必须丢头（变异③）");
    assert_eq!(h.seq(), (sys_hist::CAP + 7) as u64, "拍序照涨不清零");
    assert_eq!(
        h.as_slice()[0].load_x100,
        7,
        "丢的是最旧的（首样本 = 第 7 拍）"
    );
    assert_eq!(
        h.as_slice()[h.len() - 1].load_x100,
        (sys_hist::CAP + 6) as u32,
        "尾部 = 最新一拍"
    );
}

#[test]
fn spec_环形_清账() {
    let mut h = Hist::default();
    h.push(sample(10, 1, 2, 3));
    h.clear();
    assert!(h.is_empty());
    assert_eq!(h.seq(), 0, "清账连拍序一起归零（翻相语义）");
    // 无在播相位（空账/落盘恢复）= **稳态位**，不是起步相：起步相会把
    // 最新柱留在进口条外（右缘缺一根），恢复态必须贴右缘静止
    assert_eq!(h.elapsed_ms(Instant::now()), sys_hist::ANIM_MS);
    assert_eq!(h.slide_px_now(Instant::now()), sys_hist::STEP);
}

#[test]
fn spec_拍序_值不变照涨() {
    // kfmv4 时钟驱动同规：稳态值不变也必须追拍（否则柱轨静止）
    let mut h = Hist::default();
    let s = sample(10, 50, 50, 50);
    h.push(s);
    h.push(s);
    assert_eq!(h.seq(), 2, "同值也是两拍");
    assert_eq!(h.len(), 2);
}

// ---- 滑入位移 ----

#[test]
fn spec_滑入_匀速到点钳() {
    let step = sys_hist::STEP;
    assert_eq!(sys_hist::slide_px(0, 2000, step), 0, "末拍瞬间 = 起点");
    assert_eq!(
        sys_hist::slide_px(1000, 2000, step),
        step / 2,
        "半拍 = 半柱步长（匀速）"
    );
    assert_eq!(
        sys_hist::slide_px(2000, 2000, step),
        step,
        "到点 = 一柱步长"
    );
    assert_eq!(
        sys_hist::slide_px(99_999, 2000, step),
        step,
        "超拍长必须钳在稳态位（变异①：不钳 = 永远滑不对位）"
    );
    assert_eq!(
        sys_hist::slide_px(500, 0, step),
        step,
        "零时长 = 恒稳态位（除零防线）"
    );
}

#[test]
fn spec_滑入_相位随末拍() {
    let mut h = Hist::default();
    let t0 = Instant::now();
    h.push_at(sample(10, 50, 50, 50), t0);
    assert_eq!(h.elapsed_ms(t0), 0);
    assert_eq!(h.slide_px_now(t0), 0);
    let half = t0 + Duration::from_millis(sys_hist::ANIM_MS / 2);
    assert_eq!(h.elapsed_ms(half), sys_hist::ANIM_MS / 2);
    assert_eq!(h.slide_px_now(half), sys_hist::STEP / 2);
    assert_eq!(h.slide_px_now(t0 + Duration::from_secs(9)), sys_hist::STEP);
}

#[test]
fn spec_滑入_时长咬合轮询拍() {
    // 单一源执法：滑入时长必须 = 轮询拍长（kfmv4「速度 = 一柱步长/拍」）
    assert_eq!(
        sys_hist::ANIM_MS,
        kfm_na::svc_health::POLL_SECS * 1000,
        "柱滑入时长漂离采样拍长 = 流动感与采样不同步"
    );
}

// ---- 判色档 ----

#[test]
fn spec_判色_阈值边界() {
    assert_eq!(sys_hist::grade(None), Grade::Neutral);
    assert_eq!(sys_hist::grade(Some(0)), Grade::Ok);
    assert_eq!(sys_hist::grade(Some(69)), Grade::Ok);
    assert_eq!(
        sys_hist::grade(Some(70)),
        Grade::Warn,
        "≥70 进琥珀（变异②：边界挪一格 = 档位错）"
    );
    assert_eq!(sys_hist::grade(Some(85)), Grade::Warn, "85 仍是琥珀");
    assert_eq!(sys_hist::grade(Some(86)), Grade::Bad, ">85 才进红");
    assert_eq!(sys_hist::grade(Some(100)), Grade::Bad);
}

#[test]
fn spec_判色_轨轴分流() {
    let s = sample(42, 90, 10, 0);
    // 四核口径：负载 42/4 = 11% → 安全档（占比模式判色）
    assert_eq!(
        MetricKind::Load.grade_in(&s, Scale::Pct),
        Grade::Ok,
        "有核数 = 负载按占比判色（用户拍板）"
    );
    assert_eq!(MetricKind::Mem.grade_in(&s, Scale::Pct), Grade::Bad);
    assert_eq!(MetricKind::Swap.grade_in(&s, Scale::Pct), Grade::Ok);
    assert_eq!(MetricKind::Disk.grade_in(&s, Scale::Pct), Grade::Ok);
    // 无核数口径：负载回中性（kfmv4 无百分比分支同规），其余轨不受影响
    let nc = sample_no_cores(283, 90, 10, 0);
    assert_eq!(
        MetricKind::Load.grade_in(&nc, Scale::Peak(283)),
        Grade::Neutral,
        "缺核数 = 中性不判色"
    );
    assert_eq!(MetricKind::Mem.grade_in(&nc, Scale::Peak(283)), Grade::Bad);
    // 该路采不到 = 中性（不判色不连坐）
    let miss = Sample {
        load_x100: 0,
        has_load: false,
        load_pct: None,
        mem_pct: None,
        swap_pct: None,
        disk_pct: None,
    };
    assert_eq!(MetricKind::Mem.grade_in(&miss, Scale::Pct), Grade::Neutral);
    assert!(MetricKind::Load.bar_value(&miss, Scale::Peak(1)).is_none());
    assert!(MetricKind::Mem.bar_value(&miss, Scale::Pct).is_none());
}

#[test]
fn spec_轨模式_占比与窗峰() {
    // 有核数 = 占比轨（绝对尺，不随窗漂移）
    let with = [sample(42, 0, 0, 0), sample(100, 0, 0, 0)];
    assert_eq!(sys_hist::scale_of(MetricKind::Load, &with), Scale::Pct);
    // 无核数 = 窗内峰值归一（至少 1）
    let nc = [sample_no_cores(283, 0, 0, 0), sample_no_cores(100, 0, 0, 0)];
    assert_eq!(sys_hist::scale_of(MetricKind::Load, &nc), Scale::Peak(283));
    assert_eq!(
        sys_hist::scale_of(MetricKind::Load, &[]),
        Scale::Peak(1),
        "空窗给 1 防除零"
    );
    // 其余三轨恒占比
    for k in [MetricKind::Mem, MetricKind::Swap, MetricKind::Disk] {
        assert_eq!(sys_hist::scale_of(k, &nc), Scale::Pct);
        assert_eq!(sys_hist::scale_of(k, &[]), Scale::Pct);
    }
    // 归一底
    assert_eq!(Scale::Pct.denom(), 100);
    assert_eq!(Scale::Peak(0).denom(), 1);
    assert_eq!(Scale::Peak(283).denom(), 283);
    // 值取件两模式分流（无核数 = 原始 ×100；有核数 = 占比）
    assert_eq!(
        MetricKind::Load.bar_value(&nc[0], Scale::Peak(283)),
        Some(283)
    );
    assert_eq!(MetricKind::Load.bar_value(&with[0], Scale::Pct), Some(11));
}

#[test]
fn spec_轨轴_取值口径() {
    // 占比取件（内存/交换/磁盘恒占比；负载看核数）
    let s = sample(42, 0, 0, 0);
    assert_eq!(
        MetricKind::Load.pct(&s),
        Some(11),
        "0.42/4 核 = 10.5% → 11%"
    );
    assert_eq!(MetricKind::Mem.pct(&s), Some(0));
    assert_eq!(MetricKind::Swap.pct(&s), Some(0));
    assert_eq!(MetricKind::Disk.pct(&s), Some(0));
    let nc = sample_no_cores(42, 0, 0, 0);
    assert_eq!(MetricKind::Load.pct(&nc), None, "缺核数 = 无占比口径");
    assert_eq!(
        MetricKind::Load.bar_value(&nc, Scale::Peak(84)),
        Some(42),
        "缺核数走原始值（窗峰归一）"
    );
}

// ---- 柱高 ----

#[test]
fn spec_柱高_四舍五入与钳制() {
    let max = sys_hist::BAR_MAX_H;
    assert_eq!(sys_hist::bar_h(100, 100, max), max, "满值 = 满柱");
    assert_eq!(sys_hist::bar_h(50, 100, max), max / 2, "半值 = 半柱");
    assert_eq!(
        sys_hist::bar_h(0, 100, max),
        sys_hist::BAR_MIN_H,
        "零值留一线（这一拍有采样）"
    );
    assert_eq!(
        sys_hist::bar_h(0, 0, max),
        sys_hist::BAR_MIN_H,
        "除零防线（denom=0 = 最小柱不上限）"
    );
    assert_eq!(
        sys_hist::bar_h(200, 100, max),
        max,
        "超归一值钳满（不越上限）"
    );
    assert_eq!(
        sys_hist::bar_h(10, 100, 3),
        sys_hist::BAR_MIN_H.min(3),
        "上限比下限还小时取上限，否则保下限"
    );
    assert_eq!(
        sys_hist::bar_h(50, 100, 1),
        1,
        "上限 < 下限时取上限（钳区不反）"
    );
    // 归一底用窗内峰值（变异④：占比轨拿峰值当分母 = 柱高随窗漂移）
    let win = [sample(283, 0, 0, 0), sample(100, 0, 0, 0)];
    assert_eq!(sys_hist::window_peak(&win), 283);
    assert_eq!(
        sys_hist::bar_h(
            sys_hist::window_peak(&win),
            sys_hist::scale_of(MetricKind::Load, &win).denom(),
            max
        ),
        max,
        "峰值拍 = 满柱"
    );
    assert_eq!(sys_hist::window_peak(&[]), 1, "空窗峰值给 1 不除零");
}

// ---- 取件与柱数 ----

#[test]
fn spec_取件_尾窗与柱数() {
    let mut h = Hist::default();
    for i in 0..10u32 {
        h.push(sample(i, 0, 0, 0));
    }
    assert_eq!(sys_hist::tail(h.as_slice(), 3).len(), 3);
    assert_eq!(
        sys_hist::tail(h.as_slice(), 3)[2].load_x100,
        9,
        "尾窗取最新"
    );
    assert_eq!(sys_hist::tail(h.as_slice(), 99).len(), 10, "不足给全量");
    assert_eq!(sys_hist::tail(h.as_slice(), 0).len(), 0);
    // 柱数 = 轨宽/柱距，至少 1 柱、至多 CAP−1（超宽屏钳住 = 不编造历史）
    assert_eq!(sys_hist::bars_for(0, sys_hist::STEP), 1);
    assert_eq!(sys_hist::bars_for(9 * sys_hist::STEP, sys_hist::STEP), 9);
    assert_eq!(
        sys_hist::bars_for(u32::MAX / 2, sys_hist::STEP),
        sys_hist::CAP - 1,
        "柱数上限 = 环容量 − 1"
    );
    assert_eq!(sys_hist::bars_for(100, 0), 0, "零柱距防线");
}

// ---- 样本合成 ----

#[test]
fn spec_样本_占比与缺失() {
    let s = sys_hist::sample_of(&sysinfo());
    assert!(s.has_load);
    assert_eq!(s.load_x100, 283, "1 分钟负载 ×100 取整");
    // 内存已用 = 15432224−10146796 = 5285428K / 15432224K = 34.2% → 34
    assert_eq!(s.mem_pct, Some(34));
    // 交换已用 = 4096000−1024000 = 3072000K / 4096000K = 75%
    assert_eq!(s.swap_pct, Some(75));
    // 磁盘已用 = 105286258688−24877244416 / 总 = 76.36% → 76
    assert_eq!(s.disk_pct, Some(76));
    // 负载占比 = 2.83/4 核 = 70.75% → 71（恰落琥珀档——真机同景）
    assert_eq!(s.load_pct, Some(71));
}

#[test]
fn spec_样本_缺核数退化() {
    // 旧版 na-server（缺 cores 键）：负载占比 None → 该轨回退窗峰归一 + 中性
    let mut i = sysinfo();
    i.cores = None;
    let s = sys_hist::sample_of(&i);
    assert_eq!(s.load_pct, None);
    assert!(s.has_load, "原始负载照常采到（文字值不受影响）");
    assert_eq!(s.load_x100, 283);
    // 核数 0 也按缺口径处理（除零防线）
    let mut i2 = sysinfo();
    i2.cores = Some(0);
    assert_eq!(sys_hist::sample_of(&i2).load_pct, None);
    // 负载整路缺失 = 无占比（连坐只到本轨）
    let mut i3 = sysinfo();
    i3.load = None;
    let s3 = sys_hist::sample_of(&i3);
    assert_eq!(s3.load_pct, None);
    assert_eq!(s3.mem_pct, Some(34), "内存路不连坐");
}

#[test]
fn spec_样本_单路缺失不连坐() {
    let mut i = sysinfo();
    i.load = None;
    let s = sys_hist::sample_of(&i);
    assert!(
        !s.has_load,
        "负载采不到 = has_load 假（0.00 与采不到要分辨）"
    );
    assert_eq!(
        MetricKind::Load.bar_value(&s, Scale::Peak(1)),
        None,
        "负载采不到 → 该拍不画柱（两种口径都不画）"
    );
    assert_eq!(MetricKind::Load.pct(&s), None);
    assert_eq!(s.mem_pct, Some(34), "内存路不许被连坐");
    // 无 swap（total=0 或键缺）→ None 不编造 0%
    let mut i2 = sysinfo();
    i2.mem.as_mut().unwrap().swap = Some((0, 0));
    assert_eq!(sys_hist::sample_of(&i2).swap_pct, None, "零总量 = 除零防线");
    // 内存路整路缺失 → mem/swap 双 None（swap 挂在 mem 下）
    let mut i3 = sysinfo();
    i3.mem = None;
    let s3 = sys_hist::sample_of(&i3);
    assert_eq!(s3.mem_pct, None);
    assert_eq!(s3.swap_pct, None);
}

#[test]
fn spec_记录_最新档查询() {
    let mut h = Hist::default();
    assert_eq!(
        h.latest_grade(MetricKind::Mem),
        Grade::Neutral,
        "空账 = 中性（无数据不报警）"
    );
    // 4 核口径：负载也判色（用户拍板）——占比可满，满即红
    h.push(sample(340, 10, 10, 10)); // 3.40/4 = 85% → 琥珀
    assert_eq!(h.latest_grade(MetricKind::Load), Grade::Warn);
    h.push(sample(400, 10, 10, 10)); // 4.00/4 = 100% → 红
    assert_eq!(h.latest_grade(MetricKind::Load), Grade::Bad);
    // 无核数口径 = 负载恒中性（不被窗峰归一伪造成警戒）
    let mut h2 = Hist::default();
    h2.push(sample_no_cores(400, 10, 10, 10));
    assert_eq!(h2.latest_grade(MetricKind::Load), Grade::Neutral);
    h.clear();
    h.push(sample(10, 90, 10, 10));
    assert_eq!(h.latest_grade(MetricKind::Mem), Grade::Bad);
    h.push(sample(10, 10, 10, 10));
    assert_eq!(h.latest_grade(MetricKind::Mem), Grade::Ok, "随最新一拍翻档");
}

// ---- 柱层合成放置 ----

#[test]
fn spec_柱层_uv随位移滑() {
    use kfm_na::ui::sys_card;
    let card = kfm_na::ui::dual_pool::PoolRect {
        x: 0,
        y: 100,
        w: 600,
        h: sys_card::CARD_H,
    };
    let band = sys_card::band_of(&sys_card::layout_in(card));
    let clip = (0, 4000);
    let p0 = sys_card::band_place(&band, 0, clip);
    let p1 = sys_card::band_place(&band, sys_hist::STEP, clip);
    let cw = band.canvas_w as f32;
    assert_eq!(p0.tracks[0].uv.0, 0.0, "位移 0 = 源窗原点 0");
    assert!(
        (p0.tracks[0].uv.2 - band.tracks[0].w as f32 / cw).abs() < 1e-4,
        "源窗宽 = 轨宽（= kfmv4 overflow:hidden 的等价物）"
    );
    assert!(
        (p1.tracks[0].uv.0 - sys_hist::STEP as f32 / cw).abs() < 1e-4,
        "变异⑤：位移必须只挪源窗原点（不挪 = 层冻结滑动失效）"
    );
    // 尺寸语义钉（首版把远角当尺寸传 = 源窗纵段翻倍，redroid 截屏定罪）：
    // uv.zw 恒是**尺寸**——位移只挪原点，尺寸逐值不变
    assert_eq!(
        p0.tracks[0].uv.2, p1.tracks[0].uv.2,
        "uv 尺寸不许随位移变（远角语义复辟即咬）"
    );
    assert_eq!(p0.tracks[0].uv.3, p1.tracks[0].uv.3);
    let (o0, _, s0, _) = p0.tracks[0].uv;
    let (o1, _, s1, _) = p1.tracks[0].uv;
    assert!(
        (o0 + s0 - band.tracks[0].w as f32 / cw).abs() < 1e-4,
        "位移 0：窗右缘 = 轨右缘"
    );
    assert!(
        (o1 + s1 - (sys_hist::STEP + band.tracks[0].w) as f32 / cw).abs() < 1e-4,
        "稳态位：窗右缘取到 轨宽+一柱距"
    );
    assert!(
        o1 + s1 <= 1.0 + 1e-6,
        "窗右缘不许越画布（画布宽 = 轨宽+柱距 的由来）"
    );
    // dest 矩形 = 轨矩形（页坐标，面板偏移归合成期加）
    assert_eq!(p0.tracks[0].rect.0, band.tracks[0].x as f32);
    assert_eq!(p0.tracks[0].rect.2, band.tracks[0].w as f32);
    assert!(p0.tracks.iter().all(|t| t.visible));
    // 四轨各占层内一段（v 段不重叠、按轨序递增）
    let band2 = band;
    let (v0, v1) = band2.uv_v(0);
    let (w0, w1) = band2.uv_v(1);
    assert!(v1 <= w0 + 1e-6, "轨间 v 段不重叠");
    assert!(w0 < w1);
    assert!((v0 - 0.0).abs() < 1e-6);
    assert!((band2.uv_v(3).1 - 1.0).abs() < 1e-6, "末轨 v 段收在 1.0");
}

#[test]
fn spec_柱层_纵向裁剪() {
    use kfm_na::ui::sys_card;
    let card = kfm_na::ui::dual_pool::PoolRect {
        x: 0,
        y: 100,
        w: 600,
        h: sys_card::CARD_H,
    };
    let band = sys_card::band_of(&sys_card::layout_in(card));
    let t0 = &band.tracks[0];
    let t1 = &band.tracks[1];
    // 裁剪带罩轨 0 下半 + 轨 1 上半（区窗被滚到两轨之间）→ 两轨各砍一半
    let clip = (
        t0.y + i64::from(band.track_h / 2),
        t1.y + i64::from(band.track_h / 2),
    );
    let p = sys_card::band_place(&band, 0, clip);
    assert!(p.tracks[0].visible);
    assert_eq!(
        p.tracks[0].rect.3,
        (band.track_h / 2) as f32,
        "变异⑥：dest 高必须被裁剪带砍短（不砍 = 柱漏到卡外/压 tmux 卡）"
    );
    assert!(p.tracks[1].visible, "轨 1 的上半在带内");
    assert_eq!(p.tracks[1].rect.3, (band.track_h / 2) as f32);
    assert_eq!(
        p.tracks[1].rect.1, t1.y as f32,
        "轨 1 顶 = 带内（不带顶部偏移）"
    );
    assert!(
        p.tracks[0].rect.1 > t0.y as f32,
        "轨 0 的 dest 顶随带顶下移"
    );
    assert!(!p.tracks[2].visible, "带外轨不画");
    assert!(!p.tracks[3].visible);
    // 全部裁掉 = 全不可见（区窗外零绘制）
    let p2 = sys_card::band_place(&band, 0, (0, 1));
    assert!(p2.tracks.iter().all(|t| !t.visible));
}

#[test]
fn spec_柱层_轨序咬合() {
    // 轨序唯一源：sys_hist::METRICS 与卡面标签同序同长（涂装按序排四行）
    assert_eq!(
        sys_hist::METRICS.len(),
        kfm_na::ui::sys_card::METRIC_LABELS.len()
    );
    assert_eq!(
        sys_hist::METRICS[0],
        MetricKind::Load,
        "卡面首轨 = 负载（kfmv4 面板同序）"
    );
}

// ---- 示警色档（宪法 §2.4，2026-09-21 环境卡重做）----

#[test]
fn spec_示警色档_映射与定值() {
    use kfm_na::sys_hist::Grade;
    use kfm_na::termview::{
        WARN_AMBER, WARN_NEUTRAL, WARN_OK, WARN_RED, sys_grade_color, sys_grade_fg,
    };
    // 判色档 → 柱色（涂装唯一映射；错档 = 危险不报警）
    assert_eq!(sys_grade_color(Grade::Ok), WARN_OK);
    assert_eq!(sys_grade_color(Grade::Warn), WARN_AMBER);
    assert_eq!(sys_grade_color(Grade::Bad), WARN_RED);
    assert_eq!(sys_grade_color(Grade::Neutral), WARN_NEUTRAL);
    // 中性柱固定色（不变 accent 走）——与琥珀必须可分（redroid 实拍：
    // 黄调 accent 下中性柱与琥珀柱几乎同色，故定死）
    assert_ne!(WARN_NEUTRAL, WARN_AMBER);
    assert_ne!(WARN_NEUTRAL, WARN_OK);
    assert_ne!(WARN_NEUTRAL, WARN_RED);
    assert_ne!(WARN_OK, WARN_AMBER);
    assert_ne!(WARN_AMBER, WARN_RED);
    assert_eq!(WARN_RED, 0x00E0_6060, "危险红与 err 档同值（一个语义家族）");
    // 文字值：只有示警才上色，安全/中性走白档
    assert_eq!(sys_grade_fg(Grade::Ok, 0x0080_8080), 0x0080_8080);
    assert_eq!(sys_grade_fg(Grade::Neutral, 0x0080_8080), 0x0080_8080);
    assert_eq!(sys_grade_fg(Grade::Warn, 0x0080_8080), WARN_AMBER);
    assert_eq!(sys_grade_fg(Grade::Bad, 0x0080_8080), WARN_RED);
}

// ---- 落盘（默认铺开：进程重启不清零柱轨） ----

#[test]
fn spec_落盘_往返原样() {
    let mut a = Hist::default();
    let t0 = Instant::now();
    a.push_at(sample(283, 34, 75, 76), t0);
    a.push_at(sample_no_cores(100, 0, 0, 0), t0);
    let mut b = Hist::default();
    b.push_at(sample(400, 90, 10, 12), t0);
    let text = sys_hist::encode_hist([&a, &b], ["root@10.0.0.1:22", ""]);
    let (ra, ta) = sys_hist::decode_hist(&text);
    assert_eq!(ta[0], "root@10.0.0.1:22", "归属串往返");
    assert_eq!(ra[0].len(), a.len());
    assert_eq!(ra[0].seq(), a.seq(), "拍序往返");
    assert_eq!(ra[0].as_slice(), a.as_slice(), "样本逐值往返");
    assert_eq!(ra[1].as_slice(), b.as_slice(), "两相各归各段");
    // 恢复态相位 = 稳态（无在播动画）
    assert_eq!(ra[0].slide_px_now(Instant::now()), sys_hist::STEP);
}

#[test]
fn spec_落盘_归属串空格转义() {
    let h = Hist::default();
    let text = sys_hist::encode_hist([&h, &h], ["my server box", ""]);
    let (_, t) = sys_hist::decode_hist(&text);
    assert_eq!(t[0], "my server box", "空格 %20 往返（段头以空格分词）");
    assert_eq!(t[1], "", "空归属 = 空串（未起 supervisor）");
}

#[test]
fn spec_落盘_坏件宽容() {
    let h = Hist::default();
    let good = sys_hist::encode_hist([&h, &h], ["t", ""]);
    // 版本不认 = 整份弃（宁可重攒不误读旧语义）
    let (r, t) = sys_hist::decode_hist("kfm-na-sys-hist v0\n#server t 3\n1 1 - - - -\n");
    assert!(r.iter().all(|x| x.is_empty()));
    assert_eq!(t[0], "", "版本不认连归属也不认");
    // 段头坏 = 该段弃；坏行 = 跳该行不连坐整份
    let mut a = Hist::default();
    let t0 = Instant::now();
    a.push_at(sample(10, 1, 2, 3), t0);
    let mut text = sys_hist::encode_hist([&a, &h], ["srv", ""]);
    text = text.replace("#server srv 1", "#bogus srv 1");
    let (r2, _) = sys_hist::decode_hist(&text);
    assert!(r2[0].is_empty(), "段头坏 = 该段弃（不许误挂到别的段）");
    // 半截行（断电写一半）跳过
    let text2 = format!("{good}#bogus x 0\n12 1\nnot a line\n");
    let (r3, _) = sys_hist::decode_hist(&text2);
    assert_eq!(r3[1].len(), 0);
    assert!(r3[0].is_empty());
}

#[test]
fn spec_落盘_超容保末尾() {
    // 落盘件可能是冒版本留下的更大环：恢复时截到 CAP，保**末尾**（最新）
    let mut h = Hist::default();
    let t0 = Instant::now();
    for i in 0..(sys_hist::CAP + 5) {
        h.push_at(sample(i as u32, 0, 0, 0), t0);
    }
    let text = sys_hist::encode_hist([&h, &Hist::default()], ["t", ""]);
    let (r, _) = sys_hist::decode_hist(&text);
    assert_eq!(r[0].len(), sys_hist::CAP);
    assert_eq!(
        r[0].as_slice()[r[0].len() - 1].load_x100,
        (sys_hist::CAP + 4) as u32,
        "保末尾 = 最新的那批"
    );
}
