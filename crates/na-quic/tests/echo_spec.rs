//! M1 spike：quinn 双端可编 + echo 双通（docs/active/quic隧道.md §八 M1）
//! 服务器自签证书 + 客户端跳过 CA 验证（spike 专用——v1 上指纹 pinning，
//! 见设计 §四）。后续 netns 迁移考题（M2）在同文件上长。

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use quinn::{ClientConfig, Endpoint, ServerConfig};

/// spike 自签证书：服务器端点用（rcgen 现造，不落盘）
fn self_signed() -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let cert = rcgen::generate_simple_self_signed(vec!["na-quic-spike".into()]).unwrap();
    (
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into()),
    )
}

/// spike 客户端配置：跳过验证（v1 换指纹 pinning——设计 §四）
fn insecure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerify))
        .with_no_client_auth();
    ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ))
}

#[derive(Debug)]
struct SkipVerify;

impl rustls::client::danger::ServerCertVerifier for SkipVerify {
    fn verify_server_cert(
        &self,
        _e: &rustls::pki_types::CertificateDer<'_>,
        _i: &[rustls::pki_types::CertificateDer<'_>],
        _s: &rustls::pki_types::ServerName<'_>,
        _o: &[u8],
        _n: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
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

#[tokio::test]
async fn spec_m1_quic_echo_双通() {
    // 密码提供者显式装默认（quinn 默认 aws-lc-rs 与本测试 ring 并存时必装）
    let _ = rustls::crypto::ring::default_provider().install_default();
    // 服务器：自签 + 绑回环随机口，收一条流 echo
    let (certs, key) = self_signed();
    let mut sc = ServerConfig::with_single_cert(certs, key).unwrap();
    Arc::get_mut(&mut sc.transport)
        .unwrap()
        .max_concurrent_bidi_streams(64u32.into());
    let server = Endpoint::server(sc, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let addr = server.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        let conn = server.accept().await.unwrap().await.unwrap();
        let (mut send, mut recv) = conn.accept_bi().await.unwrap();
        let buf = recv.read_to_end(usize::MAX).await.unwrap();
        send.write_all(&buf).await.unwrap();
        send.finish().unwrap();
        conn.closed().await;
    });

    // 客户端：open_bi 写字 → 读回 echo → 断言相等
    let mut client_ep = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    client_ep.set_default_client_config(insecure_client());
    let conn = client_ep
        .connect(addr, "na-quic-spike")
        .unwrap()
        .await
        .unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let payload = "na-quic M1 echo：迁移前的第一声".as_bytes();
    send.write_all(payload).await.unwrap();
    send.finish().unwrap();
    let back = recv.read_to_end(usize::MAX).await.unwrap();
    assert_eq!(back, payload, "echo 必须逐字节回还");
    conn.close(0u32.into(), b"done");
    echo.await.unwrap();
    client_ep.wait_idle().await;
}
