//! M3 桥接全链考题（docs/active/quic隧道.md §二、§八 M3）
//!
//! 拓扑：裸 TCP echo（扮演 na-server 9021）← run_server（QUIC 监听+回联）
//! ←QUIC→ run_client（本机 TCP 桥前）← 测试客户端 TCP。
//! 判卷：
//! - B 档全链：经桥两跳的 echo 逐字节回还（pin 正确指纹时握手通过）；
//! - A 档认证：pin 错指纹必须握手即拒（PinnedVerifier 判卷面）；
//! - A 档编解码：port_header/parse_port_header 往返等值。

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use na_quic::{
    cert_fingerprint, client_config, gen_self_signed, parse_port_header, port_header, run_client,
    run_server, server_config,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 裸 TCP echo（扮演 na-server 的 9021）
async fn tcp_echo(bind: SocketAddr) {
    let l = tokio::net::TcpListener::bind(bind).await.unwrap();
    loop {
        let (mut s, _) = l.accept().await.unwrap();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match s.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if s.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
}

/// 占一个回环随机口（占完即放——紧跟着被目标服务绑走，竞态窗口可忽略）
async fn free_addr() -> SocketAddr {
    let l = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

/// 等桥前口起来（run_client 先建 QUIC 连接再绑监听，有先后）
async fn wait_connect(addr: SocketAddr) -> tokio::net::TcpStream {
    for _ in 0..100 {
        if let Ok(s) = tokio::net::TcpStream::connect(addr).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("桥前口 2s 内未就绪");
}

#[tokio::test]
async fn spec_m3_桥接全链_echo_逐字节回还() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);

    let quic_addr = free_addr().await; // QUIC 监听口（UDP）
    let back_addr = free_addr().await; // 后端 echo（扮演 9021）
    let front_addr = free_addr().await; // 桥前口（扮演 App 侧的 127.0.0.1:9021）

    tokio::spawn(tcp_echo(back_addr));
    tokio::spawn(run_server(quic_addr, server_config(certs, key), None));
    tokio::spawn(run_client(
        quic_addr,
        "kfm-na",
        front_addr,
        back_addr.port(),
        client_config(pinned),
        None,
    ));

    // 全链：桥前写 → QUIC 流（端口头=后端口）→ 回联 echo → 原路回还
    let mut s = wait_connect(front_addr).await;
    let payload = "na-quic M3：桥接模型第一声，经两跳必须原样回来".as_bytes();
    s.write_all(payload).await.unwrap();
    let mut back = vec![0u8; payload.len()];
    s.read_exact(&mut back).await.unwrap();
    assert_eq!(back, payload, "经桥两跳的 echo 必须逐字节回还");
}

#[tokio::test]
async fn spec_m3_pinning_错指纹_握手即拒() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let quic_addr = free_addr().await;
    tokio::spawn(run_server(quic_addr, server_config(certs, key), None));

    // 全零指纹 ≠ 服务器真指纹：客户端连接受阻，握手必须失败
    let wrong = [0u8; 32];
    let mut ep = quinn::Endpoint::client((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
    ep.set_default_client_config(client_config(wrong));
    let r = ep.connect(quic_addr, "kfm-na").unwrap().await;
    assert!(r.is_err(), "pin 错指纹必须握手即拒，实际却连上了");
}

#[test]
fn spec_m3_port_header_往返等值() {
    for p in [0u16, 1, 9021, 9023, 65535] {
        assert_eq!(parse_port_header(&port_header(p)), p);
    }
    // 字节序钉死：网络序大端，9021 = 0x233D
    assert_eq!(port_header(9021), [0x23, 0x3D]);
}

/// 隔离复现：HTTP 式「请求→写半关→响应头+体→服务端关」全链
#[tokio::test]
async fn spec_m3_桥接_http式半关全链() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);

    let quic_addr = free_addr().await;
    let back_addr = free_addr().await;
    let front_addr = free_addr().await;

    // 后端：读请求 → 一次性写 头+体 → 关（na-server http_handle 同款行为）
    tokio::spawn(async move {
        let l = tokio::net::TcpListener::bind(back_addr).await.unwrap();
        let (mut s, _) = l.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = s.read(&mut buf).await.unwrap();
        assert!(n > 0);
        let body = "x".repeat(57);
        let resp =
            format!("HTTP/1.1 200 OK\r\nContent-Length: 57\r\nConnection: close\r\n\r\n{body}");
        s.write_all(resp.as_bytes()).await.unwrap();
        // na-server 是 handle 返回即 drop 套接字——照做
    });
    tokio::spawn(run_server(quic_addr, server_config(certs, key), None));
    tokio::spawn(run_client(
        quic_addr,
        "kfm-na",
        front_addr,
        back_addr.port(),
        client_config(pinned),
        None,
    ));

    let mut s = wait_connect(front_addr).await;
    s.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
    s.shutdown().await.unwrap();
    let mut resp = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut resp))
        .await
        .expect("读响应超时")
        .unwrap();
    let text = String::from_utf8_lossy(&resp);
    assert!(
        text.ends_with(&"x".repeat(57)),
        "响应体必须全到，实际 {} 字节: {:?}",
        resp.len(),
        &text[..text.len().min(100)]
    );
}
