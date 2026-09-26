//! sess_pool 考题（A 档，BAR-163 会话池一期）：agentd JSON 面解析、
//! 下池路由表、上池条目表、空态占位、focus 钳制、字节数格式化。
//!
//! 变异抽检：①摘空态占位（空线路 = 上池一片白，用户以为坏了）必须咬；
//! ②ok:false 的报错面被当成成功解析（错误详情变成条目名）必须咬；
//! ③信箱特殊路由丢失（信件没入口）必须咬。

use kfm_na::sess_pool::{self, EMPTY_LETTERS, EMPTY_SESSIONS, MAILBOX_TITLE, RouteKey};

#[test]
fn spec_bar163_池页_路由表_线加信箱特殊路由() {
    let lines = vec!["demo".to_string(), "x2".to_string()];
    let rows = sess_pool::routes_of(&lines);
    assert_eq!(rows.len(), 3, "两线 + 信箱: {rows:?}");
    assert_eq!(
        rows[0],
        (RouteKey::Line("demo".into()), "demo".into(), "线".into())
    );
    assert_eq!(
        rows[1],
        (RouteKey::Line("x2".into()), "x2".into(), "线".into())
    );
    assert_eq!(
        rows[2],
        (
            RouteKey::Mailbox,
            MAILBOX_TITLE.to_string(),
            "信件".to_string()
        )
    );
    // 空线表也有信箱（信件永远有入口）
    let rows = sess_pool::routes_of(&[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, RouteKey::Mailbox);
}

#[test]
fn spec_bar163_池页_条目表_字节数格式化() {
    let ss = vec![
        ("0001-会话.jsonl".to_string(), 512u64),
        ("0002-长谈.jsonl".to_string(), 2048u64),
        ("0003-巨卷.jsonl".to_string(), 3 * 1024 * 1024u64),
    ];
    let rows = sess_pool::session_entries(&ss);
    assert_eq!(rows[0], ("0001-会话.jsonl".to_string(), "512B".to_string()));
    assert_eq!(
        rows[1],
        ("0002-长谈.jsonl".to_string(), "2.0KB".to_string())
    );
    assert_eq!(
        rows[2],
        ("0003-巨卷.jsonl".to_string(), "3.0MB".to_string())
    );
}

#[test]
fn spec_bar163_池页_空态占位行() {
    let rows = sess_pool::entries_or_placeholder(vec![], EMPTY_SESSIONS);
    assert_eq!(rows, vec![(EMPTY_SESSIONS.to_string(), String::new())]);
    let rows = sess_pool::entries_or_placeholder(vec![], EMPTY_LETTERS);
    assert_eq!(rows, vec![(EMPTY_LETTERS.to_string(), String::new())]);
    // 非空不占位
    let rows = sess_pool::entries_or_placeholder(
        vec![("a".to_string(), "1B".to_string())],
        EMPTY_SESSIONS,
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "a");
}

#[test]
fn spec_bar163_池页_focus钳制() {
    assert_eq!(sess_pool::clamp_focus(0, 0), 0);
    assert_eq!(sess_pool::clamp_focus(5, 0), 0);
    assert_eq!(sess_pool::clamp_focus(1, 3), 1);
    assert_eq!(sess_pool::clamp_focus(9, 3), 2);
}

#[test]
fn spec_bar163_解析_lines面() {
    let body = r#"{"ok":true,"lines":["demo","x2"]}"#;
    assert_eq!(
        sess_pool::parse_lines(body).unwrap(),
        vec!["demo".to_string(), "x2".to_string()]
    );
    assert!(sess_pool::parse_lines("不是 json").is_err());
    assert!(sess_pool::parse_lines(r#"{"ok":true}"#).is_err());
    let err = sess_pool::parse_lines(r#"{"ok":false,"error":"面坏了"}"#).unwrap_err();
    assert!(err.contains("面坏了"), "错误详情透传: {err}");
}

#[test]
fn spec_bar163_解析_sessions与letters同形状() {
    let body = r#"{"ok":true,"sessions":[{"name":"0001-会话.jsonl","bytes":2486}]}"#;
    let ss = sess_pool::parse_named_bytes(body, "sessions").unwrap();
    assert_eq!(ss, vec![("0001-会话.jsonl".to_string(), 2486u64)]);
    let body = r#"{"ok":true,"letters":[{"name":"a-b-report.md","bytes":900}]}"#;
    let ls = sess_pool::parse_named_bytes(body, "letters").unwrap();
    assert_eq!(ls, vec![("a-b-report.md".to_string(), 900u64)]);
    // 错键 = 错
    assert!(sess_pool::parse_named_bytes(body, "sessions").is_err());
    // ok:false 不许当空表吞掉
    assert!(sess_pool::parse_named_bytes(r#"{"ok":false,"error":"x"}"#, "sessions").is_err());
}

#[test]
fn spec_bar163_解析_tail与letter面() {
    let body = r#"{"ok":true,"events":["{\"type\":\"done\"}","{\"type\":\"usage\"}"]}"#;
    let evs = sess_pool::parse_events(body).unwrap();
    assert_eq!(evs.len(), 2);
    assert!(evs[0].contains("done"));
    let body = r##"{"ok":true,"name":"a.md","content":"# 信\n正文"}"##;
    assert_eq!(sess_pool::parse_letter_content(body).unwrap(), "# 信\n正文");
    assert!(sess_pool::parse_letter_content(r#"{"ok":true,"name":"a.md"}"#).is_err());
}
