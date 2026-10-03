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
            // 百分号解码（live 实咬补：curl 把非 ASCII 段编成 %XX——
            // 不解码 = 标准客户端全 404）
            assert_eq!(name, "0001-会话.jsonl");
            assert_eq!(n, 3);
        }
        _ => panic!("session tail 路由"),
    }
    // %2F 解码成 '/' 也只留在段内（不裂成两段吃 404），下游闸照拒
    match httpd::route("GET", "/api/agent/mailbox/letters/a%2Fb.md") {
        httpd::Route::Letter { name } => assert_eq!(name, "a/b.md"),
        _ => panic!("%2F 必须留在段内"),
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
    assert_eq!(ls[0].name, "a-b-report.md");
    assert!(ls[0].bytes > 0);
    let content = svc.letter("a-b-report.md").expect("正文");
    assert!(content.contains("# 信"));
    assert!(content.contains("正文"));
    // 404/400 语义
    assert!(svc.letter("no-such.md").unwrap_err().contains("不存在"));
    assert!(svc.letter("../x.md").unwrap_err().contains("非法"));
    assert!(svc.letter("中文名.md").unwrap_err().contains("非法"));
    assert!(svc.letter("README.md").unwrap_err().contains("非法"));
}

// ---- BAR-167 工单③：全局信箱 agent-inbox 进会话池（key→根映射，fail-closed）----

#[test]
fn spec_bar167_端点_inboxes路由面() {
    match httpd::route("GET", "/api/agent/inboxes/agent-inbox/letters") {
        httpd::Route::InboxLetters { key } => assert_eq!(key, "agent-inbox"),
        _ => panic!("inbox letters 路由"),
    }
    match httpd::route("GET", "/api/agent/inboxes/mailbox/letters/a-b-report.md") {
        httpd::Route::InboxLetter { key, name } => {
            assert_eq!(key, "mailbox");
            assert_eq!(name, "a-b-report.md");
        }
        _ => panic!("inbox letter 路由"),
    }
    // key 段百分号解码照走（%2F 只留段内，下游映射表照拒）
    match httpd::route("GET", "/api/agent/inboxes/a%2Fb/letters") {
        httpd::Route::InboxLetters { key } => assert_eq!(key, "a/b"),
        _ => panic!("inbox key 百分号解码"),
    }
    // 形状错 = 404
    assert!(matches!(
        httpd::route("POST", "/api/agent/inboxes/agent-inbox/letters"),
        httpd::Route::NotFound
    ));
    assert!(matches!(
        httpd::route("GET", "/api/agent/inboxes"),
        httpd::Route::NotFound
    ));
}

#[test]
fn spec_bar167_端点_inbox映射表failclosed() {
    let (_t, svc) = fixture();
    assert!(svc.inbox_root("mailbox").is_some(), "旧 key 保留");
    assert_eq!(
        svc.inbox_root("agent-inbox"),
        Some(na_agentd::service::agent_inbox_root()),
        "全局评审信箱根"
    );
    // 不在表里的 key 一律 None（fail-closed，不开任意路径口）
    assert!(svc.inbox_root("etc").is_none());
    assert!(svc.inbox_root("../../etc").is_none());
    assert!(svc.inbox_root("").is_none());
    // unknown key → 404 语义（错误串不含「非法」，路由层归 404）
    let e = svc.list_inbox_letters("etc").unwrap_err();
    assert!(e.contains("未知"), "unknown key = 404 语义: {e}");
    let e = svc.inbox_letter("etc", "a.md").unwrap_err();
    assert!(e.contains("未知"), "unknown key = 404 语义: {e}");
}

#[test]
fn spec_bar167_端点_keyed信箱夹具形状() {
    let (_t, svc) = fixture();
    // mailbox key 走同一映射 = 旧面同形状
    let ls = svc.list_inbox_letters("mailbox").expect("列出");
    assert_eq!(ls.len(), 1, "README.md 与 *.txt 不算信");
    assert_eq!(ls[0].name, "a-b-report.md");
    let content = svc.inbox_letter("mailbox", "a-b-report.md").expect("正文");
    assert!(content.contains("# 信"));
    // README.md 点名正文照拒（400 语义）
    let e = svc.inbox_letter("mailbox", "README.md").unwrap_err();
    assert!(e.contains("非法"), "README 闸: {e}");
    // 分隔符/穿越照拒（%2F 解码出的 '/' 也死在闸上）
    let e = svc.inbox_letter("mailbox", "a/b.md").unwrap_err();
    assert!(e.contains("非法"), "分隔符闸: {e}");
    let e = svc.inbox_letter("mailbox", "../x.md").unwrap_err();
    assert!(e.contains("非法"), "穿越闸: {e}");
}

#[test]
fn spec_bar167_端点_agentinbox真根() {
    let (_t, svc) = fixture();
    // 真根直读（kfmv4 只读引用）。断言对主册迁移鲁棒（BAR-172）：不钉具体文件名——
    // 主册 2026-09-28 已整体迁移 v2.1 中文名，钉死文件名 = 下次主册变动又红
    let ls = svc.list_inbox_letters("agent-inbox").expect("列真信");
    assert!(!ls.is_empty(), "真信量级: {} 封", ls.len());
    assert!(
        ls.iter()
            .all(|l| { na_agentd::service::valid_letter_name(&l.name) && l.bytes > 0 }),
        "全表过信件闸且非空: {ls:?}"
    );
    assert!(
        ls.iter().any(|l| l.name.contains('号')),
        "主册现行含 v2.1 中文句法信（闸门须放行）: {ls:?}"
    );
    assert!(
        !ls.iter().any(|l| l.name == "README.md"),
        "README.md 是规范不是信"
    );
    // 正文端点回真文（取列表首封，不挑名）
    let content = svc
        .inbox_letter("agent-inbox", &ls[0].name)
        .expect("真信正文");
    assert!(content.len() > 100, "真文非空: {} 字节", content.len());
    // README.md 点名正文被拒
    let e = svc.inbox_letter("agent-inbox", "README.md").unwrap_err();
    assert!(e.contains("非法"), "README 闸: {e}");
}

#[test]
fn spec_bar172_信件名闸门_v21中文句法放行() {
    use na_agentd::service::valid_letter_name as v;
    // v2.1 中文句法名放行（主册现行真名形态；含分拣码/复/关于/9 词表类型词）
    assert!(v("0001号小满致全体关于报告可读性契约的提案.md"));
    assert!(v("0022号小满致评审部白露复0016关于名字征集的勘误.md"));
    assert!(v("NA0024号白露致研究部清和及小满的通报.md"));
    // 旧 ASCII 名照旧放行
    assert!(v("0001-kfmv4-nz-report-readability-submission.md"));
    assert!(v("a-b-report.md"));
    // 非法名照拒（fail-closed 不变）
    assert!(!v("README.md"));
    assert!(!v("a/b.md"), "分隔符");
    assert!(!v("../x.md"), "穿越");
    assert!(!v("随便写的.md"), "无编号段");
    assert!(!v("0001号白致评审部白露的提案.md"), "发信人单字过不了文法");
    assert!(!v("0001号白露致全体的落地.md"), "类型词出 9 词表");
    assert!(!v("0001号白露致全体的提案.txt"), "非 .md");
    assert!(!v("0001号白露致全体关于名的提案.md"), "事由单字过不了文法");

    // 端点面：中文名信进夹具信箱，列表出、点名正文取回
    let (t, svc) = fixture();
    let mb = t.path().join("session").join("信箱");
    std::fs::write(mb.join("0001号测试致全体的通报.md"), "# 中文信\n正文\n").expect("写中文信");
    let ls = svc.list_inbox_letters("mailbox").expect("列夹具信");
    assert!(
        ls.iter().any(|l| l.name == "0001号测试致全体的通报.md"),
        "中文名信应被列出: {ls:?}"
    );
    let content = svc
        .inbox_letter("mailbox", "0001号测试致全体的通报.md")
        .expect("中文名点名取正文");
    assert!(content.contains("中文信"));
}

// ---- BAR-174：信件列表带 mtime（app 本地缓存增量同步的比对键）----

#[test]
fn spec_bar174_端点_信件列表带mtime() {
    let (_t, svc) = fixture();
    let ls = svc.list_inbox_letters("mailbox").expect("列出");
    assert_eq!(ls.len(), 1, "夹具一封信: {ls:?}");
    let letter = &ls[0];
    assert_eq!(letter.name, "a-b-report.md");
    assert!(letter.mtime > 0, "mtime 必须带（增量比对键）: {letter:?}");
    // bytes 与真实文件长度一致（BAR-163 语义不变）
    let real = std::fs::metadata(format!("{}/信箱/a-b-report.md", svc.session_root))
        .expect("真实文件元数据");
    assert_eq!(letter.bytes, real.len(), "bytes 语义不变");
    // mtime 与真实文件 mtime 一致（秒级）
    let real_mtime = real
        .modified()
        .expect("mtime")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix 秒")
        .as_secs();
    assert_eq!(letter.mtime, real_mtime);
    // 旧别名 list_letters 同形同数据
    let old = svc.list_letters().expect("旧别名");
    assert_eq!(old, ls, "旧别名与 keyed 面同形");
}

// ---- BAR-212：两册新 key + 信头四字段 + summaries 批量端点 ----

/// v2.1 真格式信（照 0090 号实拍样例缩水）
const LETTER_FULL: &str = "# 已读，随时可退：无在途写盘

> 日期: 2026-09-30 14:59 +08:00
> 从: 开发部闻灯
> 致: 评审部白露
> 复: 0050
> 状态: 通报完毕
<!-- LETTER-TOKEN v2 no=0090 nonce=c7ec8a064bf7f278 fp=52e759294e2ef921 -->

## 摘要

> 注意（写给隐藏读者）：三句话内说清是什么事、要不要我做事；不写工作术语。

搬会话的信我读完了，我这边随时可以退。
<!-- LETTER-TOKEN 摘要段内注释行也丢 -->
要你做的事：没有。

## 正文

就绪申报。
";

/// 缺字头的信（只有 H1，无引用块字头，无摘要段）
const LETTER_BARE: &str = "# 光秃秃的信

正文一句话。
";

fn book_fixture() -> (tempfile::TempDir, AgentService) {
    let tmp = tempfile::tempdir().expect("临时目录");
    let mail = tmp.path().join("mail");
    let main_book = mail.join("00-主册");
    let na_book = mail.join("10-NA信箱");
    std::fs::create_dir_all(&main_book).expect("建主册");
    std::fs::create_dir_all(&na_book).expect("建NA信箱");
    std::fs::write(na_book.join("README.md"), "# 规范\n").expect("写 README");
    std::fs::write(
        na_book.join("0090号闻灯致评审部白露复0050的回执.md"),
        LETTER_FULL,
    )
    .expect("写真格式信");
    std::fs::write(na_book.join("bare-letter.md"), LETTER_BARE).expect("写秃信");
    std::fs::write(
        main_book.join("0090号闻灯致评审部白露复0050的回执.md"),
        LETTER_FULL,
    )
    .expect("写主册信");
    let mut svc = AgentService::new(
        &tmp.path().join("session").to_string_lossy(),
        &tmp.path().join("无provider.json").to_string_lossy(),
    );
    svc.mail_root = mail.to_string_lossy().into_owned();
    (tmp, svc)
}

#[test]
fn spec_bar212_端点_两册新key映射() {
    let (_t, svc) = book_fixture();
    assert_eq!(
        svc.inbox_root("main-book").as_deref(),
        Some(format!("{}/00-主册", svc.mail_root)).as_deref(),
        "main-book → 00-主册"
    );
    assert_eq!(
        svc.inbox_root("na-book").as_deref(),
        Some(format!("{}/10-NA信箱", svc.mail_root)).as_deref(),
        "na-book → 10-NA信箱"
    );
    // 旧两 key 不动
    assert!(svc.inbox_root("mailbox").is_some());
    assert_eq!(
        svc.inbox_root("agent-inbox"),
        Some(na_agentd::service::agent_inbox_root())
    );
    // fail-closed 不变：不在表里的 key 一律 None → 404 语义
    assert!(svc.inbox_root("etc").is_none());
    assert!(svc.inbox_root("../../etc").is_none());
    let e = svc.list_inbox_letters("etc").unwrap_err();
    assert!(e.contains("未知"), "unknown key = 404 语义: {e}");
    let e = svc
        .inbox_summaries("etc", &["a.md".to_string()])
        .unwrap_err();
    assert!(e.contains("未知"), "summaries unknown key = 404 语义: {e}");
}

#[test]
fn spec_bar212_端点_列表信头四字段() {
    let (_t, svc) = book_fixture();
    let ls = svc.list_inbox_letters("na-book").expect("列NA信箱");
    assert_eq!(ls.len(), 2, "README.md 不算信: {ls:?}");
    // 按名升序：0090… 在 bare-letter.md 前
    let full = &ls[0];
    assert_eq!(full.name, "0090号闻灯致评审部白露复0050的回执.md");
    assert_eq!(full.time, "2026-09-30 14:59 +08:00");
    assert_eq!(full.from, "开发部闻灯");
    assert_eq!(full.to, "评审部白露");
    assert_eq!(full.title, "已读，随时可退：无在途写盘");
    assert!(full.bytes > 0 && full.mtime > 0, "老三样不动: {full:?}");
    // 缺字头容错：四字段全空串不炸
    let bare = &ls[1];
    assert_eq!(bare.name, "bare-letter.md");
    assert_eq!(bare.title, "光秃秃的信");
    assert_eq!(bare.time, "");
    assert_eq!(bare.from, "");
    assert_eq!(bare.to, "");
    // 两册同形：main-book 也走这套解析
    let ls = svc.list_inbox_letters("main-book").expect("列主册");
    assert_eq!(ls.len(), 1);
    assert_eq!(ls[0].from, "开发部闻灯");
    // 旧 key 同走扩展：mailbox 夹具信 H1 出 title，无字头三字段空
    let (t2, svc2) = {
        let tmp = tempfile::tempdir().expect("临时目录");
        let root = tmp.path().join("session");
        let mb = root.join("信箱");
        std::fs::create_dir_all(&mb).expect("建信箱");
        std::fs::write(mb.join("a-b-report.md"), "# 信\n正文\n").expect("写信");
        let svc = AgentService::new(
            &root.to_string_lossy(),
            &tmp.path().join("无provider.json").to_string_lossy(),
        );
        (tmp, svc)
    };
    let _keep = t2;
    let ls = svc2.list_letters().expect("旧别名");
    assert_eq!(ls[0].title, "信", "旧 key 同走信头解析");
    assert_eq!(ls[0].time, "");
    assert_eq!(ls[0].from, "");
    assert_eq!(ls[0].to, "");
}

#[test]
fn spec_bar212_端点_summaries提取与剔除() {
    let (_t, svc) = book_fixture();
    let names = vec!["0090号闻灯致评审部白露复0050的回执.md".to_string()];
    let ss = svc.inbox_summaries("na-book", &names).expect("取摘要");
    assert_eq!(ss.len(), 1);
    assert_eq!(ss[0].0, names[0]);
    // mtime 与列表端点同口径咬合（客户端摘要缓存按 (name, mtime) 对账）
    let listed = svc.list_inbox_letters("na-book").expect("列表");
    let listed_mtime = listed
        .iter()
        .find(|l| l.name == names[0])
        .expect("列表含该信")
        .mtime;
    assert!(ss[0].2 > 0, "mtime 必须带: {:?}", ss[0]);
    assert_eq!(ss[0].2, listed_mtime, "与列表端点 mtime 同口径");
    let s = &ss[0].1;
    assert!(s.contains("搬会话的信我读完了"), "真摘要进: {s}");
    assert!(s.contains("要你做的事：没有。"), "多行空格拼接: {s}");
    assert!(!s.contains("注意"), "占位提示行剔除: {s}");
    assert!(!s.contains("LETTER-TOKEN"), "注释行剔除: {s}");
    assert!(!s.contains("就绪申报"), "下一 ## 段不进摘要: {s}");
    // 无摘要块 → ""
    let ss = svc
        .inbox_summaries("na-book", &["bare-letter.md".to_string()])
        .expect("秃信摘要");
    assert_eq!(ss[0].1, "", "无摘要块 = 空串");
}

#[test]
fn spec_bar222_端点_summaries摘帽全量() {
    let (t, svc) = book_fixture();
    let long: String = "长".repeat(200);
    let text = format!("# 长信\n\n> 日期: 2026-09-30\n\n## 摘要\n\n{long}\n\n## 正文\n\n尾。\n");
    let na_book = t.path().join("mail").join("10-NA信箱");
    std::fs::write(na_book.join("long-letter.md"), text).expect("写长信");
    let ss = svc
        .inbox_summaries("na-book", &["long-letter.md".to_string()])
        .expect("取长摘要");
    assert_eq!(
        ss[0].1, long,
        "BAR-222（NA0152「有多少放多少」）：摘要全量返回，逐字等于数据源不截断"
    );
}

#[test]
fn spec_bar212_端点_summaries非法名跳过不连坐() {
    let (_t, svc) = book_fixture();
    let names = vec![
        "../x.md".to_string(),
        "0090号闻灯致评审部白露复0050的回执.md".to_string(),
        "README.md".to_string(),
        "ghost-letter.md".to_string(),
        "随便写的.md".to_string(),
    ];
    let ss = svc.inbox_summaries("na-book", &names).expect("混合名单");
    assert_eq!(
        ss.len(),
        1,
        "非法名/规范文件/不存在全跳过，只剩真信: {ss:?}"
    );
    assert_eq!(ss[0].0, "0090号闻灯致评审部白露复0050的回执.md");
    // 全非法 = 空表不是错
    let ss = svc
        .inbox_summaries("na-book", &["../x.md".to_string()])
        .expect("全非法");
    assert!(ss.is_empty());
}

// ---- BAR-222（NA0152 第四栏）：楼层解析与列表端点透传 ----

/// 带楼层的信：新形楼头两楼 + 一撤楼 + 楼间 `---` 分隔
const LETTER_FLOORED: &str = "# 某信\n\n> 日期: 2026-10-01 12:56 +08:00\n> 从: 开发部观澜\n> 致: 全体\n\n## 摘要\n\n有事。\n\n## 正文\n\n见下。\n\n---\n\n> 1楼：(楼主)观澜→(楼主)观澜 · 2026-10-01 17:23 +08:00\n\n自顶一楼，正文全量保留不截断。\n\n---\n\n> 2楼：承影→1楼(楼主)观澜 · 2026-10-02 14:40 +08:00\n\n回楼：判卷不过，请查。\n\n---\n\n> 3楼：评审部白露→(楼主)观澜 · 2026-10-02 15:21 +08:00\n\n这楼撤了。\n\n——撤回：2026-10-02 15:22 +08:00 评审部白露——原楼作废，理由：试撤\n";

#[test]
fn spec_bar222_楼层解析_新形楼头全账() {
    let fs = na_agentd::service::parse_floors(LETTER_FLOORED);
    assert_eq!(fs.len(), 2, "撤楼整楼跳过: {fs:?}");
    // 1 楼：剥 (楼主) 冠饰，正文全量
    assert_eq!(fs[0].n, 1);
    assert_eq!(fs[0].author, "观澜");
    assert_eq!(fs[0].to, "观澜");
    assert_eq!(fs[0].time, "2026-10-01 17:23 +08:00");
    assert_eq!(fs[0].body, "自顶一楼，正文全量保留不截断。");
    // 2 楼：回楼形被回复者留「1楼观澜」（剥 (楼主) 不剥楼号）
    assert_eq!(fs[1].n, 2);
    assert_eq!(fs[1].author, "承影");
    assert_eq!(fs[1].to, "1楼观澜");
    assert_eq!(fs[1].body, "回楼：判卷不过，请查。");
}

#[test]
fn spec_bar222_楼层解析_边界族() {
    // 无楼层 = 空表
    assert!(na_agentd::service::parse_floors("# 秃信\n\n## 正文\n\n没楼。\n").is_empty());
    // 长正文不截断 + 多行正文保换行 + 尾部 --- 剥掉
    let long = "长".repeat(500);
    let text = format!(
        "# 信\n\n> 1楼：闻灯→承影 · 2026-10-02 06:23 +08:00\n\n第一段。\n\n{long}\n\n---\n"
    );
    let fs = na_agentd::service::parse_floors(&text);
    assert_eq!(fs.len(), 1);
    assert_eq!(fs[0].author, "闻灯");
    assert_eq!(fs[0].to, "承影");
    assert_eq!(fs[0].body, format!("第一段。\n\n{long}"), "正文全量不截断");
    assert!(!fs[0].body.ends_with("---"), "尾部分隔线不进正文");
}

#[test]
fn spec_bar222_端点_列表带楼层且旧两面不带() {
    let (t, svc) = book_fixture();
    let na_book = t.path().join("mail").join("10-NA信箱");
    std::fs::write(na_book.join("0091号观澜致全体的通报.md"), LETTER_FLOORED).expect("写带楼信");
    let ls = svc.list_inbox_letters("na-book").expect("列NA册");
    let floored = ls
        .iter()
        .find(|l| l.name == "0091号观澜致全体的通报.md")
        .expect("带楼信在列");
    assert_eq!(floored.floors.len(), 2, "列表端点透传楼层（撤楼已滤）");
    assert_eq!(floored.floors[0].author, "观澜");
    // 秃信（LETTER_FULL 无楼层）= 空表不炸
    let bare = ls
        .iter()
        .find(|l| l.name == "0090号闻灯致评审部白露复0050的回执.md")
        .expect("旧信在列");
    assert!(bare.floors.is_empty());
    // 旧两面（mailbox/agent-inbox）恒空表——不为新栏破整读戒
    let old = svc.list_inbox_letters("agent-inbox").unwrap_or_default();
    assert!(old.iter().all(|l| l.floors.is_empty()));
}
