//! httpd.rs — na-server 的平面 HTTP 面（非 WS 升级请求）
//!
//! 就四个面：POST /api/na-report（含 /kfmv4 前缀别名）、GET /api/na/health、
//! GET /api/na/sys（环境体征，na-sys 采集）、其余 404。响应构造是纯函数
//! （A 档），IO 只是它的搬运工。

use std::io::Write as _;

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
    NotFound,
}

/// 路径归一化：/kfmv4 前缀别名折叠（na 客户端现网 POST 的是
/// /kfmv4/api/na-report——经 kfmv4 时靠前缀路由，直连 na-server 时两边都认）
pub fn route(method: &str, path: &str) -> Route {
    let p = path.strip_prefix("/kfmv4").unwrap_or(path);
    match (method, p) {
        ("POST", "/api/na-report") => Route::Report,
        ("GET", "/api/na/health") => Route::Health,
        ("GET", "/api/na/sys") => Route::Sys,
        _ => Route::NotFound,
    }
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
