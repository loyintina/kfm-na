//! selfwatch_spec.rs — 自观测第四块三件套考题(A 档,2026-08-27)
//!
//! 契约:
//! ①crash.rs 信号行格式钉死:`SIGNAL sig=11 addr=0xdeadbeef\n`;
//!   addr=0 也要出一个 0(不许出空串);缓冲不足静默截断不越界;
//! ②alert 三规则各钉死:帧耗过线且新峰值才报/峰值不刷新不重报;
//!   RSS 绝线与窗口净涨共用冷却;会话死亡窗口新增 ≥3 才报且有冷却;
//! ③水位环满帽丢最旧;历史行格式钉死(空格分隔 key=value,
//!   均值防除零 = 一帧没画报 0)。
//! 变异抽检口径:改坏格式化(十六进制字母大小写/丢前导)、冷却改
//! 双报、环推挤反序,本文件必须红。

use kfm_na::crash::format_signal_line;
use kfm_na::gate::{
    ALERT_DEATHS_NEW, ALERT_DRAW_MS, ALERT_RSS_ABS_KB, ALERT_RSS_GROW_KB, AlertState, StatsSnap,
    alert_check, format_history_line, ring_push,
};
use std::collections::VecDeque;

/// 造一张「全零正常」快照,各考题按需改字段
fn snap() -> StatsSnap {
    StatsSnap {
        uptime_ms: 0,
        foreground: true,
        loop_age_ms: Some(10),
        frames: 100,
        pump_calls: 50,
        pump_bytes: 4096,
        shots: 0,
        texts: 0,
        keys: 0,
        keys_bytes: 0,
        active: "local".into(),
        sessions: "local,remote".into(),
        draw_total_ms: 500,
        draw_max_ms: 20,
        cpu_jiffies: 1234,
        rss_kb: 200 * 1024,
        bytes_local: 100,
        bytes_remote: 200,
        bytes_other: 0,
        session_deaths: 0,
        touches: 0,
    }
}

// ---- ①信号行格式 ----

#[test]
fn spec_signal_行格式钉死() {
    let mut buf = [0u8; 128];
    let n = format_signal_line(11, 0xdeadbeef, &mut buf);
    assert_eq!(&buf[..n], b"SIGNAL sig=11 addr=0xdeadbeef\n");
}

#[test]
fn spec_signal_addr零出一个零() {
    let mut buf = [0u8; 128];
    let n = format_signal_line(10, 0, &mut buf);
    assert_eq!(&buf[..n], b"SIGNAL sig=10 addr=0x0\n");
}

#[test]
fn spec_signal_缓冲截断不越界() {
    let mut buf = [0u8; 8];
    let n = format_signal_line(11, 0xdeadbeef, &mut buf);
    assert_eq!(n, 8); // 写满即停,不许写穿
    assert_eq!(&buf, b"SIGNAL s");
}

#[test]
fn spec_signal_探针信号钉死非art认领() {
    // 装机实证:SIGUSR1 被 Android ART 认领(堆转储/GC),libsigchain
    // 截获后不下传用户 handler——探针必须钉在无人认领的 SIGURG 上
    assert_eq!(kfm_na::crash::PROBE_SIG, libc::SIGURG);
    assert_ne!(kfm_na::crash::PROBE_SIG, libc::SIGUSR1);
    assert_ne!(kfm_na::crash::PROBE_SIG, libc::SIGQUIT);
}

// ---- ②告警三规则 ----

#[test]
fn spec_alert_帧耗过线且新峰值才报() {
    let mut s = snap();
    s.draw_max_ms = ALERT_DRAW_MS; // 压线不报(规则是「超过」)
    let (msgs, st) = alert_check(&s, &AlertState::new(), 0);
    assert!(msgs.is_empty());

    s.draw_max_ms = ALERT_DRAW_MS + 50; // 过线 → 报
    let (msgs, st) = alert_check(&s, &st, 1);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("帧耗时新峰值"));

    let (msgs, _) = alert_check(&s, &st, 2); // 同峰值不重报
    assert!(msgs.is_empty());

    s.draw_max_ms = ALERT_DRAW_MS + 80; // 刷新峰值 → 再报
    let (msgs, _) = alert_check(&s, &st, 3);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn spec_alert_rss绝线与冷却() {
    let mut s = snap();
    s.rss_kb = ALERT_RSS_ABS_KB + 1024; // 越绝线 → 报
    let (msgs, st) = alert_check(&s, &AlertState::new(), 0);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("绝线"));

    // 冷却期内(10 分钟)再越线不重报
    let (msgs, _) = alert_check(&s, &st, 60_000);
    assert!(msgs.is_empty());

    // 冷却过后再越线 → 再报
    let (msgs, _) = alert_check(&s, &st, 600_001);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn spec_alert_rss窗口净涨() {
    let mut s = snap();
    let st0 = AlertState::new();
    let (_, st1) = alert_check(&s, &st0, 0); // 立基线 200MB
    assert_eq!(st1.rss_base, Some((0, s.rss_kb)));

    s.rss_kb += ALERT_RSS_GROW_KB; // 涨压线不报(规则是「超过」)
    let (msgs, st2) = alert_check(&s, &st1, 60_000);
    assert!(msgs.is_empty());

    s.rss_kb += 1024; // 净涨过线 → 报,基线重置
    let (msgs, st3) = alert_check(&s, &st2, 61_000);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("净涨"));
    assert_eq!(st3.rss_base, Some((61_000, s.rss_kb)));
}

#[test]
fn spec_alert_会话死亡窗口() {
    let mut s = snap();
    let (_, st1) = alert_check(&s, &AlertState::new(), 0); // 立基线 deaths=0

    s.session_deaths = ALERT_DEATHS_NEW - 1; // 新增 2 不报
    let (msgs, st2) = alert_check(&s, &st1, 60_000);
    assert!(msgs.is_empty());

    s.session_deaths = ALERT_DEATHS_NEW; // 新增 3 → 报
    let (msgs, st3) = alert_check(&s, &st2, 61_000);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("死亡"));

    // 冷却期内(5 分钟)再死 3 个不重报
    s.session_deaths = ALERT_DEATHS_NEW * 2;
    let (msgs, st4) = alert_check(&s, &st3, 120_000);
    assert!(msgs.is_empty());
    // 窗口过期基线重置:新增口径从报警点重算
    let (msgs, _) = alert_check(&s, &st4, 400_000);
    assert!(msgs.is_empty()); // 相对新基线新增 0,不报
}

// ---- ③水位环 ----

#[test]
fn spec_ring_满帽丢最旧() {
    let mut ring = VecDeque::new();
    for i in 0..kfm_na::gate::HISTORY_CAP {
        let mut s = snap();
        s.frames = i as u64;
        ring_push(&mut ring, s);
    }
    assert_eq!(ring.len(), kfm_na::gate::HISTORY_CAP);
    // 再压一张:最旧(frames=0)被顶走,最新在尾
    let mut extra = snap();
    extra.frames = 999;
    ring_push(&mut ring, extra);
    assert_eq!(ring.len(), kfm_na::gate::HISTORY_CAP);
    assert_eq!(ring.front().unwrap().frames, 1);
    assert_eq!(ring.back().unwrap().frames, 999);
}

#[test]
fn spec_history_行格式钉死与防除零() {
    let mut s = snap();
    s.uptime_ms = 30_000;
    s.frames = 100;
    s.draw_total_ms = 500; // 均值 5ms
    s.draw_max_ms = 20;
    assert_eq!(
        format_history_line(&s),
        "t=30000 fg=1 fr=100 pump=50 draw=5/20ms cpu=1234 rss=204800kb l=100 r=200 o=0 d=0 tch=0 act=local"
    );
    // 一帧没画:均值 0 不炸
    s.frames = 0;
    s.draw_total_ms = 0;
    let line = format_history_line(&s);
    assert!(line.contains("draw=0/20ms"));
    assert!(line.starts_with("t=30000 fg=1 fr=0 "));
}
