//! BAR-247 改道「一口多设备」协议 v2 考题（2026-10-14 用户终审：SG 零增开
//! 新口，分机分口裁被推翻——设计 docs/active/quic隧道.md §十）。
//!
//! 病灶：v1 认证锚焊死端口号（HMAC(psk, "na-quic-auth-v1"‖port_be)），连接
//! 认领槽全局唯一——双机同 psk 同口必互踢（11 机 field-reports 定罪：万次级
//! 顶替）。v2 三件套：
//! - 流头魔数 0xFF02 分流 v1/v2（在册业务口 0/8024/90xx 全在 0xFF00 之下，
//!   零撞；quinn 未配 ALPN，流内自描述与既有「流头自带口+标签」同构）；
//! - 设备 id 入流头、入认证锚（HMAC(psk, "na-quic-auth-v2"‖len‖dev‖port)——
//!   身份焊在 id 上，口不再承载身份）；
//! - 认领槽按 device_id 分槽：同 id 顶替（防僵尸语义保留），异 id 各占各槽。
//!
//! 无旗日：v1 客户端零改动照连（legacy 槽）；v2 由 servers.json quic.deviceId
//! 开启，空串 = v1 旧形态。

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use na_quic::{
    REG_PORT, RevFront, V2_MAGIC, auth_tag, auth_tag2, cert_fingerprint, client_config,
    client_config_rev, gen_self_signed, parse_stream_header_v2, run_client, run_client_v2,
    run_rev_client, run_rev_client_v2, run_rev_server_multi, run_server, server_config,
    server_config_rev, stream_header_v2, valid_device_id,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ---- A 档纯逻辑：设备 id / 流头 / 认证锚 ----

#[test]
fn spec_v2_设备id合法判() {
    assert!(valid_device_id("neo11"));
    assert!(valid_device_id("neo-9_test"));
    assert!(!valid_device_id(""), "空 id = v1 形态，不许混进 v2");
    assert!(!valid_device_id("neo 11"), "空格非法");
    assert!(!valid_device_id("neo.11"), "点号非法");
    assert!(!valid_device_id("中文机"), "非 ASCII 非法");
    let edge = "a".repeat(32);
    assert!(valid_device_id(&edge), "32 字节临界合法");
    let over = "a".repeat(33);
    assert!(!valid_device_id(&over), "33 字节越界");
}

#[test]
fn spec_v2_流头编解码往返() {
    let h = stream_header_v2("neo11", 9021).expect("合法 id 必编码");
    assert_eq!(&h[..2], &V2_MAGIC.to_be_bytes(), "魔数先行");
    let (dev, port) = parse_stream_header_v2(&h[2..]).expect("自编码必自解");
    assert_eq!(dev, "neo11");
    assert_eq!(port, 9021);
    // 注册口也可编码（v2 注册流 = 魔数+id+REG_PORT）
    let r = stream_header_v2("neo9", REG_PORT).unwrap();
    let (dev, port) = parse_stream_header_v2(&r[2..]).unwrap();
    assert_eq!((dev, port), ("neo9", REG_PORT));
    assert!(stream_header_v2("坏 id", 9021).is_none(), "非法 id 拒编码");
    assert!(stream_header_v2("", 9021).is_none(), "空 id 拒编码");
    // 截断/坏体拒解
    assert!(parse_stream_header_v2(b"").is_none());
    assert!(parse_stream_header_v2(&[0]).is_none(), "零长 id 非法");
    assert!(parse_stream_header_v2(&[33]).is_none(), "超长 id 非法");
    assert!(parse_stream_header_v2(&[5, b'n']).is_none(), "id 截断");
    assert!(
        parse_stream_header_v2(&[5, b'n', b'e', b'o', b'1', b'1']).is_none(),
        "端口截断"
    );
    assert!(
        parse_stream_header_v2(&[3, b'n', b' ', b'9', 0, 0]).is_none(),
        "非法字符拒解"
    );
}

#[test]
fn spec_v2_认证标签换锚绑设备() {
    let k = [7u8; 32];
    assert_eq!(
        auth_tag2(&k, "neo11", 9021),
        auth_tag2(&k, "neo11", 9021),
        "同钥同机同口必同签"
    );
    assert_ne!(
        auth_tag2(&k, "neo11", 9021),
        auth_tag(&k, 9021),
        "与 v1 异锚——v1 签不可挪用进 v2"
    );
    assert_ne!(
        auth_tag2(&k, "neo11", 9021),
        auth_tag2(&k, "neo9", 9021),
        "绑设备：标签不可跨机挪用"
    );
    assert_ne!(
        auth_tag2(&k, "neo11", 9021),
        auth_tag2(&k, "neo11", 9022),
        "绑端口：标签不可跨口挪用"
    );
    assert_ne!(
        auth_tag2(&k, "neo11", 9021),
        auth_tag2(&[8u8; 32], "neo11", 9021),
        "异钥必异签"
    );
}

#[test]
fn spec_v2_魔数与在册口零撞() {
    assert_eq!(V2_MAGIC, 0xFF02, "魔数值钉死（线上双端同值才分流得对）");
    assert_ne!(V2_MAGIC, REG_PORT, "注册口不许撞魔数");
    // 编译期常量断言（clippy assertions_on_constants 的合规形态）：
    // 魔数占保留段——真实业务口全在其下，现网业务口与魔数零撞
    const {
        assert!(V2_MAGIC >= 0xFF00);
        assert!(8024 < V2_MAGIC && 9021 < V2_MAGIC && 62633 < V2_MAGIC && 62694 < V2_MAGIC);
    }
}

// ---- 集成判卷：一口多设备（B 档全链，回环 QUIC 实打） ----

/// 正连面判卷核心：双 v2 设备同 UDP 口共存零互踢；同 id 重连只顶自己
#[tokio::test]
async fn spec_oneport_正连双设备零互踢() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [9u8; 32];

    let quic = free_addr().await;
    let echo = free_addr().await;
    tokio::spawn(tcp_echo(echo));
    tokio::spawn(run_server(quic, server_config(certs, key), Some(psk)));

    // 两台 v2 设备（neo11 / neo9）正连腿打进同一 UDP 口
    let front_11 = free_addr().await;
    let front_9 = free_addr().await;
    tokio::spawn(run_client_v2(
        quic,
        "kfm-na",
        front_11,
        echo.port(),
        client_config(pinned),
        Some(psk),
        None,
        "neo11",
    ));
    tokio::spawn(run_client_v2(
        quic,
        "kfm-na",
        front_9,
        echo.port(),
        client_config(pinned),
        Some(psk),
        None,
        "neo9",
    ));

    // 双机各开一条长连——异槽共存（旧制全局单槽下第二条必顶第一条）
    let mut long_11 = wait_connect(front_11).await;
    roundtrip(&mut long_11, b"11 online").await;
    let mut long_9 = wait_connect(front_9).await;
    roundtrip(&mut long_9, b"9 online").await;

    // neo11 换网络重连（同 id 新连接）：顶掉自己的旧连接，防僵尸语义保留
    let front_11b = free_addr().await;
    tokio::spawn(run_client_v2(
        quic,
        "kfm-na",
        front_11b,
        echo.port(),
        client_config(pinned),
        Some(psk),
        None,
        "neo11",
    ));
    let mut s11b = wait_connect(front_11b).await;
    roundtrip(&mut s11b, b"11 reclaim").await; // 首条验签流 = 认领动作
    assert_kicked(&mut long_11).await; // 旧连接被顶收尸
    roundtrip(&mut s11b, b"11 still ok").await; // 新连接续服

    // 判卷核心：neo9 长连全程零扰动
    roundtrip(&mut long_9, b"9 undisturbed").await;
    roundtrip(&mut long_9, b"9 still ok").await;
}

/// 无旗日：v1（legacy 槽）与 v2 同口混跑；legacy 槽顶替语义照旧，
/// v2 槽不受 legacy 顶替波及
#[tokio::test]
async fn spec_oneport_v1v2混跑_legacy单槽顶替照旧() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [10u8; 32];

    let quic = free_addr().await;
    let echo = free_addr().await;
    tokio::spawn(tcp_echo(echo));
    tokio::spawn(run_server(quic, server_config(certs, key), Some(psk)));

    let front_c1 = free_addr().await;
    let front_d = free_addr().await;
    tokio::spawn(run_client(
        quic,
        "kfm-na",
        front_c1,
        echo.port(),
        client_config(pinned),
        Some(psk),
        None,
    ));
    tokio::spawn(run_client_v2(
        quic,
        "kfm-na",
        front_d,
        echo.port(),
        client_config(pinned),
        Some(psk),
        None,
        "neo9",
    ));
    let mut long_c1 = wait_connect(front_c1).await;
    roundtrip(&mut long_c1, b"v1 c1").await;
    let mut long_d = wait_connect(front_d).await;
    roundtrip(&mut long_d, b"v2 d").await;

    // 第二台 v1 设备（旧核第二机）认领 legacy 槽：顶掉第一台 v1
    let front_c2 = free_addr().await;
    tokio::spawn(run_client(
        quic,
        "kfm-na",
        front_c2,
        echo.port(),
        client_config(pinned),
        Some(psk),
        None,
    ));
    let mut s_c2 = wait_connect(front_c2).await;
    roundtrip(&mut s_c2, b"v1 c2 reclaim").await;
    assert_kicked(&mut long_c1).await;

    // v2 设备不受 legacy 顶替波及
    roundtrip(&mut long_d, b"v2 undisturbed").await;
}

/// 反连面判卷核心：一个 UDP 口上多个 TCP 桥前（front），按 device_id
/// 各认各机——流量必须路由到认领该 id 的那条连接所对的设备后端；
/// 同 id 重注册换连接续服，异 id front 全程不动
#[tokio::test]
async fn spec_oneport_反连双front各认各机() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [11u8; 32];

    let quic = free_addr().await;
    let back_a = free_addr().await; // legacy 设备的本机 sshd 扮演
    let back_b = free_addr().await; // neo9 设备的本机 sshd 扮演
    tokio::spawn(tcp_token(back_a, b"from-A"));
    tokio::spawn(tcp_token(back_b, b"from-B"));
    let gate_a = free_addr().await; // 服务器本机桥前（扮演 9022）
    let gate_b = free_addr().await; // 扮演 9025（neo9 专属桥前）
    tokio::spawn(run_rev_server_multi(
        quic,
        server_config_rev(certs, key),
        Some(psk),
        vec![
            RevFront {
                tcp_bind: gate_a,
                device_id: String::new(),
                target_port: back_a.port(),
            },
            RevFront {
                tcp_bind: gate_b,
                device_id: "neo9".into(),
                target_port: back_b.port(),
            },
        ],
    ));
    // v1 旧核注册 legacy 槽；v2 注册 neo9 槽
    tokio::spawn(run_rev_client(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
    ));
    tokio::spawn(run_rev_client_v2(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
        "neo9",
    ));

    // 两 front 各自认领到才绑桥前口；路由错（串到对面连接）会拿到错 token
    let mut long_a = wait_connect(gate_a).await;
    let mut long_b = wait_connect(gate_b).await;
    assert_token(&mut long_a, b"from-A").await;
    assert_token(&mut long_b, b"from-B").await;

    // neo9 换网络重注册：顶替/重绑过渡窗落定后，front neo9 在新连接上续服
    // （判卷纪律：不可抢跑——抢跑连上的是旧监听器，流随旧连接收尸而殇）
    tokio::spawn(run_rev_client_v2(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
        "neo9",
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut b2 = wait_token(gate_b, b"from-B").await;
    assert_token(&mut b2, b"from-B").await;
    // legacy front 全程不动：新连接照服，长连零扰动
    let mut c_new = wait_connect(gate_a).await;
    assert_token(&mut c_new, b"from-A").await;
    assert_token(&mut long_a, b"from-A").await;
}

/// 反连面补充判卷：legacy 长连可复用（无风暴基线）
#[tokio::test]
async fn spec_oneport_反连_legacy长连复用() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [21u8; 32];
    let quic = free_addr().await;
    let back_a = free_addr().await;
    tokio::spawn(tcp_token(back_a, b"from-A"));
    let gate_a = free_addr().await;
    tokio::spawn(run_rev_server_multi(
        quic,
        server_config_rev(certs, key),
        Some(psk),
        vec![RevFront {
            tcp_bind: gate_a,
            device_id: String::new(),
            target_port: back_a.port(),
        }],
    ));
    tokio::spawn(run_rev_client(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
    ));
    let mut long_a = wait_connect(gate_a).await;
    assert_token(&mut long_a, b"from-A").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_token(&mut long_a, b"from-A").await; // 复用同一条长连
}

/// 反连面补充判卷：异槽注册（无顶替）不扰 legacy
#[tokio::test]
async fn spec_oneport_反连_异槽注册不扰legacy() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [22u8; 32];
    let quic = free_addr().await;
    let back_a = free_addr().await;
    let back_b = free_addr().await;
    tokio::spawn(tcp_token(back_a, b"from-A"));
    tokio::spawn(tcp_token(back_b, b"from-B"));
    let gate_a = free_addr().await;
    let gate_b = free_addr().await;
    tokio::spawn(run_rev_server_multi(
        quic,
        server_config_rev(certs, key),
        Some(psk),
        vec![
            RevFront {
                tcp_bind: gate_a,
                device_id: String::new(),
                target_port: back_a.port(),
            },
            RevFront {
                tcp_bind: gate_b,
                device_id: "neo9".into(),
                target_port: back_b.port(),
            },
        ],
    ));
    tokio::spawn(run_rev_client(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
    ));
    let mut long_a = wait_connect(gate_a).await;
    assert_token(&mut long_a, b"from-A").await;
    // neo9 注册（无顶替、无业务流）
    tokio::spawn(run_rev_client_v2(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
        "neo9",
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_token(&mut long_a, b"from-A").await;
}

/// 反连面补充判卷：异槽顶替不扰 legacy（顶替动作只收同槽旧连接的尸）
#[tokio::test]
async fn spec_oneport_反连_异槽顶替不扰legacy() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = [23u8; 32];
    let quic = free_addr().await;
    let back_a = free_addr().await;
    let back_b = free_addr().await;
    tokio::spawn(tcp_token(back_a, b"from-A"));
    tokio::spawn(tcp_token(back_b, b"from-B"));
    let gate_a = free_addr().await;
    let gate_b = free_addr().await;
    tokio::spawn(run_rev_server_multi(
        quic,
        server_config_rev(certs, key),
        Some(psk),
        vec![
            RevFront {
                tcp_bind: gate_a,
                device_id: String::new(),
                target_port: back_a.port(),
            },
            RevFront {
                tcp_bind: gate_b,
                device_id: "neo9".into(),
                target_port: back_b.port(),
            },
        ],
    ));
    tokio::spawn(run_rev_client(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
    ));
    tokio::spawn(run_rev_client_v2(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
        "neo9",
    ));
    let mut long_a = wait_connect(gate_a).await;
    assert_token(&mut long_a, b"from-A").await;
    // neo9 顶替（注册第二条同 id 连接）
    tokio::spawn(run_rev_client_v2(
        quic,
        "kfm-na",
        client_config_rev(pinned),
        Some(psk),
        "neo9",
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_token(&mut long_a, b"from-A").await;
}

// ---- 道具（与 auth_spec/rev_bridge_spec 同形，自含不跨文件） ----

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

/// 固定令牌后端：每收到一笔就回令牌——路由判卷靠令牌认后端（串线必拿错）
async fn tcp_token(bind: SocketAddr, token: &'static [u8]) {
    let l = tokio::net::TcpListener::bind(bind).await.unwrap();
    loop {
        let (mut s, _) = l.accept().await.unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                match s.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if s.write_all(token).await.is_err() {
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

async fn wait_connect(addr: SocketAddr) -> TcpStream {
    for _ in 0..150 {
        if let Ok(s) = TcpStream::connect(addr).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("桥前口 3s 内未就绪");
}

/// 等服务到指定令牌：顶替/重绑过渡窗内，旧监听上的连接会随旧连接收尸
/// （EOF 或静默到超时）——关掉重探，直到一条新连接拿到令牌（= front 已在
/// 新认领连接上续服）
async fn wait_token(addr: SocketAddr, token: &[u8]) -> TcpStream {
    for _ in 0..30 {
        if let Ok(mut s) = TcpStream::connect(addr).await
            && s.write_all(b"ping").await.is_ok()
        {
            let mut back = vec![0u8; token.len()];
            let r = tokio::time::timeout(Duration::from_millis(500), s.read_exact(&mut back)).await;
            if matches!(r, Ok(Ok(_)) if back == token) {
                return s;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("3s+ 内未等到目标令牌服务（顶替续服失败）");
}

async fn roundtrip(s: &mut TcpStream, payload: &[u8]) {
    s.write_all(payload).await.unwrap();
    let mut back = vec![0u8; payload.len()];
    tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut back))
        .await
        .expect("回还超时")
        .unwrap();
    assert_eq!(back, payload, "echo 必须逐字节回还");
}

/// 被顶判卷：踢 = 桥随连接收尸——写一笔逼桥去碰死连接（桥上行写死流即
/// 崩、桥析构、本侧 EOF）；没踢 = echo 回还（顶号传播有时差，吸收掉再探）
async fn assert_kicked(s: &mut TcpStream) {
    for _ in 0..20 {
        if s.write_all(b"x").await.is_err() {
            return; // 写都断了 = 已踢
        }
        let mut buf = [0u8; 1];
        match tokio::time::timeout(Duration::from_millis(500), s.read(&mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) => return, // EOF/断 = 已踢收尸
            Ok(Ok(_)) => {}                   // echo 回还 = 顶号还没到，再探
            Err(_) => {}                      // 本拍静默（桥在排水窗内）→ 再探
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("20 拍内未见被顶连接收尸（EOF/断）");
}

async fn assert_token(s: &mut TcpStream, token: &[u8]) {
    s.write_all(b"ping").await.unwrap();
    let mut back = vec![0u8; token.len()];
    tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut back))
        .await
        .expect("token 回还超时")
        .unwrap();
    assert_eq!(back, token, "路由必须命中本设备后端（串线必拿错令牌）");
}
