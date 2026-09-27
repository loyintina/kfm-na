//! sess_pool 考题（A 档，BAR-163 会话池一期）：agentd JSON 面解析、
//! 下池路由表、上池条目表、空态占位、focus 钳制、字节数格式化。
//!
//! 变异抽检：①摘空态占位（空线路 = 上池一片白，用户以为坏了）必须咬；
//! ②ok:false 的报错面被当成成功解析（错误详情变成条目名）必须咬；
//! ③信箱特殊路由丢失（信件没入口）必须咬。

use kfm_na::sess_pool::{
    self, AGENT_INBOX_TITLE, EMPTY_LETTERS, EMPTY_SESSIONS, MAILBOX_TITLE, RouteKey,
};

#[test]
fn spec_bar163_池页_路由表_线加信箱特殊路由() {
    let lines = vec!["demo".to_string(), "x2".to_string()];
    let rows = sess_pool::routes_of(&lines);
    assert_eq!(
        rows.len(),
        4,
        "两线 + agent-inbox(BAR-167) + 信箱: {rows:?}"
    );
    assert_eq!(
        rows[0],
        (RouteKey::Line("demo".into()), "demo".into(), "线".into())
    );
    assert_eq!(
        rows[1],
        (RouteKey::Line("x2".into()), "x2".into(), "线".into())
    );
    assert_eq!(
        rows[2].0,
        RouteKey::AgentInbox,
        "BAR-167 固定行（详见 spec_bar167）"
    );
    assert_eq!(
        rows[3],
        (
            RouteKey::Mailbox,
            MAILBOX_TITLE.to_string(),
            "信件".to_string()
        )
    );
    // 空线表也有信箱（信件永远有入口）
    let rows = sess_pool::routes_of(&[]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].0, RouteKey::Mailbox);
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

// ---- BAR-167 工单③：全局信箱 agent-inbox 固定行进下池 ----

#[test]
fn spec_bar167_池页_路由表_agentinbox固定行() {
    let lines = vec!["demo".to_string(), "x2".to_string()];
    let rows = sess_pool::routes_of(&lines);
    assert_eq!(rows.len(), 4, "两线 + agent-inbox + 信箱: {rows:?}");
    assert_eq!(rows[0].0, RouteKey::Line("demo".into()));
    assert_eq!(rows[1].0, RouteKey::Line("x2".into()));
    // agent-inbox 在线表后、信箱前（位置钉死）
    assert_eq!(
        rows[2],
        (
            RouteKey::AgentInbox,
            AGENT_INBOX_TITLE.to_string(),
            "信件".to_string()
        )
    );
    assert_eq!(rows[3].0, RouteKey::Mailbox);
    // 空线表也有 agent-inbox（真信永远有入口）
    let rows = sess_pool::routes_of(&[]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, RouteKey::AgentInbox);
    assert_eq!(rows[1].0, RouteKey::Mailbox);
}

#[test]
fn spec_bar167_池页_inbox_api_key映射() {
    assert_eq!(
        sess_pool::inbox_api_key(&RouteKey::Mailbox),
        Some("mailbox")
    );
    assert_eq!(
        sess_pool::inbox_api_key(&RouteKey::AgentInbox),
        Some("agent-inbox")
    );
    assert_eq!(
        sess_pool::inbox_api_key(&RouteKey::Line("demo".into())),
        None
    );
}

#[test]
fn spec_bar167_池页_agentinbox条目与空态() {
    // 条目表：信件形状与信箱同面（label = 信名，value = 字节数）
    let ls = vec![
        (
            "0001-kfmv4-nz-report-readability-submission.md".to_string(),
            4096u64,
        ),
        (
            "0002-kfmv4-review-report-readability-response.md".to_string(),
            512u64,
        ),
    ];
    let rows = sess_pool::letter_entries(&ls);
    assert_eq!(rows[0].0, "0001-kfmv4-nz-report-readability-submission.md");
    assert_eq!(rows[0].1, "4.0KB");
    assert_eq!(rows[1].1, "512B");
    // 空态沿用信箱口径
    let rows = sess_pool::entries_or_placeholder(vec![], EMPTY_LETTERS);
    assert_eq!(rows, vec![(EMPTY_LETTERS.to_string(), String::new())]);
}

#[test]
fn spec_bar167_池页_content_title新变体() {
    assert_eq!(
        sess_pool::content_title(&RouteKey::AgentInbox, "0001-x.md"),
        format!("{AGENT_INBOX_TITLE}/0001-x.md")
    );
    // 旧两变体不动
    assert_eq!(
        sess_pool::content_title(&RouteKey::Mailbox, "a.md"),
        format!("{MAILBOX_TITLE}/a.md")
    );
    assert_eq!(
        sess_pool::content_title(&RouteKey::Line("demo".into()), "0001-会话.jsonl"),
        "demo/0001-会话.jsonl"
    );
}
