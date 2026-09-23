//! 客户端证考题（设计 docs/active/quic隧道.md §四）：HMAC-SHA256 手卷
//! 判卷 = RFC 4231 标准向量；认证标签/常量时间比对/封禁裁决 = A 档纯逻辑；
//! 桥接带证全链（对钥匙过 / 错钥匙弃流）= B 档判卷。

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use na_quic::{
    AUTH_BAN_SECS, AUTH_FAIL_TRIP, auth_tag, ban_verdict, cert_fingerprint, client_config, ct_eq,
    gen_self_signed, hmac_sha256, run_client, run_server, server_config,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// RFC 4231 §4：HMAC-SHA256 标准向量——手卷实现的判卷人
#[test]
fn spec_hmac_sha256_rfc4231向量() {
    // Case 1：20 字节 0x0b / "Hi There"
    assert_eq!(
        hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    // Case 2："Jefe" / "what do ya want for nothing?"
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // Case 6：131 字节 0xaa（超块长 = 先哈希再当钥——手卷的分支面）
    assert_eq!(
        hex(&hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn spec_auth_tag_绑定端口且确定() {
    let k = [7u8; 32];
    assert_eq!(auth_tag(&k, 9021), auth_tag(&k, 9021), "同钥同口必同签");
    assert_ne!(
        auth_tag(&k, 9021),
        auth_tag(&k, 9022),
        "绑端口头：标签不可跨口挪用"
    );
    assert_ne!(auth_tag(&k, 9021), auth_tag(&[8u8; 32], 9021), "异钥必异签");
}

#[test]
fn spec_ct_eq_常量时间比对() {
    let a = [1u8; 32];
    assert!(ct_eq(&a, &[1u8; 32]));
    assert!(!ct_eq(&a, &[2u8; 32]));
    let mut b = [1u8; 32];
    b[31] = 0;
    assert!(!ct_eq(&a, &b), "末位差异也必须咬");
}

#[test]
fn spec_ban_verdict_连败封禁() {
    assert_eq!(AUTH_FAIL_TRIP, 5, "跳闸线钉死");
    assert_eq!(AUTH_BAN_SECS, 600, "封禁窗钉死");
    assert!(!ban_verdict(4, 0), "4 连败未跳闸");
    assert!(ban_verdict(5, 0), "5 连败跳闸");
    assert!(ban_verdict(9, 599), "窗内仍封");
    assert!(!ban_verdict(9, 600), "出窗即解封");
}

/// 带证桥接全链：对钥匙 echo 逐字节回还；错钥匙弃流（零字节回还）
#[tokio::test]
async fn spec_auth_桥接带证_对错钥匙两判() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [42u8; 32];

    let quic_addr = free_addr().await;
    let back_addr = free_addr().await;
    tokio::spawn(tcp_echo(back_addr));
    tokio::spawn(run_server(quic_addr, server_config(certs, key), Some(psk)));

    // 对钥匙：echo 必回还
    let front_ok = free_addr().await;
    tokio::spawn(run_client(
        quic_addr,
        "kfm-na",
        front_ok,
        back_addr.port(),
        client_config(pinned),
        Some(psk),
    ));
    let mut s = wait_connect(front_ok).await;
    let payload = "带证桥接：认钥不认人".as_bytes();
    s.write_all(payload).await.unwrap();
    let mut back = vec![0u8; payload.len()];
    s.read_exact(&mut back).await.unwrap();
    assert_eq!(back, payload, "对钥匙必须逐字节回还");
    drop(s);

    // 错钥匙：服务器验签不过弃流——桥前零字节回还
    let front_bad = free_addr().await;
    tokio::spawn(run_client(
        quic_addr,
        "kfm-na",
        front_bad,
        back_addr.port(),
        client_config(pinned),
        Some([43u8; 32]),
    ));
    let mut s = wait_connect(front_bad).await;
    s.write_all(payload).await.unwrap();
    s.shutdown().await.unwrap();
    let mut resp = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), s.read_to_end(&mut resp)).await;
    assert!(
        resp.is_empty(),
        "错钥匙必须零字节回还，实际 {} 字节",
        resp.len()
    );
}

// ---- 桥接考题同款道具（与 bridge_spec 同形，自含不跨文件） ----

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

async fn free_addr() -> SocketAddr {
    let l = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

async fn wait_connect(addr: SocketAddr) -> tokio::net::TcpStream {
    for _ in 0..100 {
        if let Ok(s) = tokio::net::TcpStream::connect(addr).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("桥前口 2s 内未就绪");
}
