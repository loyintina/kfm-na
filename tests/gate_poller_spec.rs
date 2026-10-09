//! gate_poller_spec.rs — BAR-233 na 侧考题（纯函数钉）。
//!
//! 判卷维度：hex 编解码往返（含空/二进制）／pending 体解析（坏行跳过
//! 不整批炸）／write_atomic 原子性与白名单纵深闸／请求字节构造／
//! 守卫常量（节拍/握手超时）。
//! 端到端冒烟（真 QUIC 桥全环：fetch → parse → 原子落盘）在
//! tests/gate_poller_quic_spec.rs（BAR-233 根治案，进程内 QUIC 直连）；
//! 「断 sshd 全灭」场景判卷在设备侧（pkill sshd 后八通道全活——
//! MAIN0125 §四）。

#[test]
fn spec_bar233_hex往返() {
    let f = kfm_na::gate_poller::hex_decode;
    // 空与往返
    assert_eq!(f(""), Some(Vec::new()));
    assert_eq!(f("00ff10"), Some(vec![0x00, 0xff, 0x10]));
    assert_eq!(f("616c"), Some(b"al".to_vec()));
    // 坏输入：奇长/大写/非法字符
    assert_eq!(f("0"), None);
    assert_eq!(f("0F"), None, "只认小写");
    assert_eq!(f("zz"), None);
}

/// HTTP/1.1 请求字节构造（QUIC 直连路的纯函数面）：method/path/Host/
/// Content-Length/Connection: close 五件齐、CRLF 结尾——旧 TCP 路同构
#[test]
fn spec_bar233_请求字节构造() {
    let req =
        kfm_na::gate_poller::build_request("GET", "/api/gate/pending?wait=2", 0, "srv.example");
    let s = String::from_utf8(req).unwrap();
    assert!(
        s.starts_with("GET /api/gate/pending?wait=2 HTTP/1.1\r\n"),
        "请求行: {s}"
    );
    assert!(s.contains("Host: srv.example\r\n"), "Host 头: {s}");
    assert!(s.contains("Content-Length: 0\r\n"), "Content-Length: {s}");
    assert!(s.contains("Connection: close\r\n"), "Connection: {s}");
    assert!(s.ends_with("\r\n\r\n"), "头体分隔收尾: {s}");
    let post = kfm_na::gate_poller::build_request("POST", "/api/gate/result/x", 5, "h");
    assert!(
        String::from_utf8(post)
            .unwrap()
            .contains("Content-Length: 5\r\n")
    );
}

#[test]
fn spec_bar233_pending解析_坏行跳过() {
    let body = b"3\nping-req\t616c697665\nrestart-req\t\nBAD-NO-TAB\nkeys-in\tzzbad\n";
    let items = kfm_na::gate_poller::parse_pending(body);
    // 坏行（无 TAB、非法 hex）跳过，好行保留——不整批炸
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, "ping-req");
    assert_eq!(items[0].1, b"alive");
    assert_eq!(items[1].0, "restart-req");
    assert!(items[1].1.is_empty(), "空 payload");
}

#[test]
fn spec_bar233_write_atomic_原子与白名单() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // 白名单纵深闸：非白名单名拒（服务器侧已闸，这里第二道）
    assert!(kfm_na::gate_poller::write_atomic(dir, "evil-req", b"x").is_err());
    // BAR-240：switch-req 过纵深闸（与 gateq.rs CHANNELS 同源同票）
    kfm_na::gate_poller::write_atomic(dir, "switch-req", b"").unwrap();
    assert!(dir.join("switch-req").exists());
    // 含换行/NUL 的 payload 逐字节落地；无 .new 残留
    let payload: &[u8] = b"line1\nline2\x00tail";
    kfm_na::gate_poller::write_atomic(dir, "keys-in", payload).unwrap();
    assert_eq!(std::fs::read(dir.join("keys-in")).unwrap(), payload);
    assert!(!dir.join("keys-in.new").exists(), "rename 后无 .new 残留");
    // 覆写语义：第二次写完整替换（不留半文件）
    kfm_na::gate_poller::write_atomic(dir, "keys-in", b"second").unwrap();
    assert_eq!(std::fs::read(dir.join("keys-in")).unwrap(), b"second");
}

#[test]
fn spec_bar233_轮询节拍短窗() {
    // BAR-233 追件：POLL_WAIT_SECS 须为 2（25s 长轮询在 QUIC 桥撞空闲
    // 掐线——现场「响应无头体分隔」实证；变异：改回 25 → 本钉红）
    assert_eq!(
        kfm_na::gate_poller::POLL_WAIT_SECS,
        2,
        "短轮询节拍被改回长窗"
    );
}

#[test]
fn spec_bar233_tick心跳与超时常量() {
    // BAR-233 追件三（白露 NA0163 19 楼批）：僵死双治的守卫常量——
    // tick 每 30 拍（变异：摘心跳块 → 本钉红不了行为，钉常量存在性
    // 由源码守卫咬，这里钉节拍不被改坏）；根治返工（共享腿连接）后
    // 「不挂死」的防线 = 往返整体限时（握手归腿的 run_client 自带 8s
    // 速败；poller 侧 open_bi/读写卡死由限时兜住——变异：摘 timeout
    // 包装 → 本钉红）。
    assert_eq!(kfm_na::gate_poller::POLL_WAIT_SECS, 2);
    // tick 节拍 30 写死在 poll_loop——源码守卫（test-bar-new 同族）在
    // scripts/check/ 不便，这里以「常量面 + 源码 grep」双咬：
    let src = std::fs::read_to_string("src/gate_poller.rs").unwrap_or_default();
    assert!(
        src.contains("is_multiple_of(30)"),
        "tick 心跳块被摘（每 30 拍一行是僵死可见性的唯一解药）"
    );
    assert!(
        src.contains("tokio::time::timeout"),
        "往返限时被摘（读写卡死/连接死无兜底——挂死防线回潮）"
    );
    assert!(
        src.contains("block_on"),
        "runtime-in-thread 同步桥被拆（poller 是 std::thread，学 tunnel.rs）"
    );
    assert!(
        src.contains("data_conn_slot"),
        "共享腿槽接线被拆（start 必须从 tunnel 真槽取连接）"
    );
    // tunnel 侧接线锚（返工裁定：腿握手后把 Connection 递进真槽）——
    // spawn_quic_leg 必须给 run_client 传 Some(conn_tx)（变异：传 None
    // → 槽永远空 → poller 永远等腿握手，此锚红）
    let tsrc = std::fs::read_to_string("src/tunnel.rs").unwrap_or_default();
    assert!(
        tsrc.contains("Some(conn_tx)"),
        "tunnel 腿没把 conn_out 信道递给 run_client（槽永远空）"
    );
    assert!(
        tsrc.contains("DATA_CONN_SLOT"),
        "tunnel 数据腿连接槽被摘（poller 无从取连接）"
    );
}
