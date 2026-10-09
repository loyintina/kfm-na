//! tests/gate_poller_quic_spec.rs — BAR-233 根治案端到端考题（共享腿
//! 连接直开流，白露 NA0163 返工裁定）。
//!
//! 基建复用结论：na-server/tests/quic_spec.rs 起的是真 na-server 二进制
//! （CARGO_BIN_EXE 只在该包测试里可用，且其 QUIC 桥流头路由写死回联
//! 9021——本开发机 9021 被常驻真 na-server 占用，pid 实证），故本卷照
//! na-quic/tests/bridge_spec.rs 的同款基建自起：gen_self_signed 自签 +
//! **真 na_quic::run_server 桥**（与生产 na-server 数据腿同一份代码：
//! 逐流消费端口头+认证标签验签、回联 TCP、splice）承接目标口。
//! 拓扑：假 gate HTTP（回固定 pending 体）← run_server（真 QUIC 桥）
//! ←QUIC→ 测试自连 client 的 Connection（写进注入槽——生产槽由
//! tunnel::spawn_quic_leg 的 run_client 握手后递入，同一时序语义）→
//! gate_poller 槽取连接开流 → 断言触发文件落注入 dump_dir。
//! 目标口经 start_with 注入随机口（生产恒 NA_SERVER_PORT 9021——桥的
//! 路由按流头走，行为同一）。**帧序（port_header+auth_tag 先于 HTTP
//! 字节）由本卷咬住**：psk 验签不符整流被弃 → 无响应 → 落不了盘。
//! offer_result 未启动/坏路径 no-op 也在本卷顺带咬（直接调用不 panic）。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// 32 字节 → 64 位小写 hex（ServerEntry.pin/psk 的编码面——parse_pin 只认小写）
fn hex64(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 占一个回环随机 UDP 口（占完即放，紧跟被 run_server 绑走）
fn free_udp_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

#[tokio::test]
async fn spec_bar233_quic共享腿直开流_端到端落盘() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    std::fs::create_dir_all(&dump).unwrap();

    // 假 gate HTTP 后端（扮演 na-server 9021）：第一轮回 pending（含
    // 两条），后续轮询空（Connection: close 一回一连接）。读到头体分隔
    // 即回——不依赖 EOF：桥 splice 不向 TCP 转发 QUIC 半关（GET 无体）
    let back = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_port = back.local_addr().unwrap().port();
    tokio::spawn(async move {
        for i in 0..40u32 {
            let Ok((mut s, _)) = back.accept().await else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match s.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let body: &str = if i == 0 {
                "2\nping-req\t\nrestart-req\t68656c6c6f\n"
            } else {
                "0\n"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    // 真 QUIC 桥（生产 na-server 数据腿同款 run_server）：验签 psk
    let (certs, key) = na_quic::gen_self_signed("kfm-na");
    let pin = na_quic::cert_fingerprint(&certs[0]);
    let psk = [7u8; 32];
    let quic_port = free_udp_port();
    let quic_addr: std::net::SocketAddr = format!("127.0.0.1:{quic_port}").parse().unwrap();
    tokio::spawn(na_quic::run_server(
        quic_addr,
        na_quic::server_config(certs, key),
        Some(psk),
    ));

    // 测试自连 client：握手成功把 Connection 写进注入槽（生产同一时序
    // ——tunnel 腿 run_client 握手后经 conn_out 递入真槽）
    let mut ep = quinn::Endpoint::client(std::net::SocketAddr::from((
        std::net::Ipv4Addr::LOCALHOST,
        0,
    )))
    .unwrap();
    ep.set_default_client_config(na_quic::client_config(pin));
    let conn = ep
        .connect(quic_addr, "kfm-na")
        .unwrap()
        .await
        .expect("测试 client 握手");
    let slot = Arc::new(Mutex::new(Some(conn)));

    let srv = kfm_na::settings::ServerEntry {
        id: "t".into(),
        name: "t".into(),
        ssh: kfm_na::settings::SshFields {
            host: "127.0.0.1".into(),
            port: 22,
            user: String::new(),
            key_path: String::new(),
            password: String::new(),
        },
        tunnel: kfm_na::settings::TunnelPorts::default(),
        ws_url: String::new(),
        command: None,
        hotkey: None,
        backend: kfm_na::settings::Backend::NaServer,
        quic: kfm_na::settings::QuicFields {
            enable: true,
            port: quic_port,
            pin: hex64(&pin),
            psk: hex64(&psk),
        },
        gate_port: 9021,
    };
    kfm_na::gate_poller::start_with(srv, target_port, slot, dump.clone());

    // offer_result 在（伪）启动态：坏路径不 panic 即可（读失败不上传）
    tokio::time::sleep(Duration::from_millis(300)).await;
    kfm_na::gate_poller::offer_result("never-file", std::path::Path::new("/nonexistent"));

    // 等第一轮落盘（QUIC 握手 + 首轮轮询）
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = false;
    while Instant::now() < deadline {
        if dump.join("ping-req").exists() && dump.join("restart-req").exists() {
            got = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(got, "经真 QUIC 桥的 pending 应已落成触发文件");
    assert_eq!(std::fs::read(dump.join("restart-req")).unwrap(), b"hello");
}
