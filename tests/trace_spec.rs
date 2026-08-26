//! trace/stats 考题（A 档）——自观测第二块：行踪环 + 运行时统计
//!
//! 契约（2026-08-26 与用户定）：
//! ①环满帽推新挤旧，尾部顺序 = 时间顺序；
//! ②行格式钉死 `[+00012345ms stage] msg`；
//! ③两类周期心跳（alive / loop 事件循环心跳）永不入环——它们是活性
//!   探针不是事件，入环只会稀释信号；
//! ④stats 格式化 = key=value 一行一项，机器可读。

use kfm_na::gate::{StatsSnap, format_stats};
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
        active: "local".into(),
        sessions: "local,remote".into(),
    };
    let out = format_stats(&s);
    assert!(out.contains("uptime=61000ms\n"));
    assert!(out.contains("foreground=true\n"));
    assert!(out.contains("loop_beat_age=42ms\n"));
    assert!(out.contains("pump_bytes=9999\n"));
    assert!(out.contains("sessions=local,remote\n"));
    assert!(out.ends_with('\n'));
    // 未起跳的龄期要人话,不是数字
    let mut s2 = s.clone();
    s2.loop_age_ms = None;
    assert!(format_stats(&s2).contains("loop_beat_age=未起跳\n"));
}
