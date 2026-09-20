//! crates/na-server/tests/health_spec.rs — A 档考题：health 面形状
//!
//! 答案区：crates/na-server/src/state.rs build_health。本文件是考题，不许改。

use na_server::state::{SessionRec, build_health};

fn rec(id: &str) -> SessionRec {
    SessionRec {
        id: id.into(),
        cmd: "tmux attach".into(),
        cols: 80,
        rows: 24,
        opened_epoch_s: 1,
    }
}

#[test]
fn spec_health_empty() {
    let v = build_health(12, 1000, &[]);
    assert_eq!(v["uptime_s"], 12);
    assert_eq!(v["started_epoch_s"], 1000);
    assert_eq!(v["sessions"], serde_json::json!([]));
}

#[test]
fn spec_health_session_shape() {
    let v = build_health(0, 0, &[rec("s1")]);
    let s = &v["sessions"][0];
    assert_eq!(s["id"], "s1");
    assert_eq!(s["cmd"], "tmux attach");
    assert_eq!(s["cols"], 80);
    assert_eq!(s["rows"], 24);
    assert_eq!(s["alive"], true, "在册即活（死会话先出册再发 Exit）");
    assert!(s["idle_s"].as_u64().is_some(), "idle_s 必须是数字");
}

#[test]
fn spec_health_multiple_sessions() {
    let v = build_health(0, 0, &[rec("s1"), rec("s2"), rec("s3")]);
    assert_eq!(v["sessions"].as_array().expect("数组").len(), 3);
}
