//! main.rs — na-agentd 入口：127.0.0.1:9041 本地 HTTP daemon（形制照
//! na-server main.rs：只绑回环硬闸 + 线程每连接 + 纯函数路由）。
//!
//! 配置全走环境变量：
//! - NA_AGENT_BIND          监听地址（缺省 127.0.0.1:9041，只准回环。
//!   9041 端口对账 2026-09-26：现役 8021-8032/9021/9022/9099/9229/
//!   62633/62694（ss -tln 实证），9041 空闲）
//! - NA_AGENT_SESSION_ROOT  会话根（缺省 /root/.kfm/session）
//! - NA_AGENT_PROVIDER_JSON provider 配置（缺省 /root/.kfm/provider.json，
//!   key 永不进日志正文——错误信息只报路径不报内容）

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use na_agentd::httpd;
use na_agentd::service::AgentService;

/// 缺省监听口（2026-09-26 端口对账见上注）
pub const DEFAULT_BIND: &str = "127.0.0.1:9041";
pub const DEFAULT_SESSION_ROOT: &str = "/root/.kfm/session";
pub const DEFAULT_PROVIDER_JSON: &str = "/root/.kfm/provider.json";

/// 只绑回环的硬闸（与 na-server 同款安全语义：入口只有 SSH/本机）
fn assert_loopback(addr: &str) {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    assert!(
        host == "127.0.0.1" || host == "localhost" || host == "::1",
        "NA_AGENT_BIND 只准回环地址，收到: {addr}"
    );
}

fn main() {
    let addr = std::env::var("NA_AGENT_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    assert_loopback(&addr);
    let svc = Arc::new(AgentService::new(
        &std::env::var("NA_AGENT_SESSION_ROOT").unwrap_or_else(|_| DEFAULT_SESSION_ROOT.into()),
        &std::env::var("NA_AGENT_PROVIDER_JSON").unwrap_or_else(|_| DEFAULT_PROVIDER_JSON.into()),
    ));
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| panic!("绑 {addr} 失败: {e}"));
    eprintln!(
        "[na-agentd] 听 {addr}（会话根 {} / provider {}）",
        svc.session_root, svc.provider_json
    );
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let svc = Arc::clone(&svc);
                std::thread::spawn(move || {
                    if let Err(e) = handle(s, &svc) {
                        eprintln!("[na-agentd] 连接处理失败: {e}");
                    }
                });
            }
            Err(e) => eprintln!("[na-agentd] accept 失败: {e}"),
        }
    }
}

fn handle(mut stream: TcpStream, svc: &AgentService) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    // 读头到 \r\n\r\n（上限 16KB）
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream
            .read(&mut buf)
            .map_err(|e| format!("读请求失败: {e}"))?;
        if n == 0 {
            return Err("对端在头区前断开".into());
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > 16384 {
            return Err("头区超 16KB".into());
        }
    };
    let head = String::from_utf8(raw[..head_end].to_vec()).map_err(|_| "头区非 UTF-8")?;
    let (method, path, content_length) = httpd::parse_head(&head)?;
    let mut body = raw[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream
            .read(&mut buf)
            .map_err(|e| format!("读请求体失败: {e}"))?;
        if n == 0 {
            return Err("请求体提前 EOF".into());
        }
        body.extend_from_slice(&buf[..n]);
        if body.len() > 1024 * 1024 {
            return Err("请求体超 1MB".into());
        }
    }
    body.truncate(content_length);
    let body = String::from_utf8_lossy(&body).into_owned();

    let resp = route_exec(&method, &path, &body, svc);
    stream
        .write_all(&resp)
        .map_err(|e| format!("写响应失败: {e}"))?;
    Ok(())
}

fn route_exec(method: &str, path: &str, body: &str, svc: &AgentService) -> Vec<u8> {
    let ok = |v: serde_json::Value| httpd::respond(200, "OK", &v.to_string());
    let err = |status: u16, reason: &str, msg: &str| {
        httpd::respond(
            status,
            reason,
            &serde_json::json!({"ok": false, "error": msg}).to_string(),
        )
    };
    match httpd::route(method, path) {
        httpd::Route::Health => ok(serde_json::json!({
            "ok": true,
            "service": "na-agentd",
            "session_root": svc.session_root,
        })),
        httpd::Route::Lines => match svc.list_lines() {
            Ok(lines) => ok(serde_json::json!({"ok": true, "lines": lines})),
            Err(e) => err(500, "Internal Server Error", &e),
        },
        httpd::Route::Send { line } => {
            let msg = serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                });
            let Some(message) = msg else {
                return err(400, "Bad Request", "body 缺 message 字段");
            };
            match svc.send(&line, &message) {
                Ok(out) => ok(serde_json::json!({
                    "ok": true,
                    "reply": out.reply,
                    "session": out.session_path,
                })),
                Err(e) => err(500, "Internal Server Error", &e),
            }
        }
        httpd::Route::Tail { line, n } => match svc.tail(&line, n) {
            Ok(events) => ok(serde_json::json!({"ok": true, "events": events})),
            Err(e) => err(404, "Not Found", &e),
        },
        httpd::Route::NotFound => err(404, "Not Found", "not found"),
    }
}
