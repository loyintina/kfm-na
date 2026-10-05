//! gate_http_spec.rs — BAR-233 乙案 v1 服务器侧考题。
//!
//! 判卷维度（白露 MAIN0125 §二之4）：路由白名单红绿例／pending 体
//! 黄金样例／队列与结果的往返与一次性语义。端到端「断 sshd 场景」
//! 在设备侧判卷（卡萝+承影），本卷钉住纯函数面。

use na_server::gateq;
use na_server::httpd::Route;

#[test]
fn spec_bar233_路由白名单红绿() {
    // 绿：九名全通
    for ch in [
        "shot-req",
        "shot-gles-req",
        "text-req",
        "keys-in",
        "ping-req",
        "restart-req",
        "trace-req",
        "stats-req",
        "orb-inject",
    ] {
        assert!(gateq::channel_ok(ch), "{ch} 应在白名单");
        assert!(
            matches!(
                httpd_route("POST", &format!("/api/gate/{ch}")),
                Route::GatePush { .. }
            ),
            "POST /api/gate/{ch} 应路由 GatePush"
        );
    }
    // 红：非法通道名（路径穿越/未列名）
    for bad in [
        "install-apk-req",
        "shot-req%2F..%2Fetc",
        "../etc/passwd",
        "front-req",
    ] {
        assert!(!gateq::channel_ok(bad), "{bad} 不应在白名单");
    }
    // pending 是 GET 面：POST /api/gate/pending → NotFound
    assert!(matches!(
        httpd_route("POST", "/api/gate/pending"),
        Route::NotFound
    ));
    assert!(matches!(
        httpd_route("GET", "/api/gate/pending"),
        Route::GatePending { wait: 0 }
    ));
    // wait 解析与上限
    assert!(matches!(
        httpd_route("GET", "/api/gate/pending?wait=99"),
        Route::GatePending { wait: 30 }
    ));
    // result 两路
    assert!(matches!(
        httpd_route("POST", "/api/gate/result/shot.rgb"),
        Route::GateResultPut { .. }
    ));
    assert!(matches!(
        httpd_route("GET", "/api/gate/result/ping-res"),
        Route::GateResultGet { .. }
    ));
}

fn httpd_route(method: &str, path: &str) -> Route {
    na_server::httpd::route(method, path)
}

#[test]
fn spec_bar233_队列往返与pending体() {
    // push → drain 清空 → 再 drain 为空（一次性）
    gateq::push("ping-req", b"alive beat".to_vec());
    gateq::push("restart-req", Vec::new());
    let items = gateq::drain();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].1, "ping-req");
    assert_eq!(items[0].2, b"alive beat");
    assert!(gateq::queue_empty());
    assert!(gateq::drain().is_empty());
    // pending 体黄金样例（计数行 + channel\thex 行；空 payload = 空 hex）
    let body = gateq::pending_body(&items);
    assert_eq!(body, "2\nping-req\t616c6976652062656174\nrestart-req\t\n");
}

#[test]
fn spec_bar233_结果一次性() {
    gateq::put_result("shot.rgb", vec![0x00, 0xff, 0x10]);
    assert_eq!(gateq::take_result("shot.rgb"), Some(vec![0x00, 0xff, 0x10]));
    assert_eq!(gateq::take_result("shot.rgb"), None, "取走即删（一次性）");
    assert_eq!(gateq::take_result("never-put"), None);
    // 覆写语义：同名单次覆盖
    gateq::put_result("ping-res", b"old".to_vec());
    gateq::put_result("ping-res", b"new".to_vec());
    assert_eq!(gateq::take_result("ping-res"), Some(b"new".to_vec()));
}
