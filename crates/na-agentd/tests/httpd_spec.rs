//! crates/na-agentd/tests/httpd_spec.rs — BAR-161 钉：daemon 路由纯函数。
//!
//! 答案区：crates/na-agentd/src/httpd.rs。考题不许改。

use na_agentd::httpd;

#[test]
fn spec_bar161d_route_四面() {
    assert!(matches!(
        httpd::route("GET", "/api/agent/health"),
        httpd::Route::Health
    ));
    assert!(matches!(
        httpd::route("GET", "/api/agent/lines"),
        httpd::Route::Lines
    ));
    match httpd::route("POST", "/api/agent/lines/demo/send") {
        httpd::Route::Send { line } => assert_eq!(line, "demo"),
        _ => panic!("send 路由"),
    }
    match httpd::route("GET", "/api/agent/lines/demo/tail?n=7") {
        httpd::Route::Tail { line, n } => {
            assert_eq!(line, "demo");
            assert_eq!(n, 7);
        }
        _ => panic!("tail 路由"),
    }
}

#[test]
fn spec_bar161d_route_tail_n缺省与钳制() {
    match httpd::route("GET", "/api/agent/lines/x/tail") {
        httpd::Route::Tail { n, .. } => assert_eq!(n, 50, "缺省 50"),
        _ => panic!("tail"),
    }
    match httpd::route("GET", "/api/agent/lines/x/tail?n=99999") {
        httpd::Route::Tail { n, .. } => assert_eq!(n, 500, "上限钳 500"),
        _ => panic!("tail"),
    }
    match httpd::route("GET", "/api/agent/lines/x/tail?n=abc") {
        httpd::Route::Tail { n, .. } => assert_eq!(n, 50, "坏值回落缺省"),
        _ => panic!("tail"),
    }
}

#[test]
fn spec_bar161d_route_方法与未知_404() {
    assert!(matches!(
        httpd::route("POST", "/api/agent/lines"),
        httpd::Route::NotFound
    ));
    assert!(
        matches!(
            httpd::route("GET", "/api/agent/lines/demo/send"),
            httpd::Route::NotFound
        ),
        "GET 打 send 口 = 404（方法也是路由的一部分）"
    );
    assert!(
        matches!(
            httpd::route("GET", "/api/na/health"),
            httpd::Route::NotFound
        ),
        "na-server 的面不归本 daemon"
    );
    assert!(matches!(httpd::route("GET", "/"), httpd::Route::NotFound));
}

#[test]
fn spec_bar161d_parse_head() {
    let (m, p, cl) = httpd::parse_head(
        "POST /api/agent/lines/demo/send HTTP/1.1\r\nHost: 127.0.0.1:9041\r\nContent-Length: 21",
    )
    .expect("合法头");
    assert_eq!(
        (m.as_str(), p.as_str(), cl),
        ("POST", "/api/agent/lines/demo/send", 21)
    );
    assert!(httpd::parse_head("").is_err());
    assert!(httpd::parse_head("POST /a HTTP/1.1\r\nContent-Length: zz").is_err());
}

#[test]
fn spec_bar161d_respond_shape() {
    let r = httpd::respond(200, "OK", "{\"ok\":true}");
    let s = String::from_utf8(r).expect("UTF-8");
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(s.contains("Content-Length: 11\r\n"));
    assert!(s.ends_with("\r\n\r\n{\"ok\":true}"));
}
