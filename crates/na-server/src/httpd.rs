//! httpd.rs — na-server 的平面 HTTP 面（非 WS 升级请求）
//!
//! 就六个面：POST /api/na-report（含 /kfmv4 前缀别名）、GET /api/na/health、
//! GET /api/na/sys（环境体征，na-sys 采集）、GET /api/fs/list、
//! GET /api/fs/read（文件树数据面，逻辑在 na-protocol::fsapi）、其余 404。
//! 响应构造是纯函数（A 档），IO 只是它的搬运工（fs 面的 IO 在 main.rs
//! 的 spawn_blocking 里跑——current_thread 运行时上不许同步 fs）。

use std::io::Write as _;

use na_protocol::fsapi;

/// 环境体征 JSON（A 档纯函数：形状的唯一事实源，服务卡消费）：
/// {"load":[l1,l5,l15]|null,"procs":[running,total]|null,
///  "mem_total_kb":N|null,"mem_avail_kb":N|null,
///  "swap_total_kb":N|null,"swap_free_kb":N|null,
///  "disk_total_b":N|null,"disk_avail_b":N|null,"uptime_s":N|null}
/// 键永远在（客户端凭键认版本），采不到的路 = null 显形不编造；
/// 旧版 na-server 缺新键 → 客户端解析成 None → 卡面「—」，
/// 契约向旧兼容不破（2026-09-20 三路扩：进程/交换/在线）
pub fn sys_json(info: &na_sys::SysInfo) -> String {
    serde_json::json!({
        "load": info.load.map(|l| [l.l1, l.l5, l.l15]),
        "procs": info.load.and_then(|l| l.procs.map(|p| [p.0, p.1])),
        "mem_total_kb": info.mem.map(|m| m.total_kb),
        "mem_avail_kb": info.mem.map(|m| m.avail_kb),
        "swap_total_kb": info.mem.and_then(|m| m.swap.map(|s| s.0)),
        "swap_free_kb": info.mem.and_then(|m| m.swap.map(|s| s.1)),
        "disk_total_b": info.disk.map(|d| d.0),
        "disk_avail_b": info.disk.map(|d| d.1),
        "uptime_s": info.uptime_s,
        "cores": info.cores,
    })
    .to_string()
}

/// 日志落盘路径（env NA_REPORT_LOG 可改；缺省与 kfmv4 files.ts:378 同路径）
pub fn report_log_path() -> String {
    std::env::var("NA_REPORT_LOG").unwrap_or_else(|_| "/root/kfm-na/field-reports.log".into())
}

/// 构造 HTTP/1.1 响应（A 档纯函数）
pub fn respond(status: u16, reason: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// 路由结果（A 档纯函数）：(动作, 响应)
pub enum Route {
    Report,
    Health,
    Sys,
    /// GET /api/fs/list?dir=<相对路径>（dir 缺省 = 空串 = 允许根本身）
    FsList {
        dir: String,
    },
    /// GET /api/fs/read?path=<相对路径>&max=<字节>（max 缺省 64KB、上限 1MB）
    FsRead {
        path: String,
        max: usize,
    },
    /// /agent 前缀反代（BAR-163，工单⑥ A：手机经既有 9021 隧道直达
    /// na-agentd，不开新口）——携带剥前缀后的上游路径
    Agent {
        upstream: String,
    },
    NotFound,
}

/// /agent 反代的上游路径（A 档纯函数）：`/agent/api/agent/...` →
/// `/api/agent/...`。只放剥完仍以 /api/ 起头的（反代面只许吃 agentd
/// 的公开 API 面，/agent/ 后面乱写的 = None 落 404）；query 原样携带
pub fn agent_upstream(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/agent")?;
    // "/agent" 裸路径 = 上游根"/" 也放行（agentd 404 自己答）；
    // 非 /api/ 起头的上游路径不放行（反代不是任意口转发器）
    if rest.is_empty() {
        return Some("/".to_string());
    }
    rest.starts_with("/api/").then(|| rest.to_string())
}

/// 路径归一化：/kfmv4 前缀别名折叠（na 客户端现网 POST 的是
/// /kfmv4/api/na-report——经 kfmv4 时靠前缀路由，直连 na-server 时两边都认）
///
/// **切 query 与折前缀的先后**：先切 query、再折前缀。query 不是路径的一部分，
/// 折前缀只许作用于路径段——否则 query 里的 `/kfmv4` 字样会参与前缀判定；
/// 两条 fs 面正是靠 query 传参（dir/path/max），切错 = 参数全丢落 404。
/// 反代面例外：那一路的上游路径**连 query 原样带走**（agentd 自己切），
/// 所以 agent_upstream 吃的是未切的整串。
pub fn route(method: &str, path: &str) -> Route {
    // /agent 前缀 = 反代面（方法照传——GET/POST 都可能是 agentd 的面）
    if let Some(upstream) = agent_upstream(path) {
        return Route::Agent { upstream };
    }
    let (p, query) = match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path, ""),
    };
    let p = p.strip_prefix("/kfmv4").unwrap_or(p);
    match (method, p) {
        ("POST", "/api/na-report") => Route::Report,
        ("GET", "/api/na/health") => Route::Health,
        ("GET", "/api/na/sys") => Route::Sys,
        ("GET", "/api/fs/list") => Route::FsList {
            dir: fsapi::query_get(query, "dir").unwrap_or_default(),
        },
        ("GET", "/api/fs/read") => Route::FsRead {
            path: fsapi::query_get(query, "path").unwrap_or_default(),
            max: fsapi::parse_max(query),
        },
        _ => Route::NotFound,
    }
}

/// fs 面失败 → HTTP 响应（A 档纯函数）：NotFound 与 NotDir **同一条 404
/// 同文案**（越界/类型不符/不存在不许互相区分——不透露存在性）；Io = 500
/// 显形内部故障（500 本身已说明不是「没有」）。
pub fn fs_error_response(e: &fsapi::FsError) -> Vec<u8> {
    match e {
        fsapi::FsError::NotFound | fsapi::FsError::NotDir => {
            respond(404, "Not Found", "{\"ok\":false,\"error\":\"not found\"}")
        }
        fsapi::FsError::Io(detail) => respond(500, "Internal Server Error", &error_body(detail)),
    }
}

/// 通用失败体（A 档纯函数）：`{"ok":false,"error":…}`——键序与 404 手写体
/// 一致（json! 走 BTreeMap 会把 error 排到 ok 前面，形状不一观感乱）；
/// detail 走 serde 转义，含引号/换行也不破 JSON
pub fn error_body(detail: &str) -> String {
    format!(
        "{{\"ok\":false,\"error\":{}}}",
        serde_json::Value::String(detail.to_string())
    )
}

/// 反代失败诚实报错体（A 档纯函数）：502 = 上游 9041 不可达/转发失败，
/// 不许静默 404 不许空体——手机端卡面要显形「agentd 挂了」不是「没有这条线」
pub fn agent_error_body(detail: &str) -> String {
    serde_json::json!({
        "ok": false,
        "error": format!("agent 反代失败: {detail}"),
    })
    .to_string()
}

/// na-report：body 原样 append 一行（与 kfmv4 同行为：落盘即收，不解析）
pub fn append_report(body: &str) -> Result<(), String> {
    let path = report_log_path();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("打开 {path} 失败: {e}"))?;
    writeln!(f, "{body}").map_err(|e| format!("写 {path} 失败: {e}"))?;
    Ok(())
}

/// 解析请求头（A 档纯函数）：(method, path, content_length)
/// 输入是 \r\n\r\n 之前的头区文本；畸形即 Err。
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
