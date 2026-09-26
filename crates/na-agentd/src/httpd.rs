//! httpd.rs — na-agentd 的平面 HTTP 面（形制照 na-server httpd.rs：
//! 路由/响应构造是纯函数（A 档），IO 只是搬运工）。
//!
//! 四面：GET /api/agent/health、GET /api/agent/lines、
//! POST /api/agent/lines/<线>/send、GET /api/agent/lines/<线>/tail?n=N。

/// 构造 HTTP/1.1 响应（A 档纯函数）
pub fn respond(status: u16, reason: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// 路由结果（A 档纯函数）
pub enum Route {
    Health,
    Lines,
    Send { line: String },
    Tail { line: String, n: usize },
    NotFound,
}

pub fn route(method: &str, path: &str) -> Route {
    let (p, query) = match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path, ""),
    };
    let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    match (method, segs.as_slice()) {
        ("GET", ["api", "agent", "health"]) => Route::Health,
        ("GET", ["api", "agent", "lines"]) => Route::Lines,
        ("POST", ["api", "agent", "lines", line, "send"]) => Route::Send {
            line: line.to_string(),
        },
        ("GET", ["api", "agent", "lines", line, "tail"]) => Route::Tail {
            line: line.to_string(),
            n: parse_n(query),
        },
        _ => Route::NotFound,
    }
}

/// tail 的 ?n=N（缺省/坏值 = 50，上限 500）
fn parse_n(query: &str) -> usize {
    for kv in query.split('&') {
        if let Some(v) = kv.strip_prefix("n=")
            && let Ok(n) = v.parse::<usize>()
        {
            return n.clamp(1, 500);
        }
    }
    50
}

/// 解析请求头（A 档纯函数）：(method, path, content_length)
pub fn parse_head(head: &str) -> Result<(String, String, usize), String> {
    let mut lines = head.split("\r\n");
    let req = lines.next().ok_or("空请求行")?;
    let mut parts = req.split_whitespace();
    let method = parts.next().ok_or("缺 method")?.to_string();
    let path = parts.next().ok_or("缺 path")?.to_string();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = v
                .trim()
                .parse()
                .map_err(|_| "Content-Length 非数字".to_string())?;
        }
    }
    Ok((method, path, content_length))
}
