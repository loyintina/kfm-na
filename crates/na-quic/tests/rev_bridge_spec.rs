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
    IDLE_TIMEOUT, KEEPALIVE, REG_PORT, REV_IDLE_TIMEOUT, cert_fingerprint, client_config,
    client_config_rev, gen_self_signed, run_rev_client, run_rev_server, server_config,
    server_config_rev,
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

/// BAR-157 考场基建：发得出 Initial 就整机冻结的弃尸客户端
/// （手机侧「8s 握手超时 → 销毁重投」的缩影——socket 留着、驱动停摆，
/// 服务器回包被内核收下但无人 ACK，半生连接只能等 idle 收尸。
/// 必须冻结而非 drop：本机回环 drop 掉 socket 会回 ICMP 拒绝，
/// 服务器秒收尸，生产上 CGNAT 吞掉 ICMP 的「无声陈尸」就演不出来）
fn spawn_abandoning_client(quic_addr: SocketAddr, pinned: [u8; 32]) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut ep =
                quinn::Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).unwrap();
            ep.set_default_client_config(client_config(pinned));
            let _connecting = ep.connect(quic_addr, "kfm-na").unwrap();
            // 让 Initial 发出去（并撑出一点重传窗口）
            tokio::time::sleep(Duration::from_millis(300)).await;
            // 冻结：线程停在这里直到考场进程退场
            std::future::pending::<()>().await;
        });
    });
}

#[tokio::test]
async fn spec_m4_bar157_弃尸风暴_诚实客户端不被憋死() {
    // BAR-157（2026-09-26 pcap 定罪）：run_rev_server 认领循环串行
    // await 每个 Incoming——客户端弃连后服务器要等满 REV_IDLE_TIMEOUT
    // 才收尸，期间 accept 停摆，而 quinn 的 Incoming 在应用 accept 前
    // 不回第一个包 → 所有新握手零回包。62694 实录：60s cadence 一具
    // 陈尸一组 PTO 序列飞向早已放弃的端口，23 次新鲜尝试全程零回包。
    // 考场：8 具弃尸后，诚实客户端必须在预算内完成握手。
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = gen_self_signed("kfm-na");
    let pinned = cert_fingerprint(&certs[0]);
    let psk = psk_of(7);

    let quic_addr = free_addr().await;
    let sshd_addr = free_addr().await;
    let gate_addr = free_addr().await;

    // 考场加速器：服务器 idle 压到 2s（真身 60s——「陈尸占队等 idle
    // 收尸」的病灶结构不变，只是一具陈尸占队 2s 而非 60s）
    let mut sc = server_config_rev(certs, key);
    std::sync::Arc::get_mut(&mut sc.transport)
        .unwrap()
        .max_idle_timeout(Some(Duration::from_secs(2).try_into().unwrap()));

    tokio::spawn(tcp_echo(sshd_addr));
    tokio::spawn(run_rev_server(
        quic_addr,
        sc,
        Some(psk),
        gate_addr,
        sshd_addr.port(),
    ));

    // 弃尸风暴：8 具，间隔 300ms——旧码串行认领下它们要排队等收尸，
    // 排尽需 ≥ 2s（末具 idle）+ 8×0.3s（间隔）≈ 4.4s
    for _ in 0..8 {
        spawn_abandoning_client(quic_addr, pinned);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // 诚实客户端：旧码下它的握手排在弃尸队尾（~4.4s 后才被 accept），
    // 1.5s 预算必超；accept 驱动独立成任务后毫秒级完成
    let mut ep = quinn::Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).unwrap();
    ep.set_default_client_config(client_config(pinned));
    tokio::time::timeout(
        Duration::from_millis(1500),
        ep.connect(quic_addr, "kfm-na").unwrap(),
    )
    .await
    .expect("诚实客户端 1.5s 握手预算——超时 = accept 被陈尸风暴憋死（BAR-157 复现）")
    .expect("握手成功");
}

#[test]
fn spec_m4_反连死寂判死_常量契约() {
    // M4-5 兜底演练实踩：反连腿无本地可观测物，死寂判死全靠 idle
    // 上限——沿用数据腿 4h = ssh 兜底永远接不上（9022 僵尸占口）
    assert_eq!(
        REV_IDLE_TIMEOUT,
        Duration::from_secs(60),
        "反连死寂判死钉死 60s（keepalive 6 倍）"
    );
    assert!(
        REV_IDLE_TIMEOUT >= KEEPALIVE * 3,
        "idle 必须 ≥ 3× keepalive——健康连接对端 ACK 续命不被误杀"
    );
    assert!(
        REV_IDLE_TIMEOUT < IDLE_TIMEOUT,
        "反连判死必须远快于数据腿 4h——不然 ssh 兜底永远接不上 9022"
    );
    // 双腿配置面真实吃到这个常量（装配钉：常量改了接线没改 = 白钉）
    let _c = client_config_rev([7u8; 32]);
    let (certs, key) = gen_self_signed("kfm-na");
    let _s = server_config_rev(certs, key);
}
