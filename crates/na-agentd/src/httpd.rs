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
    Send {
        line: String,
    },
    Tail {
        line: String,
        n: usize,
    },
    /// 线内会话列表（BAR-163 工单⑥ B：会话池条目面）
    Sessions {
        line: String,
    },
    /// 指定会话文件的尾部 n 事件（Tail 只吃最新会话，本面吃点名文件）
    SessionTail {
        line: String,
        name: String,
        n: usize,
    },
    /// 信箱信件列表（信箱 = 特殊路由）
    Letters,
    /// 信件正文
    Letter {
        name: String,
    },
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
            line: pct_decode(line),
        },
        ("GET", ["api", "agent", "lines", line, "tail"]) => Route::Tail {
            line: pct_decode(line),
            n: parse_n(query),
        },
        ("GET", ["api", "agent", "lines", line, "sessions"]) => Route::Sessions {
            line: pct_decode(line),
        },
        ("GET", ["api", "agent", "lines", line, "sessions", name, "tail"]) => Route::SessionTail {
            line: pct_decode(line),
            name: pct_decode(name),
            n: parse_n(query),
        },
        ("GET", ["api", "agent", "mailbox", "letters"]) => Route::Letters,
        ("GET", ["api", "agent", "mailbox", "letters", name]) => Route::Letter {
            name: pct_decode(name),
        },
        _ => Route::NotFound,
    }
}

/// 路径段百分号解码（BAR-163 live 实咬补：curl 等标准客户端把非 ASCII
/// 段编成 %XX，app 侧 sess_pool 发原样 UTF-8——两吃）。分段后逐段调用：
/// %2F 解码成 '/' 也只留在段内不变分隔符（下游闸 is_session_file/
/// valid_letter_name 照拒）；非法 % 序列原样保留
fn pct_decode(seg: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bs = seg.as_bytes();
    let mut out = Vec::with_capacity(bs.len());
    let mut i = 0;
    while i < bs.len() {
        if bs[i] == b'%'
            && i + 3 <= bs.len()
            && let (Some(h), Some(l)) = (hex(bs[i + 1]), hex(bs[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bs[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
