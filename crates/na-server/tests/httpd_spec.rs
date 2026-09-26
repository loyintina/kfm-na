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
        cores: Some(4),
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
    assert_eq!(v["cores"], 4, "核数下发（客户端凭它算负载占比判色）");
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
        cores: None,
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
    assert!(
        v["cores"].is_null(),
        "采不到核数 = null（客户端回退窗峰归一）"
    );
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

// ---------- 文件树数据面路由（A 档；语义在 na-protocol::fsapi，此处只钉路由形状） ----------

#[test]
fn spec_route_fs_list_取_dir() {
    let httpd::Route::FsList { dir } = httpd::route("GET", "/api/fs/list?dir=x") else {
        panic!("GET /api/fs/list 该是 FsList");
    };
    assert_eq!(dir, "x");
    // 无 query → dir 空串（= 允许根本身）
    let httpd::Route::FsList { dir } = httpd::route("GET", "/api/fs/list") else {
        panic!("无 query 也要路由得上");
    };
    assert_eq!(dir, "");
    // 百分号编码的路径值
    let httpd::Route::FsList { dir } = httpd::route("GET", "/api/fs/list?dir=%2Froot%2F00&x=1")
    else {
        panic!();
    };
    assert_eq!(dir, "/root/00");
    // 方法也参与路由
    assert!(matches!(
        httpd::route("POST", "/api/fs/list?dir=x"),
        httpd::Route::NotFound
    ));
}

/// /kfmv4 前缀别名与 query 共存：先切 query 再折前缀，参数不许被前缀折叠吃掉
#[test]
fn spec_route_fs_kfmv4_前缀别名() {
    let httpd::Route::FsList { dir } = httpd::route("GET", "/kfmv4/api/fs/list?dir=x") else {
        panic!("前缀别名该路由到 FsList");
    };
    assert_eq!(dir, "x");
    let httpd::Route::FsRead { path, max } = httpd::route("GET", "/kfmv4/api/fs/read?path=a&max=9")
    else {
        panic!("前缀别名该路由到 FsRead");
    };
    assert_eq!(path, "a");
    assert_eq!(max, 9);
}

#[test]
fn spec_route_fs_read_取_path_max() {
    let httpd::Route::FsRead { path, max } = httpd::route("GET", "/api/fs/read?path=sub%2Fa.txt")
    else {
        panic!("GET /api/fs/read 该是 FsRead");
    };
    assert_eq!(path, "sub/a.txt");
    assert_eq!(max, na_protocol::fsapi::DEFAULT_MAX, "缺 max = 64KB");
    let httpd::Route::FsRead { path, max } = httpd::route("GET", "/api/fs/read?path=b.txt&max=abc")
    else {
        panic!();
    };
    assert_eq!(path, "b.txt");
    assert_eq!(max, na_protocol::fsapi::DEFAULT_MAX, "非数字回落缺省");
    let httpd::Route::FsRead { max, .. } =
        httpd::route("GET", "/api/fs/read?path=c.txt&max=999999999")
    else {
        panic!();
    };
    assert_eq!(max, na_protocol::fsapi::MAX_MAX, "上限 1MB");
    let httpd::Route::FsRead { path, max } =
        httpd::route("GET", "/api/fs/read?path=%26%3D&max=1024")
    else {
        panic!();
    };
    assert_eq!(path, "&=", "值里的 %26/%3D 解成普通字符");
    assert_eq!(max, 1024);
}

/// 越界/不存在/类型不符在 HTTP 层必须塌成同一 404 字节串（不透露存在性）
#[test]
fn spec_fs_error_response_同文案() {
    let nf = httpd::fs_error_response(&na_protocol::fsapi::FsError::NotFound);
    let nd = httpd::fs_error_response(&na_protocol::fsapi::FsError::NotDir);
    assert_eq!(nf, nd, "NotFound 与 NotDir 不许互相区分");
    let s = String::from_utf8(nf).unwrap();
    assert!(s.starts_with("HTTP/1.1 404 Not Found\r\n"), "{s:?}");
    assert!(
        s.ends_with("{\"ok\":false,\"error\":\"not found\"}"),
        "{s:?}"
    );
    // Io = 500 显形（不是「没有」）
    let io = String::from_utf8(httpd::fs_error_response(&na_protocol::fsapi::FsError::Io(
        "permission denied".into(),
    )))
    .unwrap();
    assert!(
        io.starts_with("HTTP/1.1 500 Internal Server Error\r\n"),
        "{io:?}"
    );
    assert!(
        io.ends_with("{\"ok\":false,\"error\":\"permission denied\"}"),
        "{io:?}"
    );
}
