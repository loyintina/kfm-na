//! crates/na-server/tests/ui_proxy_spec.rs — BAR-234 钉①：/ui 路由。
//!
//! 前缀转发/路径拼接/前缀边界（不许误吞邻居路径）/上游 8032 不到达时的
//! 诚实报错体。答案区：crates/na-server/src/httpd.rs（ui_upstream/route/
//! ui_error_body）。考题不许改。

use na_server::httpd;

#[test]
fn spec_bar234_ui_前缀转发() {
    match httpd::route("GET", "/ui/训练场/index.html") {
        httpd::Route::Ui { upstream } => assert_eq!(upstream, "/训练场/index.html"),
        _ => panic!("/ui/训练场/index.html 应走反代"),
    }
    match httpd::route("POST", "/ui/训练场/log.txt") {
        httpd::Route::Ui { upstream } => assert_eq!(upstream, "/训练场/log.txt"),
        _ => panic!("POST 也走反代（方法照传）"),
    }
}

#[test]
fn spec_bar234_ui_根路径() {
    // 裸前缀与斜杠收尾都落上游根——静态站的目录列表自己答
    assert_eq!(
        httpd::ui_upstream("/ui").as_deref(),
        Some("/"),
        "裸 /ui = 上游根"
    );
    assert_eq!(
        httpd::ui_upstream("/ui/").as_deref(),
        Some("/"),
        "/ui/ = 上游根"
    );
    assert!(matches!(
        httpd::route("GET", "/ui/"),
        httpd::Route::Ui { .. }
    ));
}

#[test]
fn spec_bar234_ui_路径拼接_query原样() {
    assert_eq!(
        httpd::ui_upstream("/ui/训练场/index.html?v=2").as_deref(),
        Some("/训练场/index.html?v=2"),
        "query 原样携带（由 8032 自己切）"
    );
}

#[test]
fn spec_bar234_ui_非ui路径不误吞() {
    // 既有面不被反代抢走
    assert!(httpd::ui_upstream("/api/fs/list").is_none());
    assert!(matches!(
        httpd::route("GET", "/api/fs/list"),
        httpd::Route::FsList { .. }
    ));
    assert!(matches!(
        httpd::route("GET", "/api/na/health"),
        httpd::Route::Health
    ));
    assert!(matches!(
        httpd::route("POST", "/api/na-report"),
        httpd::Route::Report
    ));
}

#[test]
fn spec_bar234_ui_前缀边界() {
    // /ui 后面必须是 `/` 或字符串结束才算命中：邻居路径一律 None 落 404
    assert!(httpd::ui_upstream("/uix/1").is_none(), "/uix/1 不许误吞");
    assert!(httpd::ui_upstream("/ui2").is_none(), "/ui2 不许误吞");
    assert!(matches!(
        httpd::route("GET", "/uix/1"),
        httpd::Route::NotFound
    ));
    assert!(matches!(
        httpd::route("GET", "/ui2"),
        httpd::Route::NotFound
    ));
}

#[test]
fn spec_bar234_ui_路径穿越负例() {
    // 清和 NA0164 楼1 ② 加的判据：**纯剥离、不归一化、不解码**。
    //
    // 本条函数**不负责**containment（它只剥前缀），所以负例的语义是：
    //   · 不许"顺手归一化"把 `..` 吃掉或改写——那会与 `/agent` 同族的纯剥离形制分家，
    //     也会让"上游看到什么"变成不可预测的；
    //   · 不许**解码**（`%2f` 原样带走，编码归上游）。
    // containment 由**下游 8032 自己兜**（python http.server 的 translate_path
    // 滤 `..` 段）。★ 这一条不是假设，是实测：不经 na 直打 8032，
    //   `/../../../../root/90-信箱/README.md` → 404
    //   `/..%2f..%2f..%2f..%2froot/90-信箱/README.md` → 404
    //   `//root/90-信箱/README.md` → 404
    // 而 docroot 内真文件 `/current.html` → 200（对照组）。
    // ⇒ 若将来上游面变宽（不再只有 8032），**本负例必须重审**。
    assert_eq!(
        httpd::ui_upstream("/ui/../secret").as_deref(),
        Some("/../secret"),
        "不归一化：`..` 原样交给上游（与 /agent 同形制）"
    );
    assert_eq!(
        httpd::ui_upstream("/ui/..%2fsecret").as_deref(),
        Some("/..%2fsecret"),
        "不解码：百分号编码原样交给上游"
    );
    assert_eq!(
        httpd::ui_upstream("/ui//etc/passwd").as_deref(),
        Some("//etc/passwd"),
        "不折叠双斜杠：纯剥离不做路径整理"
    );

    // 不变量（比逐例更强）：剥完的结果**要么是 `/`、要么以 `/` 起头**——
    // 绝不许剥出相对路径（那会让上游按相对路径解析，等于换了个资源）。
    for p in [
        "/ui",
        "/ui/",
        "/ui/../s",
        "/ui/..%2f..%2fs",
        "/ui//a",
        "/ui/a/b/../../c",
    ] {
        let up = httpd::ui_upstream(p).expect("前缀命中");
        assert!(
            up == "/" || up.starts_with('/'),
            "{p} 剥出的上游路径必须绝对：{up}"
        );
    }
}

#[test]
fn spec_bar234_ui_不可达诚实报错体() {
    let body = httpd::ui_error_body("连 127.0.0.1:8032 失败: Connection refused");
    let v: serde_json::Value = serde_json::from_str(&body).expect("合法 JSON");
    assert_eq!(v["ok"], false);
    let err = v["error"].as_str().expect("error 在");
    assert!(err.contains("静态站"), "措辞指向静态站: {err}");
    assert!(err.contains("8032"), "细节透出（不许空泛 502）: {err}");
    assert!(!err.contains("agentd"), "不许串到 agentd 那条反代: {err}");
}
