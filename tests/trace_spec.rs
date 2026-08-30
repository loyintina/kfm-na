//! trace/stats 考题（A 档）——自观测第二块：行踪环 + 运行时统计
//!
//! 契约（2026-08-26 与用户定）：
//! ①环满帽推新挤旧，尾部顺序 = 时间顺序；
//! ②行格式钉死 `[+00012345ms stage] msg`；
//! ③两类周期心跳（alive / loop 事件循环心跳）永不入环——它们是活性
//!   探针不是事件，入环只会稀释信号；
//! ④stats 格式化 = key=value 一行一项，机器可读。

use kfm_na::gate::{StatsSnap, format_stats, parse_self_stat_jiffies, parse_vmrss_kb};
use kfm_na::trace::{TraceEntry, TraceRing, should_trace};

fn e(ms: u128, stage: &str, msg: &str) -> TraceEntry {
    TraceEntry {
        boot_ms: ms,
        stage: stage.into(),
        msg: msg.into(),
    }
}

#[test]
fn spec_trace_满帽挤旧_尾部保序() {
    let mut r = TraceRing::new(3);
    for i in 0..5 {
        r.push(e(i, "t", &format!("m{i}")));
    }
    assert_eq!(r.len(), 3, "超帽必须钉在帽上");
    let tail = r.tail(3);
    assert_eq!(tail[0].msg, "m2", "最旧的 m0/m1 已被挤掉");
    assert_eq!(tail[2].msg, "m4", "最新一条在尾部");
    // tail(超存量) = 全量
    assert_eq!(r.tail(99).len(), 3);
}

#[test]
fn spec_trace_行格式钉死() {
    let out = TraceRing::format_entries(&[e(12345, "boot", "android_main 进入")]);
    assert_eq!(out, "[+00012345ms boot] android_main 进入\n");
}

#[test]
fn spec_trace_心跳过滤() {
    assert!(!should_trace("alive", "心跳 42"), "alive 心跳不入环");
    assert!(
        !should_trace("loop", "事件循环心跳 jni(commit=0/0 key=0 log=0)"),
        "loop 周期戳不入环"
    );
    assert!(
        should_trace("loop", "unix=1 STALL beat_age=9ms(前台)"),
        "loop 迁移档是事件,入环"
    );
    assert!(should_trace("death", "run_app 返回"), "真事件入环");
    assert!(should_trace("boot", "android_main 进入"), "真事件入环");
}

#[test]
fn spec_stats_格式_keyvalue一行一项() {
    let s = StatsSnap {
        uptime_ms: 61000,
        foreground: true,
        loop_age_ms: Some(42),
        frames: 1000,
        pump_calls: 200,
        pump_bytes: 9999,
        shots: 3,
        texts: 5,
        keys: 7,
        keys_bytes: 128,
        touches: 4,
        active: "local".into(),
        sessions: "local,remote".into(),
        draw_total_ms: 5000,
        draw_max_ms: 120,
        cpu_jiffies: 3210,
        rss_kb: 45678,
        bytes_local: 111,
        bytes_remote: 222,
        bytes_other: 3,
        session_deaths: 2,
        ai_page: "terminal".into(),
        ai_running: false,
        ai_orb_x: 0,
        ai_orb_y: 0,
        ai_pressed: false,
        ai_overlay: false,
    };
    let out = format_stats(&s);
    assert!(out.contains("uptime=61000ms\n"));
    assert!(out.contains("foreground=true\n"));
    assert!(out.contains("loop_beat_age=42ms\n"));
    assert!(out.contains("pump_bytes=9999\n"));
    assert!(out.contains("sessions=local,remote\n"));
    // 资源画像段(自观测第三块):draw_avg = total/frames = 5000/1000 = 5
    assert!(out.contains("draw_avg_ms=5\n"));
    assert!(out.contains("draw_max_ms=120\n"));
    assert!(out.contains("cpu_jiffies=3210\n"));
    assert!(out.contains("rss_kb=45678\n"));
    assert!(out.contains("bytes_local=111\n"));
    assert!(out.contains("bytes_remote=222\n"));
    assert!(out.contains("bytes_other=3\n"));
    assert!(out.contains("session_deaths=2\n"));
    assert!(out.contains("touches=4\n"));
    assert!(out.ends_with('\n'));
    // 未起跳的龄期要人话,不是数字
    let mut s2 = s.clone();
    s2.loop_age_ms = None;
    assert!(format_stats(&s2).contains("loop_beat_age=未起跳\n"));
    // 一帧没画过:均耗防除零报 0
    let mut s3 = s.clone();
    s3.frames = 0;
    s3.draw_total_ms = 0;
    assert!(format_stats(&s3).contains("draw_avg_ms=0\n"));
}

// ---- 自观测第三块:/proc 解析器考题 ----

#[test]
fn spec_proc_stat_jiffies_带括号comm照常切对() {
    // 真实样本形态:comm 字段可含空格与括号,必须从最后一个 ')' 后切。
    // ')' 后第 12/13 项(原序号 14/15)= utime/stime
    let stat = "1234 (weird ) name) S 1 2 3 4 5 6 7 8 9 10 100 25 0 0 20 0 1 0 5 123456 789 0 0 0";
    assert_eq!(parse_self_stat_jiffies(stat), Some(125)); // 100 + 25
    // 常规形态
    let stat2 = "1 (init) S 0 1 1 0 -1 4194560 100 2000 50 0 7 3 0 0 20 0 1 0 1 100 200 10";
    assert_eq!(parse_self_stat_jiffies(stat2), Some(10)); // 7 + 3
    // 残缺行 → None,不许 panic
    assert_eq!(parse_self_stat_jiffies("garbage"), None);
    assert_eq!(parse_self_stat_jiffies("1 (init) S 0 1"), None);
}

#[test]
fn spec_proc_status_vmrss() {
    let status = "Name:\tkfm-na\nState:\tR (running)\nVmRSS:\t   45678 kB\nThreads:\t9\n";
    assert_eq!(parse_vmrss_kb(status), Some(45678));
    assert_eq!(parse_vmrss_kb("Name:\tx\n"), None);
}
