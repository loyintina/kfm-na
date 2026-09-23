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

/// 服务器腿：QUIC 监听，每条入站流读 2 字节端口头 → 回联 127.0.0.1:{port}
/// → splice。连接级循环：连接死了 accept 出错返回（看门狗外侧重建）
pub async fn run_server(bind: SocketAddr, cfg: ServerConfig) -> std::io::Result<()> {
    let ep = Endpoint::server(cfg, bind).map_err(|e| std::io::Error::other(e.to_string()))?;
    while let Some(inc) = ep.accept().await {
        tokio::spawn(async move {
            let Ok(conn) = inc.await else { return };
            loop {
                let stream = conn.accept_bi().await;
                let Ok((send, mut recv)) = stream else { break };
                tokio::spawn(async move {
                    let mut hdr = [0u8; 2];
                    if recv.read_exact(&mut hdr).await.is_err() {
                        return;
                    }
                    let port = parse_port_header(&hdr);
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
/// 连接 → 开一条流（写端口头 = target_port）→ splice。连接死 = 本函数
/// 返回（看门狗外侧重建——死亡检测是事件驱动的，这就是 QUIC 腿比 ssh
/// 腿省掉探活三件套的原因）。target_port 与 local_bind 解耦：部署惯例
/// 双端同口（9021→9021），但桥接模型本身不绑这个约定
pub async fn run_client(
    server: SocketAddr,
    sni: &str,
    local_bind: SocketAddr,
    target_port: u16,
    cfg: ClientConfig,
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
            splice(tcp, send, recv).await;
        });
    }
}
