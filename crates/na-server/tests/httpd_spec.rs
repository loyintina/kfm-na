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
fn spec_route_sys() {
    assert!(matches!(
        httpd::route("GET", "/api/na/sys"),
        httpd::Route::Sys
    ));
    assert!(matches!(
        httpd::route("GET", "/kfmv4/api/na/sys"),
        httpd::Route::Sys
    ));
    // POST 打 sys 口 = 404（方法也是路由的一部分）
    assert!(matches!(
        httpd::route("POST", "/api/na/sys"),
        httpd::Route::NotFound
    ));
}

#[test]
fn spec_sys_json_形状() {
    let info = na_sys::SysInfo {
        load: Some(na_sys::LoadAvg {
            l1: 0.42,
            l5: 0.38,
            l15: 0.35,
            procs: Some((2, 123)),
        }),
        mem: Some(na_sys::MemInfo {
            total_kb: 16384000,
            avail_kb: 8192000,
            swap: Some((4096000, 1024000)),
        }),
        disk: Some((100_000_000_000, 45_000_000_000)),
        uptime_s: Some(7849375),
    };
    let v: serde_json::Value =
        serde_json::from_str(&httpd::sys_json(&info)).expect("sys 是合法 JSON");
    assert_eq!(v["load"], serde_json::json!([0.42, 0.38, 0.35]));
    assert_eq!(v["mem_total_kb"], 16384000);
    assert_eq!(v["mem_avail_kb"], 8192000);
    assert_eq!(v["disk_total_b"], 100_000_000_000u64);
    assert_eq!(v["disk_avail_b"], 45_000_000_000u64);
    assert_eq!(v["procs"], serde_json::json!([2, 123]));
    assert_eq!(v["swap_total_kb"], 4096000);
    assert_eq!(v["swap_free_kb"], 1024000);
    assert_eq!(v["uptime_s"], 7849375);
    // load 在但 procs 缺（第 4 段坏件）：procs 独立显形 null 不连坐 load
    let info2 = na_sys::SysInfo {
        load: Some(na_sys::LoadAvg {
            l1: 0.1,
            l5: 0.2,
            l15: 0.3,
            procs: None,
        }),
        ..info
    };
    let v2: serde_json::Value =
        serde_json::from_str(&httpd::sys_json(&info2)).expect("sys 是合法 JSON");
    assert!(v2["procs"].is_null(), "procs 坏件 = null 显形");
    assert!(v2["load"].is_array(), "procs 坏了不许连坐 load");
}

#[test]
fn spec_sys_json_坏件显形() {
    // 采不到的路 = null 显形：键永远在（客户端凭键认版本），
    // 值不许缺键不许编造零值（Android 拒 loadavg/uptime 是合法常态）
    let info = na_sys::SysInfo {
        load: None,
        mem: None,
        disk: None,
        uptime_s: None,
    };
    let v: serde_json::Value =
        serde_json::from_str(&httpd::sys_json(&info)).expect("sys 是合法 JSON");
    assert!(v["load"].is_null(), "采不到 = null，不许缺键");
    assert!(v["mem_total_kb"].is_null());
    assert!(v["mem_avail_kb"].is_null());
    assert!(v["disk_total_b"].is_null());
    assert!(v["disk_avail_b"].is_null());
    assert!(v["procs"].is_null());
    assert!(v["swap_total_kb"].is_null());
    assert!(v["swap_free_kb"].is_null());
    assert!(v["uptime_s"].is_null());
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
