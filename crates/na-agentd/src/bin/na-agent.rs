//! na-agent — CLI（na-agentd 的第二 bin）：打本机 daemon 的 HTTP 面。
//!
//!   na-agent lines                  列线（花名册 = 列目录）
//!   na-agent send <线> <消息>       发消息（同步跑到 stop 再返回）
//!   na-agent tail <线> [n]          看最新会话尾部 n 事件（缺省 20）
//!
//! 走 HTTP 打 127.0.0.1:9041（NA_AGENT_ADDR 可改）；纯 std TcpStream，
//! 回环明文（与 na-server 同款安全语义：入口只有 SSH/本机）。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const DEFAULT_ADDR: &str = "127.0.0.1:9041";

fn usage() -> ! {
    eprintln!(
        "na-agent — na agent CLI\n\
         用法:\n\
         \x20 na-agent lines\n\
         \x20 na-agent send <线> <消息>\n\
         \x20 na-agent tail <线> [n]"
    );
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (method, path, body) = match args.first().map(String::as_str) {
        Some("lines") if args.len() == 1 => ("GET", "/api/agent/lines".to_string(), None),
        Some("send") if args.len() == 3 => (
            "POST",
            format!("/api/agent/lines/{}/send", args[1]),
            Some(serde_json::json!({"message": args[2]}).to_string()),
        ),
        Some("tail") if args.len() == 2 || args.len() == 3 => {
            let n = args.get(2).map(String::as_str).unwrap_or("20");
            (
                "GET",
                format!("/api/agent/lines/{}/tail?n={n}", args[1]),
                None,
            )
        }
        _ => usage(),
    };
    let addr = std::env::var("NA_AGENT_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.into());
    match call(&addr, method, &path, body.as_deref()) {
        Ok((status, text)) => {
            println!("{text}");
            if status != 200 {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("na-agent: {e}");
            std::process::exit(1);
        }
    }
}

/// 一发一收（Connection: close）。返回 (status, body)。
fn call(addr: &str, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect(addr).map_err(|e| format!("连 {addr} 失败: {e}"))?;
    // send 是同步跑到 stop 的慢口：读超时不设上限心跳，给足模型时间
    s.set_read_timeout(Some(Duration::from_secs(3600)))
        .map_err(|e| e.to_string())?;
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("写请求失败: {e}"))?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)
        .map_err(|e| format!("读响应失败: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse::<u16>().ok())
        .ok_or("坏响应状态行")?;
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or("")
        .to_string();
    Ok((status, body))
}
