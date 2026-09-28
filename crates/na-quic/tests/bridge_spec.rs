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
    auth_tag, cert_fingerprint, client_config, gen_self_signed, parse_port_header, port_header,
    probe, run_client, run_server, server_config,
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

/// BAR-171：桥一侧终结 → 另侧排水窗内强制收尾。旧 `join!` 制下后端
/// 永不开口 = 桥任务陪葬（本钉在旧码上必超时判红）。排水窗拧到
/// 300ms（本文件其他钉在回环上 300ms 绰绰有余，互不影响）。
#[tokio::test]
async fn spec_bar171_桥一侧终_排水窗内收尾() {
    unsafe { std::env::set_var("NA_SPLICE_DRAIN_MS", "300") };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);

    let quic_addr = free_addr().await;
    let back_addr = free_addr().await;
    let front_addr = free_addr().await;

    // 后端：accept 后永不开口永不关（扮演死透但 TCP 没断的对端）；
    // 读到 EOF = 桥把后端侧收掉了——这是唯一的判卷观察口
    let (eof_tx, eof_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let l = tokio::net::TcpListener::bind(back_addr).await.unwrap();
        let (mut s, _) = l.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        loop {
            match s.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = eof_tx.send(());
                    return;
                }
                Ok(_) => {} // 请求字节照吞，不开口
            }
        }
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

    // 客户端：写一字节后整个 socket 直接丢（全关）——桥两侧都必须
    // 在排水窗内收尾，最终传导成后端的 EOF
    let mut s = wait_connect(front_addr).await;
    s.write_all(b"x").await.unwrap();
    drop(s);

    tokio::time::timeout(Duration::from_secs(10), eof_rx)
        .await
        .expect("10s 内后端没等到 EOF——桥任务陪葬（BAR-171 病灶：一侧终另侧永挂）")
        .unwrap();
}

/// BAR-171 翻案·修复①：正连腿顶替——新连接第一条验签通过的流落地，
/// 旧连接必须当场 close（级联：splice 终 → loopback TCP 断 → wsterm
/// killAll → 预热壳整组收）。带 psk 考：验签不过的假客户端顶不掉
/// 真客户端（握手无客户端证，扫描器也能走完握手）。
#[tokio::test]
async fn spec_bar171_正连顶替_新客认领_旧客收尸() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [7u8; 32];

    let quic_addr = free_addr().await;
    let back_addr = free_addr().await;
    tokio::spawn(tcp_echo(back_addr));
    tokio::spawn(run_server(quic_addr, server_config(certs, key), Some(psk)));
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 客户端工厂（角色 = 手机重连后新一茬预热池的第一条控制通道）：
    // 裸 quinn 握手 + 开流写端口头+验签标签；流的发送端随连接一并
    // 返回保活（drop 了 splice 收尾是流水席的事，连接级存活才是
    // 判卷面）
    async fn authed_conn(
        quic_addr: SocketAddr,
        pinned: [u8; 32],
        psk: [u8; 32],
        port: u16,
    ) -> (quinn::Connection, quinn::SendStream) {
        let mut ep = quinn::Endpoint::client((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        ep.set_default_client_config(client_config(pinned));
        let conn = ep.connect(quic_addr, "kfm-na").unwrap().await.unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        send.write_all(&port_header(port)).await.unwrap();
        send.write_all(&auth_tag(&psk, port)).await.unwrap();
        (conn, send)
    }

    let (c1, _s1) = authed_conn(quic_addr, pinned, psk, back_addr.port()).await;
    tokio::time::sleep(Duration::from_millis(300)).await; // 等认领落地
    assert!(c1.close_reason().is_none(), "自己认领自己不许自杀");

    // 假客户端（验签标签错）：流被弃、不认领——真客户端必须无恙
    {
        let mut ep = quinn::Endpoint::client((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        ep.set_default_client_config(client_config(pinned));
        let fake = ep.connect(quic_addr, "kfm-na").unwrap().await.unwrap();
        let (mut send, _recv) = fake.open_bi().await.unwrap();
        send.write_all(&port_header(back_addr.port()))
            .await
            .unwrap();
        send.write_all(&[0u8; 32]).await.unwrap(); // 假标签
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            c1.close_reason().is_none(),
            "验签不过的连接顶不掉真客户端（扫描器免疫条款）"
        );
    }

    // 真重连：c2 认领 → c1 必须被判死
    let (c2, _s2) = authed_conn(quic_addr, pinned, psk, back_addr.port()).await;
    tokio::time::timeout(Duration::from_secs(5), c1.closed())
        .await
        .expect("5s 内旧连接没被判死——顶替失效（BAR-171 白天重连堆积病灶）");
    assert!(c2.close_reason().is_none(), "新连接必须活着接棒");
}

/// BAR-171 翻案③：probe 只握手——活口必须探得通；死口（回环无人听
/// 的 UDP）必须速败，不许挂到 idle 上限（BAR-146 黑洞教训）
#[tokio::test]
async fn spec_bar171_probe_活口通_死口速败() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);

    let quic_addr = free_addr().await;
    tokio::spawn(run_server(quic_addr, server_config(certs, key), None));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        probe(quic_addr, "kfm-na", client_config(pinned)).await,
        "活口必须探得通"
    );

    let dead = free_addr().await; // 占过即放——回环上此刻无人听
    let t0 = std::time::Instant::now();
    assert!(
        !probe(dead, "kfm-na", client_config(pinned)).await,
        "死口必须报死"
    );
    assert!(
        t0.elapsed() < Duration::from_secs(10),
        "死口探测必须速败（握手超时 8s + 余量），实际 {:?}",
        t0.elapsed()
    );
}
