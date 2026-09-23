//! quic_bridge — QUIC 桥客户端（运维/冒烟工具）：
//! 本机 TCP 监听 ←QUIC→ 服务器 na-server QUIC 腿 → 回联目标口。
//!
//! 用法：
//!   quic_bridge <server:port> <sni> <local_addr> <target_port> <pin_hex> <psk_hex>
//! 例（服务器本机自环冒烟）：
//!   quic_bridge 127.0.0.1:62633 kfm-na 127.0.0.1:19021 9021 <pin> <psk>
//! 然后 curl 127.0.0.1:19021/api/na/health ——经桥两跳回还即绿。

use std::net::SocketAddr;

fn parse_hex32(h: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    assert_eq!(h.len(), 64, "必须 64 位 hex");
    for (i, c) in h.as_bytes().chunks_exact(2).enumerate() {
        let hi = (c[0] as char).to_digit(16).expect("hex");
        let lo = (c[1] as char).to_digit(16).expect("hex");
        out[i] = ((hi << 4) | lo) as u8;
    }
    out
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 7 {
        eprintln!(
            "用法: quic_bridge <server:port> <sni> <local_addr> <target_port> <pin_hex> <psk_hex>"
        );
        std::process::exit(2);
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let server: SocketAddr = a[1].parse().expect("服务器地址");
    let local: SocketAddr = a[3].parse().expect("本机监听地址");
    let target: u16 = a[4].parse().expect("目标端口");
    let pin = parse_hex32(&a[5]);
    let psk = parse_hex32(&a[6]);
    eprintln!("[quic_bridge] {local} ←QUIC→ {server} → 回联 :{target}");
    if let Err(e) = na_quic::run_client(
        server,
        &a[2],
        local,
        target,
        na_quic::client_config(pin),
        Some(psk),
    )
    .await
    {
        eprintln!("[quic_bridge] 腿死: {e}");
        std::process::exit(1);
    }
}
