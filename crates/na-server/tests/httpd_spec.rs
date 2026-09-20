//! crates/na-server/tests/httpd_spec.rs — A 档考题：平面 HTTP 面纯函数
//!
//! 答案区：crates/na-server/src/httpd.rs。本文件是考题，生成器不许改。

use na_server::httpd;

#[test]
fn spec_respond_shape() {
    let r = httpd::respond(200, "OK", "{\"ok\":true}");
    let s = String::from_utf8(r).expect("合法 UTF-8");
    assert!(s.starts_with("HTTP/1.1 200 OK\r\n"), "状态行: {s:?}");
    assert!(s.contains("Content-Type: application/json\r\n"));
    assert!(s.contains("Content-Length: 11\r\n"), "体长 11: {s:?}");
    assert!(s.contains("Connection: close\r\n"));
    assert!(s.ends_with("\r\n\r\n{\"ok\":true}"), "头体分界: {s:?}");
}

#[test]
fn spec_parse_head_ok() {
    let (m, p, cl) = httpd::parse_head(
        "POST /api/na-report HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 42\r\nX-Y: z",
    )
    .expect("合法头");
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/na-report");
    assert_eq!(cl, 42);
}

#[test]
fn spec_parse_head_no_content_length_defaults_zero() {
    let (_, _, cl) = httpd::parse_head("GET /api/na/health HTTP/1.1\r\nHost: x").expect("合法头");
    assert_eq!(cl, 0);
}

#[test]
fn spec_parse_head_case_insensitive_content_length() {
    let (_, _, cl) =
        httpd::parse_head("POST /a HTTP/1.1\r\ncontent-length: 7").expect("大小写不敏感");
    assert_eq!(cl, 7);
}

#[test]
fn spec_parse_head_bad_content_length_err() {
    assert!(httpd::parse_head("POST /a HTTP/1.1\r\nContent-Length: abc").is_err());
}

#[test]
fn spec_parse_head_empty_err() {
    assert!(httpd::parse_head("").is_err());
}

#[test]
fn spec_route_report() {
    assert!(matches!(
        httpd::route("POST", "/api/na-report"),
        httpd::Route::Report
    ));
}

/// /kfmv4 前缀别名必须折叠（na 现网 POST 的就是带前缀路径）
#[test]
fn spec_route_report_kfmv4_prefix_folded() {
    assert!(matches!(
        httpd::route("POST", "/kfmv4/api/na-report"),
        httpd::Route::Report
    ));
}

#[test]
fn spec_route_health() {
    assert!(matches!(
        httpd::route("GET", "/api/na/health"),
        httpd::Route::Health
    ));
}

#[test]
fn spec_route_health_prefix_folded() {
    assert!(matches!(
        httpd::route("GET", "/kfmv4/api/na/health"),
        httpd::Route::Health
    ));
}

#[test]
fn spec_route_method_mismatch_404() {
    // GET 打 report 口 = 404（方法也是路由的一部分）
    assert!(matches!(
        httpd::route("GET", "/api/na-report"),
        httpd::Route::NotFound
    ));
}

#[test]
fn spec_route_unknown_404() {
    assert!(matches!(
        httpd::route("GET", "/api/ai/chat"),
        httpd::Route::NotFound
    ));
}
