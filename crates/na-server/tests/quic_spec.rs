//! crates/na-server/tests/quic_spec.rs — QUIC 腿整环考题（M3）
//!
//! 判卷：真二进制起 na-server（TCP 9021 等价口 + NA_QUIC_BIND 开 QUIC 腿）
//! → na_quic::run_client 桥回本机随机口 → 经桥打 GET /api/na/health，
//! 断言 200 + JSON。证书首跑自签落临时目录，指纹从落盘 DER 重算 pinning。

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 占一个回环随机口
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

#[tokio::test]
async fn spec_m3_na_server_quic_leg_health() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tcp_port = free_port();
    let quic_port = free_port();
    let front_port = free_port();
    let tmp = tempfile::tempdir().expect("临时目录");
    let cert_prefix = tmp.path().join("quic").to_str().unwrap().to_owned();

    // 真二进制：TCP 口 + QUIC 腿 + idle 自退关死
    let child = Command::new(env!("CARGO_BIN_EXE_na-server"))
        .env("NA_BIND", format!("127.0.0.1:{tcp_port}"))
        .env("NA_IDLE_EXIT_SECS", "0")
        .env("NA_REPORT_LOG", tmp.path().join("field-reports.log"))
        .env("NA_QUIC_BIND", format!("127.0.0.1:{quic_port}"))
        .env("NA_QUIC_CERT", &cert_prefix)
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(tmp.path().join("server.err")).unwrap(),
        ))
        .spawn()
        .expect("spawn na-server");
    let _guard = ServerGuard(child);

    // 证书首跑落盘 → 读 DER 重算指纹（pinning 比对物 = 生产同款取径）
    let cert_der = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(d) = std::fs::read(format!("{cert_prefix}.der")) {
                break d;
            }
            assert!(Instant::now() < deadline, "证书 10s 没落盘");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let pinned = na_quic::cert_fingerprint(&rustls::pki_types::CertificateDer::from(cert_der));

    // 桥：本机 front 口 ←QUIC→ na-server QUIC 腿 → 回联 TCP 口
    tokio::spawn(na_quic::run_client(
        format!("127.0.0.1:{quic_port}").parse().unwrap(),
        "kfm-na",
        format!("127.0.0.1:{front_port}").parse().unwrap(),
        tcp_port,
        na_quic::client_config(pinned),
    ));

    // 经桥打健康检查（等桥前口起来）
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut s = loop {
        match tokio::net::TcpStream::connect(("127.0.0.1", front_port)).await {
            Ok(s) => break s,
            Err(_) => {
                assert!(Instant::now() < deadline, "桥前口 10s 未就绪");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };
    s.write_all(b"GET /api/na/health HTTP/1.0\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    // 半关写向：桥 splice 上行见 EOF 才 finish QUIC 流，全链才能落地 EOF
    s.shutdown().await.unwrap();
    let mut resp = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut resp))
        .await
        .expect("读响应超时")
        .unwrap();
    let text = String::from_utf8_lossy(&resp);
    if !text.contains("\"uptime_s\"") {
        eprintln!(
            "=== na-server stderr ===\n{}",
            std::fs::read_to_string(tmp.path().join("server.err")).unwrap_or_default()
        );
        eprintln!("=== 响应 {} 字节 ===\n{text}", resp.len());
    }
    assert!(
        text.starts_with("HTTP/1.0 200") || text.starts_with("HTTP/1.1 200"),
        "经 QUIC 桥的 health 必须 200，实际: {}",
        &text[..text.len().min(120)]
    );
    assert!(
        text.contains("\"uptime_s\""),
        "health 必须是 health_json 体: {text}"
    );
}
