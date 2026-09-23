//! quic_echo — M2 迁移考题载体（docs/active/quic隧道.md §六）
//!
//! 四种形态：
//!   quic_echo server <bind-addr>          QUIC echo 服务
//!   quic_echo client <server-addr> <n>    QUIC ping-pong n 轮（100ms 一拍）
//!   quic_echo tcpserver <bind-addr>       TCP echo 服务（反例对照）
//!   quic_echo tcpclient <addr> <n>        TCP ping-pong（NAT 换映射后必死）
//!
//! 判卷：client 逐行打印 `echo NNNN`，退出码 0 = 全通；任何断连 = 非零。
//! NAT 重映射（conntrack 清空+snat 换源）后：QUIC 客户端应无损跑完（迁移），
//! TCP 客户端应失败（反例——证明测试环境真的在模拟运营商断链）。

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use quinn::{ClientConfig, Endpoint, ServerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

fn insecure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerify))
        .with_no_client_auth();
    ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ))
}

fn server_config() -> ServerConfig {
    let cert = rcgen::generate_simple_self_signed(vec!["na-quic-spike".into()]).unwrap();
    let certs = vec![cert.cert.der().clone()];
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
    let mut sc = ServerConfig::with_single_cert(certs, key).unwrap();
    Arc::get_mut(&mut sc.transport)
        .unwrap()
        .max_concurrent_bidi_streams(64u32.into())
        // 4h：省电冻结期连接状态保活的代价 ≈ 一条 CID 表项（设计 §五）
        .max_idle_timeout(Some(Duration::from_secs(4 * 3600).try_into().unwrap()));
    sc
}

async fn run_server(bind: SocketAddr) -> std::io::Result<()> {
    let ep = Endpoint::server(server_config(), bind)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    eprintln!("[quic_echo] QUIC server on {bind}");
    while let Some(inc) = ep.accept().await {
        tokio::spawn(async move {
            let Ok(conn) = inc.await else { return };
            loop {
                let Ok((mut send, mut recv)) = conn.accept_bi().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 64 * 1024];
                    loop {
                        match recv.read(&mut buf).await {
                            Ok(Some(n)) if n > 0 => {
                                if send.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                    let _ = send.finish();
                });
            }
        });
    }
    Ok(())
}

async fn run_client(addr: SocketAddr, n: u32) -> std::io::Result<()> {
    let mut ep = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    ep.set_default_client_config(insecure_client());
    let conn = ep
        .connect(addr, "na-quic-spike")
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    for i in 0..n {
        let line = format!("{i:04}\n");
        send.write_all(line.as_bytes())
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut buf = vec![0u8; line.len()];
        recv.read_exact(&mut buf)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if buf != line.as_bytes() {
            return Err(std::io::Error::other("echo 逐字节校验失败"));
        }
        println!("echo {i:04}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    send.finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    conn.close(0u32.into(), b"done");
    Ok(())
}

async fn run_tcp_server(bind: SocketAddr) -> std::io::Result<()> {
    let l = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("[quic_echo] TCP server on {bind}");
    loop {
        let (mut s, _) = l.accept().await?;
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

async fn run_tcp_client(addr: SocketAddr, n: u32) -> std::io::Result<()> {
    let mut s = tokio::net::TcpStream::connect(addr).await?;
    for i in 0..n {
        let line = format!("{i:04}\n");
        s.write_all(line.as_bytes()).await?;
        let mut buf = vec![0u8; line.len()];
        s.read_exact(&mut buf).await?;
        if buf != line.as_bytes() {
            return Err(std::io::Error::other("echo 逐字节校验失败"));
        }
        println!("echo {i:04}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    let r = match mode {
        "server" | "tcpserver" => {
            let bind: SocketAddr = args[2].parse().expect("bind addr");
            if mode == "server" {
                run_server(bind).await
            } else {
                run_tcp_server(bind).await
            }
        }
        "client" | "tcpclient" => {
            let addr: SocketAddr = args[2].parse().expect("server addr");
            let n: u32 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(60);
            if mode == "client" {
                run_client(addr, n).await
            } else {
                run_tcp_client(addr, n).await
            }
        }
        _ => {
            eprintln!("用法: quic_echo server|client|tcpserver|tcpclient <addr> [n]");
            std::process::exit(64);
        }
    };
    if let Err(e) = r {
        eprintln!("[quic_echo] {mode} 失败: {e}");
        std::process::exit(1);
    }
}
