//! M4 反连桥接全链考题（docs/active/quic隧道.md §二、§八 M4）
//!
//! 拓扑（角色与正连镜像）：裸 TCP echo（扮演手机侧 na sshd 8024）
//! ← run_rev_client（手机：QUIC 拨出 + 注册流 + accept 回联）
//! ←QUIC→ run_rev_server（服务器：QUIC 监听 + 注册闸 + 本机 TCP 桥前
//! 扮演 9022）← 测试客户端 TCP（扮演 na_ssh）。
//! 判卷：
//! - B 档全链：na_ssh → 9022 → QUIC 反向开流 → echo 逐字节回还；
//! - A 档认证：注册流 psk 错 = 永不认领（TCP 桥前永不绑——零服务，
//!   不是「服务了再拒」）；
//! - A 档注册口：REG_PORT = 0（非业务口，与正连流天然不混）。
//!
//! 变异抽检：①服务器注册闸摘验签（错钥匙也认领 = 公网口白开）必须咬；
//! ②客户端注册流写错端口头（不写 REG_PORT = 服务器永不认领）必须咬。

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use na_quic::{
    REG_PORT, cert_fingerprint, client_config, gen_self_signed, run_rev_client, run_rev_server,
    server_config,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 裸 TCP echo（扮演手机侧 na sshd）
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

/// 占一个回环随机口
async fn free_addr() -> SocketAddr {
    let l = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

/// 桥前口可连则返回（反连服务器先认领注册再绑 TCP，有先后）
async fn try_connect(addr: SocketAddr, tries: u32) -> Option<tokio::net::TcpStream> {
    for _ in 0..tries {
        if let Ok(s) = tokio::net::TcpStream::connect(addr).await {
            return Some(s);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

fn psk_of(b: u8) -> [u8; 32] {
    [b; 32]
}

#[tokio::test]
async fn spec_m4_反连桥接全链_echo_逐字节回还() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = psk_of(7);

    let quic_addr = free_addr().await; // QUIC 反连监听（UDP，扮演 62694）
    let sshd_addr = free_addr().await; // 手机侧 echo（扮演 na sshd 8024）
    let gate_addr = free_addr().await; // 服务器本机 TCP 桥前（扮演 9022）

    tokio::spawn(tcp_echo(sshd_addr));
    tokio::spawn(run_rev_server(
        quic_addr,
        server_config(certs, key),
        Some(psk),
        gate_addr,
        sshd_addr.port(),
    ));
    tokio::spawn(run_rev_client(
        quic_addr,
        "kfm-na",
        client_config(pinned),
        Some(psk),
    ));

    // 全链：na_ssh → 9022 → QUIC 反向开流 → 手机回联 echo → 原路回还
    let mut s = try_connect(gate_addr, 100)
        .await
        .expect("注册认领后桥前口 2s 内必须就绪");
    let payload = "na-quic M4：反连第一声，经两跳必须原样回来".as_bytes();
    s.write_all(payload).await.unwrap();
    let mut back = vec![0u8; payload.len()];
    s.read_exact(&mut back).await.unwrap();
    assert_eq!(back, payload, "经反连桥两跳的 echo 必须逐字节回还");
}

#[tokio::test]
async fn spec_m4_反连注册_错钥匙零服务() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);

    let quic_addr = free_addr().await;
    let sshd_addr = free_addr().await;
    let gate_addr = free_addr().await;

    tokio::spawn(tcp_echo(sshd_addr));
    tokio::spawn(run_rev_server(
        quic_addr,
        server_config(certs, key),
        Some(psk_of(7)),
        gate_addr,
        sshd_addr.port(),
    ));
    // 错钥匙客户端：注册验签必栽 → 服务器永不认领 → 桥前口永不可连
    tokio::spawn(run_rev_client(
        quic_addr,
        "kfm-na",
        client_config(pinned),
        Some(psk_of(9)),
    ));

    assert!(
        try_connect(gate_addr, 25).await.is_none(),
        "错钥匙必须零服务（桥前口永不绑），实际却连上了（变异①：注册闸摘验签漏网）"
    );
}

#[test]
fn spec_m4_注册口_零非业务() {
    assert_eq!(REG_PORT, 0, "注册口 = 0（合法业务口之外，与正连流不混）");
}
