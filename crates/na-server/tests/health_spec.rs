//! crates/na-server/tests/health_spec.rs — A 档考题：health 面形状
//!
//! 答案区：crates/na-server/src/state.rs build_health。本文件是考题，不许改。
//!
//! 2026-09-20 修宪（用户拍板「真空闲」）：idle_s 语义从「会话年龄」
//! （now - opened_epoch_s）改为「距最后活动」（now - last_active_epoch_s）
//! ——名不副实的字段被服务卡显形后修约。活动 = input 输入 / output 产出
//! （wsterm 接线 registry.touch）。

use na_server::state::{Registry, SessionRec, build_health};

fn now_epoch_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rec(id: &str) -> SessionRec {
    SessionRec {
        id: id.into(),
        cmd: "tmux attach".into(),
        cols: 80,
        rows: 24,
        opened_epoch_s: 1,
        last_active_epoch_s: 1,
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
fn spec_health_idle_是真空闲() {
    // 开了一年但 5 秒前还在活动 → idle ≈ 5，不是年龄
    let mut r = rec("s1");
    r.opened_epoch_s = 1;
    r.last_active_epoch_s = now_epoch_s() - 5;
    let v = build_health(0, 0, &[r]);
    let idle = v["sessions"][0]["idle_s"].as_u64().expect("idle_s 数字");
    assert!(
        (4..=8).contains(&idle),
        "idle_s = 距最后活动（期望 ~5s），不是会话年龄；实得 {idle}"
    );
}

#[test]
fn spec_registry_touch_刷新活动() {
    // touch 接线钉：注册（活动=开时）→ touch → idle 回零档
    let reg = Registry::new();
    let mut r = rec("s1");
    r.opened_epoch_s = now_epoch_s() - 600;
    r.last_active_epoch_s = now_epoch_s() - 600;
    reg.register(r);
    reg.touch("s1");
    let v: serde_json::Value =
        serde_json::from_str(&reg.health_json()).expect("health 是合法 JSON");
    let idle = v["sessions"][0]["idle_s"].as_u64().expect("idle_s 数字");
    assert!(idle <= 2, "touch 后 idle 必须回零档；实得 {idle}");
    // 没 touch 的会话 idle 保持年龄档
    let mut r2 = rec("s2");
    r2.opened_epoch_s = now_epoch_s() - 600;
    r2.last_active_epoch_s = now_epoch_s() - 600;
    reg.register(r2);
    let v: serde_json::Value =
        serde_json::from_str(&reg.health_json()).expect("health 是合法 JSON");
    let idle2 = v["sessions"]
        .as_array()
        .expect("数组")
        .iter()
        .find(|s| s["id"] == "s2")
        .expect("s2 在册")["idle_s"]
        .as_u64()
        .expect("idle_s 数字");
    assert!(idle2 >= 599, "未 touch 的会话 idle 必须保真；实得 {idle2}");
}

#[test]
fn spec_health_multiple_sessions() {
    let v = build_health(0, 0, &[rec("s1"), rec("s2"), rec("s3")]);
    assert_eq!(v["sessions"].as_array().expect("数组").len(), 3);
}
