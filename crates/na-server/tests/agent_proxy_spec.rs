//! crates/na-server/tests/agent_proxy_spec.rs — BAR-163 钉①：/agent 路由。
//!
//! 前缀转发/路径拼接/上游 9041 不到达时的诚实报错体。
//! 答案区：crates/na-server/src/httpd.rs（agent_upstream/route/agent_error_body）。
//! 考题不许改。

use na_server::httpd;

#[test]
fn spec_bar163_agent_前缀转发() {
    match httpd::route("GET", "/agent/api/agent/lines") {
        httpd::Route::Agent { upstream } => assert_eq!(upstream, "/api/agent/lines"),
        _ => panic!("/agent/api/agent/lines 应走反代"),
    }
    match httpd::route("POST", "/agent/api/agent/lines/demo/send") {
        httpd::Route::Agent { upstream } => assert_eq!(upstream, "/api/agent/lines/demo/send"),
        _ => panic!("POST 也走反代（方法照传）"),
    }
}

#[test]
fn spec_bar163_agent_路径拼接_query原样() {
    assert_eq!(
        httpd::agent_upstream("/agent/api/agent/lines/demo/tail?n=7").as_deref(),
        Some("/api/agent/lines/demo/tail?n=7"),
        "query 原样携带"
    );
    assert_eq!(
        httpd::agent_upstream("/agent").as_deref(),
        Some("/"),
        "裸前缀 = 上游根"
    );
}

#[test]
fn spec_bar163_agent_非api面不放行() {
    // 反代不是任意口转发器：/agent 后面不接 /api/ 的一律 404
    assert!(httpd::agent_upstream("/agent/etc/passwd").is_none());
    assert!(httpd::agent_upstream("/agent/ws").is_none());
    assert!(matches!(
        httpd::route("GET", "/agent/etc/passwd"),
        httpd::Route::NotFound
    ));
    // 既有面不被反代抢走
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
fn spec_bar163_agent_不可达诚实报错体() {
    let body = httpd::agent_error_body("连 127.0.0.1:9041 失败: Connection refused");
    let v: serde_json::Value = serde_json::from_str(&body).expect("合法 JSON");
    assert_eq!(v["ok"], false);
    let err = v["error"].as_str().expect("error 在");
    assert!(err.contains("agent 反代失败"), "{err}");
    assert!(err.contains("9041"), "细节透出（不许空泛 502）: {err}");
}
