//! gate_http_spec.rs — BAR-233 乙案 v1 服务器侧考题。
//!
//! 判卷维度（白露 MAIN0125 §二之4）：路由白名单红绿例／pending 体
//! 黄金样例／队列与结果的往返与一次性语义。端到端「断 sshd 场景」
//! 在设备侧判卷（卡萝+承影），本卷钉住纯函数面。

use na_server::gateq;
use na_server::httpd::Route;

#[test]
fn spec_bar233_路由白名单红绿() {
    // 绿：十名全通
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
        "switch-req",
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

/// BAR-240（MAIN0134 实案）：switch-req 必须在白名单——承影远程「先切回
/// 自己会话」的正路被 400 拒，才逼出 keys-in 盲发误投白露会话。
/// 变异：从 CHANNELS 摘掉 switch-req → 本钉红。
#[test]
fn spec_bar240_switch_req入白名单() {
    assert!(gateq::channel_ok("switch-req"), "switch-req 应在白名单");
    assert!(
        matches!(
            httpd_route("POST", "/api/gate/switch-req"),
            Route::GatePush { .. }
        ),
        "POST /api/gate/switch-req 应路由 GatePush"
    );
}

#[test]
fn spec_bar233_队列往返与pending体() {
    // push → drain 清空 → 再 drain 为空（一次性）
    // （19021＝本钉专口：QUEUE 按口分槽（NA0163），并行考题各用各口互不串）
    gateq::push(19021, "ping-req", b"alive beat".to_vec());
    gateq::push(19021, "restart-req", Vec::new());
    let items = gateq::drain(19021);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].1, "ping-req");
    assert_eq!(items[0].2, b"alive beat");
    assert!(gateq::queue_empty(19021));
    assert!(gateq::drain(19021).is_empty());
    // pending 体黄金样例（计数行 + channel\thex 行；空 payload = 空 hex）
    let body = gateq::pending_body(&items);
    assert_eq!(body, "2\nping-req\t616c6976652062656174\nrestart-req\t\n");
}

#[test]
fn spec_bar233_结果一次性() {
    gateq::put_result(19022, "shot.rgb", vec![0x00, 0xff, 0x10]);
    assert_eq!(
        gateq::take_result(19022, "shot.rgb"),
        Some(vec![0x00, 0xff, 0x10])
    );
    assert_eq!(
        gateq::take_result(19022, "shot.rgb"),
        None,
        "取走即删（一次性）"
    );
    assert_eq!(gateq::take_result(19022, "never-put"), None);
    // 覆写语义：同名单次覆盖
    gateq::put_result(19022, "ping-res", b"old".to_vec());
    gateq::put_result(19022, "ping-res", b"new".to_vec());
    assert_eq!(gateq::take_result(19022, "ping-res"), Some(b"new".to_vec()));
}

/// NA0163 楼25/26 多设备闸门（B 案「每机一条闸门腿」）：队列与结果表
/// 按**到达监听口**分槽——端口即设备命名空间，9 机（9023）与 11 机
/// （主口）各进各队、各取各果，互不串。
/// 变异：gateq 退回全局单槽（丢 leg 键）→ 本钉红。
#[test]
fn spec_na0163_按口分槽互不串() {
    // 9023 的货别的口看不见（19023＝只读见证口，不进货）
    gateq::push(9023, "ping-req", b"leg9023".to_vec());
    assert!(gateq::queue_empty(19023), "别的口不应见 9023 的货");
    assert!(gateq::drain(19023).is_empty());
    let items = gateq::drain(9023);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].1, "ping-req");
    assert_eq!(items[0].2, b"leg9023");
    // 结果表同按口分槽
    gateq::put_result(9023, "shot.rgb", vec![1, 2, 3]);
    assert_eq!(
        gateq::take_result(19023, "shot.rgb"),
        None,
        "别的口不应取到 9023 的结果"
    );
    assert_eq!(gateq::take_result(9023, "shot.rgb"), Some(vec![1, 2, 3]));
}

/// parse_gate_legs（A 档纯函数，NA0163 楼25/26）：逗号分隔腿口，
/// fail-loud 拒半残/歧义配置（串机温床宁可拒启）。
/// 变异：重复/撞主口校验摘掉任一 → 对应断言红。
#[test]
fn spec_na0163_gate_legs解析() {
    // 未设 / 空串 = 无腿
    assert_eq!(gateq::parse_gate_legs(None, 9021), Ok(vec![]));
    assert_eq!(gateq::parse_gate_legs(Some(""), 9021), Ok(vec![]));
    // 单口 / 双口（逗号分隔、容忍空白）
    assert_eq!(gateq::parse_gate_legs(Some("9023"), 9021), Ok(vec![9023]));
    assert_eq!(
        gateq::parse_gate_legs(Some("9023, 9025"), 9021),
        Ok(vec![9023, 9025])
    );
    // 拒：腿间重复 / 撞主口 / 非数字 / 0
    assert!(gateq::parse_gate_legs(Some("9023,9023"), 9021).is_err());
    assert!(gateq::parse_gate_legs(Some("9021"), 9021).is_err());
    assert!(gateq::parse_gate_legs(Some("abc"), 9021).is_err());
    assert!(gateq::parse_gate_legs(Some("0"), 9021).is_err());
}
