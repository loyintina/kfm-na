//! na-quic — QUIC 隧道核心（设计：docs/active/quic隧道.md）
//!
//! 桥接模型：本机 TCP 监听器 → QUIC bidirectional stream → 对端回联 TCP。
//! 协议层零改动：App 照旧打 127.0.0.1:9021，QUIC 只是载体（角色与今天
//! 的 ssh -L/-R 完全相同），出问题关桥退 ssh，应用层无感。
//!
//! 分层：核心层合规——quinn/rustls/ring 无平台依赖（M1 双端编译实证）。
//!
//! 帧格式：每条 bidirectional stream 前 2 字节 = 目标端口（网络序）。
//! 双向同构——客户端开流带 9021（正连），服务器开流带 8024（反连），
//! 对端按端口回联本机 127.0.0.1:{port}。撞口在类型层面不存在：
//! 多并发 = 多条流，没有端口绑定动作。

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// idle 上限：4h——省电冻结期连接状态保活（代价 ≈ 一条 CID 表项，设计 §五）
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(4 * 3600);

/// 反连腿死寂判死（M4-5 兜底演练实踩）：反连连接**协议层必须自带
/// 死亡检测**——数据腿僵尸有 e2e 探活收尸（本地口还在可打），反连
/// 腿无本地可观测物，服务器重启/网络静死后全靠 idle 上限。沿用 4h
/// = ssh 兜底永远接不上（2026-09-24 实录：systemd 重启 na-server，
/// 反连腿僵尸举「在线」相，9022 无人绑）。取 60s = keepalive 10s
/// 的 6 倍：健康连接对端 ACK 续命稳活，死寂 1 分钟内定罪；doze 冻结
/// 期计时器同冻，醒来一次性重拉（成本与数据腿 BAR-141 同级）
pub const REV_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 客户端保活：10s 一 ping 保 NAT 映射（设计 §五）
pub const KEEPALIVE: Duration = Duration::from_secs(10);

/// 握手超时（BAR-146）：UDP 黑洞里 connect 会挂到 idle 上限（4h）——
/// 腿线程不死、死信不发、本地口不开，看门狗在 Starting 里空转成天坑。
/// 握手必须速败，把定罪权交回看门狗（跳闸账满即降级 ssh 兜底）
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);

// ---- A 档纯逻辑：流头端口编解码 ----

/// 流头（A 档）：每条流前 2 字节 = 目标端口（网络序大端）
pub fn port_header(port: u16) -> [u8; 2] {
    port.to_be_bytes()
}

pub fn parse_port_header(h: &[u8; 2]) -> u16 {
    u16::from_be_bytes(*h)
}

// ---- 证书 ----

/// 自签证书生成（双端同用：服务器生成一次落盘，客户端钉指纹——设计 §四）
pub fn gen_self_signed(
    name: &str,
) -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let cert = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
    (
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into()),
    )
}

/// 证书指纹（pinning 的比对物）：DER 的 SHA-256
pub fn cert_fingerprint(der: &rustls::pki_types::CertificateDer<'_>) -> [u8; 32] {
    use sha2::Digest as _;
    let mut h = sha2::Sha256::new();
    h.update(der.as_ref());
    h.finalize().into()
}

// ---- 客户端证（设计 §四：预共享密钥 HMAC 挑战——服务器认密钥不认 IP） ----

/// HMAC-SHA256（RFC 2104 手卷——不引 hmac 依赖，vendor 免重生；
/// 判卷 = RFC 4231 标准向量，tests/auth_spec.rs 钉死）
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha2::Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = sha2::Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let ih = inner.finalize();
    let mut outer = sha2::Sha256::new();
    outer.update(opad);
    outer.update(ih);
    outer.finalize().into()
}

/// 认证标签长度（流头 = 2 字节端口 + 32 字节标签）
pub const AUTH_TAG_LEN: usize = 32;

/// 认证标签：HMAC(psk, "na-quic-auth-v1" || 端口头)。标签在 TLS 内部
/// 传输——攻击者看不见（加密）也伪造不了（无钥），静态标签已足够；
/// 绑端口头 = 标签不可跨端口挪用
pub fn auth_tag(psk: &[u8; 32], port: u16) -> [u8; 32] {
    let mut m = [0u8; 17];
    m[..15].copy_from_slice(b"na-quic-auth-v1");
    m[15..].copy_from_slice(&port.to_be_bytes());
    hmac_sha256(psk, &m)
}

/// 常量时间比对（认证面不许早退——时序侧信道零成本就堵上）
pub fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 认证连败封禁裁决（A 档纯函数）：同一来源连败满 trip 次，ban_secs 内
/// 不再给它花一个字节的处理——在线枚举 32 字节密钥本是天文数字，
/// 这道闸防的是资源消耗，不是枚举本身
pub const AUTH_FAIL_TRIP: u32 = 5;
pub const AUTH_BAN_SECS: u64 = 600;

pub fn ban_verdict(fails: u32, elapsed_secs: u64) -> bool {
    fails >= AUTH_FAIL_TRIP && elapsed_secs < AUTH_BAN_SECS
}

/// 服务器 QUIC 配置
pub fn server_config(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> ServerConfig {
    server_config_with_idle(certs, key, IDLE_TIMEOUT)
}

/// 服务器 QUIC 配置（反连腿版，M4-5）：死寂判死 60s——手机静死/掉线
/// 时认领连接按期收尸，9022 及时让位 ssh 伴生兜底（4h = 兜底撞口
/// 活锁：na-server 僵尸占着 9022，ssh -R 绑不上 255 空转）
pub fn server_config_rev(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> ServerConfig {
    server_config_with_idle(certs, key, REV_IDLE_TIMEOUT)
}

fn server_config_with_idle(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
    idle: Duration,
) -> ServerConfig {
    let mut sc = ServerConfig::with_single_cert(certs, key).expect("证书装载");
    Arc::get_mut(&mut sc.transport)
        .expect("transport 独占")
        .max_concurrent_bidi_streams(128u32.into())
        .max_idle_timeout(Some(idle.try_into().expect("idle 上限")));
    sc
}

/// 指纹 pinning 验证器（设计 §四：不走 CA——我们没有域名，CA 是负资产）
#[derive(Debug)]
pub struct PinnedVerifier {
    pub pinned: [u8; 32],
}

impl rustls::client::danger::ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if cert_fingerprint(end_entity) == self.pinned {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "na-quic: 服务器证书指纹与 pinning 不符".into(),
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &rustls::pki_types::CertificateDer<'_>,
        _d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _m: &[u8],
        _c: &rustls::pki_types::CertificateDer<'_>,
        _d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// 客户端 QUIC 配置（指纹 pinning）
pub fn client_config(pinned: [u8; 32]) -> ClientConfig {
    client_config_with_idle(pinned, IDLE_TIMEOUT)
}

/// 客户端 QUIC 配置（反连腿版，M4-5）：死寂判死 60s——反连腿无本地
/// 可观测物（数据腿僵尸有 e2e 探活收尸），服务器重启/网络静死全靠
/// idle 上限定罪；4h = ssh 兜底永远接不上（2026-09-24 兜底演练实踩：
/// systemd 重启 na-server，反连腿僵尸举「在线」相，9022 无人绑）
pub fn client_config_rev(pinned: [u8; 32]) -> ClientConfig {
    client_config_with_idle(pinned, REV_IDLE_TIMEOUT)
}

fn client_config_with_idle(pinned: [u8; 32], idle: Duration) -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier { pinned }))
        .with_no_client_auth();
    let mut cc = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).expect("tls 配置"),
    ));
    cc.transport_config(Arc::new({
        let mut t = quinn::TransportConfig::default();
        t.keep_alive_interval(Some(KEEPALIVE));
        t.max_idle_timeout(Some(idle.try_into().expect("idle 上限")));
        t
    }));
    cc
}

// ---- B 档桥接（TCP↔stream splice，IO 胶水） ----

/// TCP ↔ QUIC 流双向拷贝：任一侧 EOF/错误即停（B 档胶水，无判卷面）
async fn splice(
    mut tcp: tokio::net::TcpStream,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
) {
    let (mut tr, mut tw) = tcp.split();
    let up = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match tr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if send.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = send.finish();
    };
    let down = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match recv.read(&mut buf).await {
                Ok(Some(n)) if n > 0 => {
                    if tw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    };
    tokio::join!(up, down);
}

/// 服务器腿：QUIC 监听，每条入站流读流头 → 回联 127.0.0.1:{port} →
/// splice。流头 = 2 字节端口；psk 为 Some 时再读 32 字节认证标签，
/// 验不过直接弃流并按来源 IP 记连败（满 AUTH_FAIL_TRIP 封 10 分钟）。
/// 连接级循环：连接死了 accept 出错返回（看门狗外侧重建）
pub async fn run_server(
    bind: SocketAddr,
    cfg: ServerConfig,
    psk: Option<[u8; 32]>,
) -> std::io::Result<()> {
    use std::collections::HashMap;
    let ep = Endpoint::server(cfg, bind).map_err(|e| std::io::Error::other(e.to_string()))?;
    // 连败账（来源 IP → (次数, 首次失败时刻)）——认证面的资源闸
    let fails = std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::<
        std::net::IpAddr,
        (u32, std::time::Instant),
    >::new()));
    while let Some(inc) = ep.accept().await {
        let fails = std::sync::Arc::clone(&fails);
        tokio::spawn(async move {
            let Ok(conn) = inc.await else { return };
            let ip = conn.remote_address().ip();
            // 敲门账本（用户「担心被扫」的观测面）：谁来了/谁验签栽了/
            // 谁被封，全落 stderr（na-server → /var/log/kfm-na-server.log）
            eprintln!("[na-quic] 连接 {ip}");
            {
                let g = fails.lock().await;
                if let Some(&(n, t0)) = g.get(&ip)
                    && ban_verdict(n, t0.elapsed().as_secs())
                {
                    eprintln!("[na-quic] 封禁中拒连 {ip}（第 {n} 次连败）");
                    conn.close(1u32.into(), b"auth banned");
                    return;
                }
            }
            loop {
                let stream = conn.accept_bi().await;
                let Ok((send, mut recv)) = stream else { break };
                let fails = std::sync::Arc::clone(&fails);
                tokio::spawn(async move {
                    let mut hdr = [0u8; 2];
                    if recv.read_exact(&mut hdr).await.is_err() {
                        return;
                    }
                    let port = parse_port_header(&hdr);
                    if let Some(k) = psk {
                        let mut tag = [0u8; AUTH_TAG_LEN];
                        let ok = recv.read_exact(&mut tag).await.is_ok()
                            && ct_eq(&tag, &auth_tag(&k, port));
                        if !ok {
                            let mut g = fails.lock().await;
                            let e = g.entry(ip).or_insert((0, std::time::Instant::now()));
                            if e.1.elapsed().as_secs() >= AUTH_BAN_SECS {
                                *e = (0, std::time::Instant::now());
                            }
                            e.0 += 1;
                            eprintln!("[na-quic] 验签失败 {ip}（第 {} 次连败）", e.0);
                            return;
                        }
                    }
                    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                    let Ok(tcp) = tokio::net::TcpStream::connect(target).await else {
                        return;
                    };
                    splice(tcp, send, recv).await;
                });
            }
        });
    }
    Ok(())
}

/// 客户端腿：连上服务器保持一条 QUIC 连接；本机 TCP 监听器每收一个
/// 连接 → 开一条流（写端口头 = target_port；psk 为 Some 时续写认证
/// 标签——设计 §四客户端证）→ splice。连接死 = 本函数返回（看门狗
/// 外侧重建——死亡检测是事件驱动的，这就是 QUIC 腿比 ssh 腿省掉探活
/// 三件套的原因）。target_port 与 local_bind 解耦：部署惯例双端同口
/// （9021→9021），但桥接模型本身不绑这个约定
pub async fn run_client(
    server: SocketAddr,
    sni: &str,
    local_bind: SocketAddr,
    target_port: u16,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
) -> std::io::Result<()> {
    let mut ep = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    ep.set_default_client_config(cfg);
    let conn: Connection = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        ep.connect(server, sni)
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    )
    .await
    .map_err(|_| std::io::Error::other("QUIC 握手超时（UDP 黑洞？）"))?
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    let listener = tokio::net::TcpListener::bind(local_bind).await?;
    loop {
        let (tcp, _) = listener.accept().await?;
        let Ok((mut send, recv)) = conn.open_bi().await else {
            return Err(std::io::Error::other("QUIC 连接已死"));
        };
        tokio::spawn(async move {
            if send.write_all(&port_header(target_port)).await.is_err() {
                return;
            }
            if let Some(k) = psk
                && send.write_all(&auth_tag(&k, target_port)).await.is_err()
            {
                return;
            }
            splice(tcp, send, recv).await;
        });
    }
}

// ---- M4 反连路（设计 §二：9022 从「sshd 绑口」变「na-server 本机 TCP
// 监听器 + 服务器沿 QUIC 反向开流」——撞口/僵尸/释放三件套整族消失）----
//
// 角色互换：手机是 QUIC 客户端（拨出），服务器是流的**发起方**。
// 注册闸：反连客户端不主动开业务流，服务器无从验客户端证——故连接
// 建立后手机必须先开一条「注册流」（端口头 = REG_PORT + psk 标签），
// 验过才认领这条连接当反连载具；验不过记连败（与正连同一份资源闸）。

/// 注册流端口头（A 档常量）：0 不是合法业务口，天然不会与正连流混
pub const REG_PORT: u16 = 0;

/// 反连服务器腿：QUIC 监听（UDP 62694）；认领注册连接后绑本机 TCP
/// （127.0.0.1:9022），每个入站 TCP → 沿注册连接开流（写端口头 =
/// target_port，即手机侧 na sshd 8024）→ splice。连接死 = 收 TCP
/// 监听器回 QUIC 认领循环（9022 让位给 ssh 伴生兜底——口随供应商走，
/// 与今天 sshd 持有它的语义同构）。同时只认领一条（多设备是未来事，
/// 新注册挤掉旧的：旧手机掉线残留不会让新手机认领不上）
pub async fn run_rev_server(
    bind: SocketAddr,
    cfg: ServerConfig,
    psk: Option<[u8; 32]>,
    tcp_bind: SocketAddr,
    target_port: u16,
) -> std::io::Result<()> {
    let ep = Endpoint::server(cfg, bind).map_err(|e| std::io::Error::other(e.to_string()))?;
    let fails = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        std::net::IpAddr,
        (u32, std::time::Instant),
    >::new()));
    loop {
        // —— 认领循环：等一条验过客户端证的反连连接 ——
        let conn = loop {
            let Some(inc) = ep.accept().await else {
                return Err(std::io::Error::other("QUIC endpoint 关闭"));
            };
            let Ok(conn) = inc.await else { continue };
            let ip = conn.remote_address().ip();
            eprintln!("[na-quic] 反连连接 {ip}");
            {
                let g = fails.lock().await;
                if let Some(&(n, t0)) = g.get(&ip)
                    && ban_verdict(n, t0.elapsed().as_secs())
                {
                    eprintln!("[na-quic] 封禁中拒连 {ip}（第 {n} 次连败）");
                    conn.close(1u32.into(), b"auth banned");
                    continue;
                }
            }
            // 注册流：第一条流必须是 REG_PORT + 合法标签
            let registered = async {
                let (_send, mut recv) = conn.accept_bi().await.ok()?;
                let mut hdr = [0u8; 2];
                recv.read_exact(&mut hdr).await.ok()?;
                let port = parse_port_header(&hdr);
                if port != REG_PORT {
                    return None;
                }
                if let Some(k) = psk {
                    let mut tag = [0u8; AUTH_TAG_LEN];
                    let ok =
                        recv.read_exact(&mut tag).await.is_ok() && ct_eq(&tag, &auth_tag(&k, port));
                    if !ok {
                        return None;
                    }
                }
                Some(())
            }
            .await;
            if registered.is_none() {
                let mut g = fails.lock().await;
                let e = g.entry(ip).or_insert((0, std::time::Instant::now()));
                if e.1.elapsed().as_secs() >= AUTH_BAN_SECS {
                    *e = (0, std::time::Instant::now());
                }
                e.0 += 1;
                eprintln!("[na-quic] 反连注册验签失败 {ip}（第 {} 次连败）", e.0);
                conn.close(1u32.into(), b"bad registration");
                continue;
            }
            eprintln!("[na-quic] 反连认领 {ip} → 本机 TCP {tcp_bind}");
            break conn;
        };
        // —— 服务循环：TCP 桥到注册连接，连接死即收（让位兜底）——
        serve_rev_tcp(&conn, tcp_bind, target_port).await;
        eprintln!("[na-quic] 反连连接死，TCP {tcp_bind} 让位");
    }
}

/// TCP 服务段（run_rev_server 私有）：注册连接存活期内绑 tcp_bind，
/// 每个入站 → 开流写 target_port 端口头 → splice；连接死返回
async fn serve_rev_tcp(conn: &Connection, tcp_bind: SocketAddr, target_port: u16) {
    let Ok(listener) = tokio::net::TcpListener::bind(tcp_bind).await else {
        return; // 口被占（ssh 伴生兜底在场？）——等下轮认领再说
    };
    loop {
        tokio::select! {
            a = listener.accept() => {
                let Ok((tcp, _)) = a else { return };
                let Ok((mut send, recv)) = conn.open_bi().await else { return };
                tokio::spawn(async move {
                    if send.write_all(&port_header(target_port)).await.is_err() {
                        return;
                    }
                    splice(tcp, send, recv).await;
                });
            }
            _ = conn.closed() => return,
        }
    }
}

/// 反连客户端腿（手机侧，核内线程）：拨出到服务器 UDP 62694，先发
/// 注册流（REG_PORT + psk 标签），随后 accept_bi 循环——服务器每开
/// 一条流 = 9022 那边来了一个连接，读端口头回联本机 127.0.0.1:{port}
/// （na sshd 8024）→ splice。连接死 = 本函数返回（看门狗外侧重建，
/// 与正连腿同款事件驱动）
pub async fn run_rev_client(
    server: SocketAddr,
    sni: &str,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
) -> std::io::Result<()> {
    let mut ep = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    ep.set_default_client_config(cfg);
    let conn: Connection = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        ep.connect(server, sni)
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    )
    .await
    .map_err(|_| std::io::Error::other("QUIC 反连握手超时（UDP 黑洞？）"))?
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    // 注册流：验客户端证（服务器不验不认领，设计 §四）
    let (mut send, _recv) = conn
        .open_bi()
        .await
        .map_err(|e| std::io::Error::other(format!("注册流开不出: {e}")))?;
    send.write_all(&port_header(REG_PORT))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Some(k) = psk {
        send.write_all(&auth_tag(&k, REG_PORT))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    let _ = send.finish();
    loop {
        let (_send2, mut recv) = match conn.accept_bi().await {
            Ok(s) => s,
            Err(e) => return Err(std::io::Error::other(format!("QUIC 反连连接已死: {e}"))),
        };
        tokio::spawn(async move {
            let mut hdr = [0u8; 2];
            if recv.read_exact(&mut hdr).await.is_err() {
                return;
            }
            let port = parse_port_header(&hdr);
            if port == REG_PORT {
                return; // 注册口不是业务口
            }
            let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
            let Ok(tcp) = tokio::net::TcpStream::connect(target).await else {
                return;
            };
            splice(tcp, _send2, recv).await;
        });
    }
}
