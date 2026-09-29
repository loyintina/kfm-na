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

// ---- BAR-174：信件本地缓存 + 增量加载（纯核钉）----
//
// 变异抽检：①reconcile 比对键摘 mtime（同 bytes 改 mtime 不重抓 = 内容
// 变了看不到）必须咬；②delete 漏做（幽灵信不删，列表减了正文还在）必须咬；
// ③stale 后缀摘掉（用户分不清缓存行/新鲜行）必须咬。

use kfm_na::sess_pool::{LetterMeta, SyncPlan};

fn lm(name: &str, bytes: u64, mtime: u64) -> LetterMeta {
    LetterMeta {
        name: name.to_string(),
        bytes,
        mtime,
    }
}

fn names(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn spec_bar174_parse_letter_list_容错() {
    // 新面：mtime 带上
    let body = r#"{"ok":true,"letters":[{"name":"a.md","bytes":900,"mtime":1727000000}]}"#;
    let ls = sess_pool::parse_letter_list(body).unwrap();
    assert_eq!(ls, vec![lm("a.md", 900, 1727000000)]);
    // 旧响应无 mtime 字段兼容为 0（= 全量重抓语义）
    let body = r#"{"ok":true,"letters":[{"name":"a.md","bytes":900}]}"#;
    let ls = sess_pool::parse_letter_list(body).unwrap();
    assert_eq!(ls, vec![lm("a.md", 900, 0)]);
    // 坏 JSON / ok:false / 缺字段 都是错不是吞
    assert!(sess_pool::parse_letter_list("不是 json").is_err());
    assert!(sess_pool::parse_letter_list(r#"{"ok":false,"error":"x"}"#).is_err());
    assert!(sess_pool::parse_letter_list(r#"{"ok":true}"#).is_err());
}

#[test]
fn spec_bar174_manifest_落盘串同端点形可回读() {
    let list = vec![lm("a.md", 900, 111), lm("b.md", 5, 222)];
    let text = sess_pool::manifest_json(&list);
    // 与端点同形（ok:true 防 ok:false 吞表闸咬自家缓存）且可一口径读回
    assert!(text.contains("\"ok\":true"), "manifest 带 ok:true: {text}");
    assert!(text.contains("\"mtime\":111"), "manifest 带 mtime: {text}");
    let back = sess_pool::parse_letter_list(&text).unwrap();
    assert_eq!(back, list, "写读回环");
}

#[test]
fn spec_bar174_reconcile_全新全抓() {
    let remote = vec![lm("a.md", 1, 10), lm("b.md", 2, 20)];
    let plan = sess_pool::reconcile(&[], &remote, &[]);
    assert_eq!(plan.list, remote, "list = 远端新清单");
    assert_eq!(names(&plan.fetch), vec!["a.md", "b.md"], "全新全抓");
    assert!(plan.delete.is_empty());
}

#[test]
fn spec_bar174_reconcile_全同零动作() {
    let cached = vec![lm("a.md", 1, 10), lm("b.md", 2, 20)];
    let have = vec!["a.md".to_string(), "b.md".to_string()];
    let plan = sess_pool::reconcile(&cached, &cached.clone(), &have);
    assert_eq!(
        plan,
        SyncPlan {
            list: cached,
            ..SyncPlan::default()
        },
        "全同零动作"
    );
}

#[test]
fn spec_bar174_reconcile_新信抓() {
    let cached = vec![lm("a.md", 1, 10)];
    let remote = vec![lm("a.md", 1, 10), lm("b.md", 2, 20)];
    let have = vec!["a.md".to_string()];
    let plan = sess_pool::reconcile(&cached, &remote, &have);
    assert_eq!(names(&plan.fetch), vec!["b.md"], "只抓新信");
    assert!(plan.delete.is_empty());
}

#[test]
fn spec_bar174_reconcile_消失删() {
    let cached = vec![lm("a.md", 1, 10), lm("ghost.md", 3, 30)];
    let remote = vec![lm("a.md", 1, 10)];
    let have = vec!["a.md".to_string(), "ghost.md".to_string()];
    let plan = sess_pool::reconcile(&cached, &remote, &have);
    assert_eq!(names(&plan.delete), vec!["ghost.md"], "幽灵信正文要删");
    assert!(plan.fetch.is_empty(), "留存的信不重抓");
    assert_eq!(plan.list, remote, "list 跟远端（幽灵出列）");
}

#[test]
fn spec_bar174_reconcile_mtime变重抓() {
    // 同 bytes 不同 mtime = 内容变了（比对键是 (bytes, mtime) 双元）
    let cached = vec![lm("a.md", 100, 10)];
    let remote = vec![lm("a.md", 100, 99)];
    let have = vec!["a.md".to_string()];
    let plan = sess_pool::reconcile(&cached, &remote, &have);
    assert_eq!(names(&plan.fetch), vec!["a.md"], "mtime 变必须重抓");
    // bytes 变同理
    let remote = vec![lm("a.md", 101, 10)];
    let plan = sess_pool::reconcile(&cached, &remote, &have);
    assert_eq!(names(&plan.fetch), vec!["a.md"], "bytes 变必须重抓");
}

#[test]
fn spec_bar174_reconcile_有清单缺正文补抓() {
    let cached = vec![lm("a.md", 1, 10), lm("b.md", 2, 20)];
    let have = vec!["a.md".to_string()]; // b.md 正文文件丢了
    let plan = sess_pool::reconcile(&cached, &cached.clone(), &have);
    assert_eq!(names(&plan.fetch), vec!["b.md"], "清单在正文缺 = 补抓");
}

#[test]
fn spec_bar174_stale值串形状() {
    let list = vec![lm("a.md", 512, 10), lm("b.md", 2048, 20)];
    let rows = sess_pool::stale_letter_entries(&list);
    // BAR-175 起展示序 = 按名降序（新上旧下），b.md 在 a.md 前
    assert_eq!(rows[0], ("b.md".to_string(), "2.0KB · 缓存".to_string()));
    assert_eq!(rows[1].1, "512B · 缓存", "fmt_bytes 后缀「 · 缓存」");
    assert_eq!(sess_pool::STALE_MARK, " · 缓存");
    // 新鲜行去标（同倒序）
    let rows = sess_pool::fresh_letter_entries(&list);
    assert_eq!(rows[0].1, "2.0KB");
}

#[test]
fn spec_bar175_信件条目_新上旧下() {
    // 用户拍板「最新的信在最上面」：v2.1/存量信名 NNNN 零填充开头，
    // 字典序降序 = 编号降序；输入乱序也必须出新上旧下
    let list = vec![
        lm("0007号甲致乙的通报.md", 100, 70),
        lm("0001-kfm-na-legacy-report.md", 100, 10),
        lm("0022号甲致乙的提案.md", 100, 220),
        lm("0009号甲致乙复0001的回信.md", 100, 90),
    ];
    let order = |rows: Vec<(String, String)>| rows.into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    let fresh = order(sess_pool::fresh_letter_entries(&list));
    assert_eq!(
        fresh,
        vec![
            "0022号甲致乙的提案.md",
            "0009号甲致乙复0001的回信.md",
            "0007号甲致乙的通报.md",
            "0001-kfm-na-legacy-report.md",
        ],
        "新鲜行新上旧下（v2.1 与存量混排同律）"
    );
    let stale = order(sess_pool::stale_letter_entries(&list));
    assert_eq!(
        stale.first().map(String::as_str),
        Some("0022号甲致乙的提案.md"),
        "stale 缓存行同倒序"
    );
    assert_eq!(stale.len(), 4);
}
