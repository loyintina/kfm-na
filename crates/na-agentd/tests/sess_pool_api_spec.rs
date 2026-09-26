//! crates/na-agentd/tests/sess_pool_api_spec.rs — BAR-163 钉②：agentd
//! 新端点契约（会话列表/信件列表/信件正文的形状与 404 语义）。
//!
//! 答案区：crates/na-agentd/src/{httpd,service}.rs。考题不许改。
//! 服务层直接吃临时目录（AgentService 文件系统制，host 可判卷）。

use na_agentd::httpd;
use na_agentd::service::AgentService;

fn fixture() -> (tempfile::TempDir, AgentService) {
    let tmp = tempfile::tempdir().expect("临时目录");
    let root = tmp.path().join("session");
    // demo 线两个会话 + 杂件
    let demo = root.join("demo");
    std::fs::create_dir_all(&demo).expect("建线");
    std::fs::write(
        demo.join("0001-会话.jsonl"),
        "{\"type\":\"user_msg\"}\n{\"type\":\"done\"}\n",
    )
    .expect("写会话");
    std::fs::write(demo.join("0002-会话.jsonl"), "{\"type\":\"user_msg\"}\n").expect("写会话");
    std::fs::write(demo.join("line.toml"), "provider = \"x\"\n").expect("写杂件");
    // 信箱一封信 + README + 非 md 杂件
    let mb = root.join("信箱");
    std::fs::create_dir_all(&mb).expect("建信箱");
    std::fs::write(mb.join("README.md"), "# 规范\n").expect("写 README");
    std::fs::write(mb.join("a-b-report.md"), "# 信\n正文\n").expect("写信");
    std::fs::write(mb.join("notes.txt"), "杂\n").expect("写杂件");
    let svc = AgentService::new(
        &root.to_string_lossy(),
        &tmp.path().join("无provider.json").to_string_lossy(),
    );
    (tmp, svc)
}

// ---- 路由面 ----

#[test]
fn spec_bar163_端点_路由四面() {
    match httpd::route("GET", "/api/agent/lines/demo/sessions") {
        httpd::Route::Sessions { line } => assert_eq!(line, "demo"),
        _ => panic!("sessions 路由"),
    }
    match httpd::route(
        "GET",
        "/api/agent/lines/demo/sessions/0001-%E4%BC%9A%E8%AF%9D.jsonl/tail?n=3",
    ) {
        httpd::Route::SessionTail { line, name, n } => {
            assert_eq!(line, "demo");
            assert_eq!(name, "0001-%E4%BC%9A%E8%AF%9D.jsonl");
            assert_eq!(n, 3);
        }
        _ => panic!("session tail 路由"),
    }
    assert!(matches!(
        httpd::route("GET", "/api/agent/mailbox/letters"),
        httpd::Route::Letters
    ));
    match httpd::route("GET", "/api/agent/mailbox/letters/a-b-report.md") {
        httpd::Route::Letter { name } => assert_eq!(name, "a-b-report.md"),
        _ => panic!("letter 路由"),
    }
    // 方法与形状错 = 404
    assert!(matches!(
        httpd::route("POST", "/api/agent/mailbox/letters"),
        httpd::Route::NotFound
    ));
    assert!(matches!(
        httpd::route("GET", "/api/agent/lines/demo/sessions/x"),
        httpd::Route::NotFound
    ));
}

// ---- 数据面形状 ----

#[test]
fn spec_bar163_端点_会话列表形状() {
    let (_t, svc) = fixture();
    let ss = svc.list_sessions("demo").expect("列出");
    assert_eq!(ss.len(), 2, "line.toml 等杂件不算会话");
    assert_eq!(ss[0].0, "0001-会话.jsonl");
    assert_eq!(ss[1].0, "0002-会话.jsonl");
    assert!(ss[0].1 > 0, "字节数在");
    // 空线（目录在但无会话）= 空表不是错
    let root = format!("{}/empty", svc.session_root);
    std::fs::create_dir_all(&root).expect("建空线");
    assert!(svc.list_sessions("empty").expect("空线").is_empty());
}

#[test]
fn spec_bar163_端点_会话列表_404语义() {
    let (_t, svc) = fixture();
    let e = svc.list_sessions("无线").unwrap_err();
    assert!(e.contains("非法"), "非 ASCII 线名 = 400 语义: {e}");
    let e = svc.list_sessions("nosuch").unwrap_err();
    assert!(e.contains("不存在"), "不存在的线 = 404 语义: {e}");
}

#[test]
fn spec_bar163_端点_点名会话tail() {
    let (_t, svc) = fixture();
    let ev = svc
        .tail_session("demo", "0001-会话.jsonl", 10)
        .expect("tail");
    assert_eq!(ev.len(), 2);
    // n 截断：只要尾部 1 行
    let ev = svc
        .tail_session("demo", "0001-会话.jsonl", 1)
        .expect("tail");
    assert_eq!(ev.len(), 1);
    assert!(ev[0].contains("done"), "尾部行: {}", ev[0]);
    // 404/400 语义
    assert!(
        svc.tail_session("demo", "9999-会话.jsonl", 1)
            .unwrap_err()
            .contains("不存在")
    );
    assert!(
        svc.tail_session("demo", "../escape", 1)
            .unwrap_err()
            .contains("非法")
    );
    assert!(
        svc.tail_session("demo", "line.toml", 1)
            .unwrap_err()
            .contains("非法")
    );
}

#[test]
fn spec_bar163_端点_信件列表与正文() {
    let (_t, svc) = fixture();
    let ls = svc.list_letters().expect("列出");
    assert_eq!(ls.len(), 1, "README.md 与 *.txt 不算信");
    assert_eq!(ls[0].0, "a-b-report.md");
    assert!(ls[0].1 > 0);
    let content = svc.letter("a-b-report.md").expect("正文");
    assert!(content.contains("# 信"));
    assert!(content.contains("正文"));
    // 404/400 语义
    assert!(svc.letter("no-such.md").unwrap_err().contains("不存在"));
    assert!(svc.letter("../x.md").unwrap_err().contains("非法"));
    assert!(svc.letter("中文名.md").unwrap_err().contains("非法"));
    assert!(svc.letter("README.md").unwrap_err().contains("非法"));
}
