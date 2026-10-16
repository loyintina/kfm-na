//! BAR-254 钉：agentd 列表端点预切四键形状（letter_json）＋名册加载退化。
//! 判据 = NA0183 楼3／楼4（白露 MAIN0173 楼6 三加固）：
//! 原文 from/to 不动；from_parts/to_parts 结构化（person 带 func、
//! 及式继承标 inherited、broadcast/unknown 原样 raw）；from_disp/to_disp
//! 显示串；roster 读不到 = 空池 = 逐项原样照抄不致死。

use na_agentd::service::{FloorMeta, LetterMeta, letter_json, load_roster};

const ROSTER_JSON: &str = r#"{
  "projects": ["na"],
  "functions": { "坤部一组": {}, "乾部一组": {}, "研究部": {}, "开发部": {}, "评审部": {} },
  "names": {
    "清和": { "functions": ["坤部一组"] },
    "观澜": { "functions": ["坤部一组"] },
    "闻灯": { "functions": ["坤部一组"] },
    "白露": { "functions": ["乾部一组"] }
  }
}"#;

fn roster() -> mailbox_core::roster::Roster {
    mailbox_core::roster::Roster::from_json_str(ROSTER_JSON).unwrap()
}

fn meta(from: &str, to: &str) -> LetterMeta {
    LetterMeta {
        name: "0001号x.md".to_string(),
        bytes: 1,
        mtime: 0,
        time: String::new(),
        from: from.to_string(),
        to: to.to_string(),
        title: String::new(),
        floors: Vec::<FloorMeta>::new(),
    }
}

#[test]
fn spec_bar254_预切四键形状() {
    let v = letter_json(
        &meta("坤部一组清和", "开发部闻灯及观澜、研究部全体、星际舰队张三"),
        Some(&roster()),
    );
    // 原文不动（加固1：双发）
    assert_eq!(v["from"], "坤部一组清和");
    assert_eq!(v["to"], "开发部闻灯及观澜、研究部全体、星际舰队张三");
    // from_parts 单项 person
    assert_eq!(
        v["from_parts"],
        serde_json::json!([{"kind": "person", "func": "坤部一组", "name": "清和"}])
    );
    assert_eq!(v["from_disp"], "清和");
    // to_parts：及式继承标 inherited；broadcast/unknown 原样 raw
    assert_eq!(
        v["to_parts"],
        serde_json::json!([
            {"kind": "person", "func": "开发部", "name": "闻灯"},
            {"kind": "person", "func": "开发部", "name": "观澜", "inherited": true},
            {"kind": "broadcast", "raw": "研究部全体"},
            {"kind": "unknown", "raw": "星际舰队张三"},
        ])
    );
    assert_eq!(v["to_disp"], "闻灯、观澜、研究部全体、星际舰队张三");
    // 增量字段旧三样与楼层表不破
    assert_eq!(v["name"], "0001号x.md");
    assert_eq!(v["floors"], serde_json::json!([]));
}

#[test]
fn spec_bar254_广播与unknown显示串() {
    let v = letter_json(&meta("评审部白露", "全体"), Some(&roster()));
    assert_eq!(
        v["to_parts"],
        serde_json::json!([{"kind": "broadcast", "raw": "全体"}])
    );
    assert_eq!(v["to_disp"], "全体");
}

#[test]
fn spec_bar254_空池逐项原样照抄() {
    let v = letter_json(&meta("坤部一组清和", "开发部闻灯及观澜"), None);
    assert_eq!(
        v["from_parts"],
        serde_json::json!([{"kind": "unknown", "raw": "坤部一组清和"}])
    );
    assert_eq!(v["from_disp"], "坤部一组清和");
    assert_eq!(
        v["to_parts"],
        serde_json::json!([
            {"kind": "unknown", "raw": "开发部闻灯"},
            {"kind": "unknown", "raw": "观澜"},
        ])
    );
}

#[test]
fn spec_bar254_load_roster退化() {
    // 读不到 = None（空池语义），不 panic
    assert!(load_roster("/nonexistent-mail-root").is_none());
    // 读得到且同构 = Some
    let dir = std::env::temp_dir().join(format!("bar254-roster-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("00-主册")).unwrap();
    std::fs::write(dir.join("00-主册/roster.json"), ROSTER_JSON).unwrap();
    let r = load_roster(dir.to_str().unwrap()).expect("夹具名册可读");
    assert!(r.functions.iter().any(|f| f == "坤部一组"));
    std::fs::remove_dir_all(&dir).ok();
}
