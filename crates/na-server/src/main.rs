//! main.rs — na-server 入口：监听 / 分流 / idle 自退
//!
//! 配置全走环境变量（主体拉起制：拉起命令由 na 给出，没有配置文件）：
//! - NA_BIND            监听地址（缺省 127.0.0.1:9021，只准回环）
//! - NA_REPORT_LOG      na-report 落盘路径（缺省 /root/kfm-na/field-reports.log）
//! - NA_IDLE_EXIT_SECS  无连接无会话持续 N 秒自退（缺省 1800，0 = 永不）
//!
//! 分流：peek 请求头不消费——见 Upgrade: websocket 交 wsterm（tokio-tungstenite
//! 从头自读），否则按平面 HTTP 处理（httpd）。

use na_server::httpd;
use na_server::state::Registry;
use na_server::wsterm;

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn bind_addr() -> String {
    std::env::var("NA_BIND").unwrap_or_else(|_| "127.0.0.1:9021".into())
}

fn idle_exit_secs() -> u64 {
    std::env::var("NA_IDLE_EXIT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
}

/// 只绑回环的硬闸（安全语义 = 8021 同款：入口只有 SSH 隧道）
fn assert_loopback(addr: &str) {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    assert!(
        host == "127.0.0.1" || host == "localhost" || host == "::1",
        "NA_BIND 只准回环地址，收到: {addr}"
    );
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let addr = bind_addr();
    assert_loopback(&addr);
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("绑 {addr} 失败: {e}"));
    eprintln!("[na-server] 听 {addr}（idle 自退 {}s）", idle_exit_secs());

    let registry = Arc::new(Registry::new());

    // idle 自退：无连接无会话持续超时 → 退出（下次 na 连接重新拉起）
    {
        let reg = Arc::clone(&registry);
        tokio::spawn(async move {
            let limit = idle_exit_secs();
            if limit == 0 {
                return; // 0 = 永不自退（考题/调试用）
            }
            let mut clock = tokio::time::interval(Duration::from_secs(60));
            loop {
                clock.tick().await;
                if reg.uptime_s() >= limit && reg.conn_count() == 0 && reg.session_count() == 0 {
                    eprintln!("[na-server] idle {limit}s 无连接无会话，自退");
                    std::process::exit(0);
                }
            }
        });
    }

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[na-server] accept 失败: {e}");
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let reg = Arc::clone(&registry);
        tokio::spawn(async move {
            if let Err(e) = dispatch(stream, reg).await {
                eprintln!("[na-server] 连接 {peer} 处理失败: {e}");
            }
        });
    }
}

/// 连接分流：peek 头区 → WS 升级 or 平面 HTTP
async fn dispatch(stream: TcpStream, registry: Arc<Registry>) -> Result<(), String> {
    let head = peek_head(&stream).await?;
    if head
        .lines()
        .any(|l| l.eq_ignore_ascii_case("upgrade: websocket"))
    {
        wsterm::handle(stream, registry).await;
        return Ok(());
    }
    http_handle(stream, &head, registry).await
}

/// peek 到 \r\n\r\n 为止（不消费；上限 16KB / 10s）
async fn peek_head(stream: &TcpStream) -> Result<String, String> {
    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let n = tokio::time::timeout_at(deadline, stream.peek(&mut buf))
            .await
            .map_err(|_| "peek 头区超时".to_string())?
            .map_err(|e| format!("peek 失败: {e}"))?;
        if n == 0 {
            return Err("对端在头区前断开".into());
        }
        if let Some(pos) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            return String::from_utf8(buf[..pos].to_vec()).map_err(|_| "头区非 UTF-8".to_string());
        }
        if n >= 16384 {
            return Err("头区超 16KB".into());
        }
        if n == buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
        // 头未到齐，让出等更多字节
        tokio::task::yield_now().await;
    }
}

/// 平面 HTTP：读体 → 路由 → 响应
async fn http_handle(
    mut stream: TcpStream,
    head: &str,
    registry: Arc<Registry>,
) -> Result<(), String> {
    let (method, path, content_length) = httpd::parse_head(head)?;
    let head_len = head.len() + 4; // + \r\n\r\n
    let mut raw = vec![0u8; head_len + content_length];
    stream
        .read_exact(&mut raw)
        .await
        .map_err(|e| format!("读请求失败: {e}"))?;
    let body = String::from_utf8_lossy(&raw[head_len..]).into_owned();

    let resp = match httpd::route(&method, &path) {
        httpd::Route::Report => match httpd::append_report(&body) {
            Ok(()) => httpd::respond(200, "OK", "{\"ok\":true}"),
            Err(e) => httpd::respond(
                500,
                "Internal Server Error",
                &format!("{{\"ok\":false,\"error\":\"{e}\"}}"),
            ),
        },
        httpd::Route::Health => httpd::respond(200, "OK", &registry.health_json()),
        httpd::Route::Sys => {
            // collect 永不失败——单路采不到归该路 null 显形
            httpd::respond(200, "OK", &httpd::sys_json(&na_sys::collect("/")))
        }
        httpd::Route::NotFound => {
            httpd::respond(404, "Not Found", "{\"ok\":false,\"error\":\"not found\"}")
        }
    };
    stream
        .write_all(&resp)
        .await
        .map_err(|e| format!("写响应失败: {e}"))?;
    Ok(())
}
