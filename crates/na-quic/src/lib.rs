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

/// 客户端保活：10s 一 ping 保 NAT 映射（设计 §五）
pub const KEEPALIVE: Duration = Duration::from_secs(10);

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
    let mut sc = ServerConfig::with_single_cert(certs, key).expect("证书装载");
    Arc::get_mut(&mut sc.transport)
        .expect("transport 独占")
        .max_concurrent_bidi_streams(128u32.into())
        .max_idle_timeout(Some(IDLE_TIMEOUT.try_into().expect("idle 上限")));
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
        t.max_idle_timeout(Some(IDLE_TIMEOUT.try_into().expect("idle 上限")));
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
    let conn: Connection = ep
        .connect(server, sni)
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .await
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
