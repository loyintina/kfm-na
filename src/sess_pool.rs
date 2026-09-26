//! sess_pool.rs — 会话池数据源（设置页第三页「会话池」，BAR-163 一期只读）。
//!
//! 分层：上半 A 档纯核（agentd JSON 面解析/路由表/条目表/空态占位/
//! focus 钳制，tests/sess_pool_spec.rs 钉死）；下半 B 档取数胶水
//! （svc_health 同款：TcpStream + http1 手写客户端打隧道本地口
//! 127.0.0.1:{port}/agent/... 反代进 agentd，零新依赖）。
//!
//! 快照全局注册（svc_health/tunnel 同款）：涂装直读免穿 App plumbing；
//! 取回即 bump epoch + DIRTY 置位 → 壳脏帧重烘 + rebuild_cfg_rows。
//! 路径段带非 ASCII（会话名 0001-会话.jsonl）原样放行——na-server
//! 反代与 agentd 头解析两端都是自家码，全程 UTF-8 透传。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

// ---- A 档：数据形状与纯函数（考题先行钉死）----

/// 下池路由键：线 或 信箱（特殊路由）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteKey {
    Line(String),
    Mailbox,
}

/// 信箱路由标题（与 session/MAILBOX_DIR 同一面，UI 文案层）
pub const MAILBOX_TITLE: &str = "信箱";
/// 空态占位行文案
pub const EMPTY_SESSIONS: &str = "（无会话）";
pub const EMPTY_LETTERS: &str = "（无信件）";
/// 查看器一次渲染的尾部事件数
pub const TAIL_EVENTS: usize = 60;

/// 下池路由表：各线一行（meta = "线"）+ 信箱特殊路由收尾（meta = "信件"）
pub fn routes_of(lines: &[String]) -> Vec<(RouteKey, String, String)> {
    let mut rows: Vec<(RouteKey, String, String)> = lines
        .iter()
        .map(|l| (RouteKey::Line(l.clone()), l.clone(), "线".to_string()))
        .collect();
    rows.push((
        RouteKey::Mailbox,
        MAILBOX_TITLE.to_string(),
        "信件".to_string(),
    ));
    rows
}

/// 上池条目表（会话）：label = 会话名，value = 字节数格式化
pub fn session_entries(sessions: &[(String, u64)]) -> Vec<(String, String)> {
    sessions
        .iter()
        .map(|(name, bytes)| (name.clone(), fmt_bytes(*bytes)))
        .collect()
}

/// 上池条目表（信件）：label = 信名，value = 字节数格式化
pub fn letter_entries(letters: &[(String, u64)]) -> Vec<(String, String)> {
    session_entries(letters)
}

/// 空态占位：条目空 → 一行占位（label = 空态文案，value 空）
pub fn entries_or_placeholder(
    rows: Vec<(String, String)>,
    empty_word: &str,
) -> Vec<(String, String)> {
    if rows.is_empty() {
        vec![(empty_word.to_string(), String::new())]
    } else {
        rows
    }
}

/// focus 钳制：空表归 0，越界收回表尾
pub fn clamp_focus(focus: usize, len: usize) -> usize {
    if len == 0 { 0 } else { focus.min(len - 1) }
}

/// 字节数格式化（B/KB/MB 一档，取整）
pub fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

/// GET /api/agent/lines 响应 → 线名表
pub fn parse_lines(body: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 坏: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(err_detail(&v));
    }
    Ok(v.get("lines")
        .and_then(Value::as_array)
        .ok_or("缺 lines 字段")?
        .iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect())
}

/// sessions/letters 同形状：[{"name","bytes"}] → (名, 字节) 表
pub fn parse_named_bytes(body: &str, key: &str) -> Result<Vec<(String, u64)>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 坏: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(err_detail(&v));
    }
    Ok(v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("缺 {key} 字段"))?
        .iter()
        .filter_map(|x| {
            let name = x.get("name").and_then(Value::as_str)?.to_string();
            let bytes = x.get("bytes").and_then(Value::as_u64).unwrap_or(0);
            Some((name, bytes))
        })
        .collect())
}

/// tail 响应 → 原始 jsonl 行（交给 wire_render 渲染）
pub fn parse_events(body: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 坏: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(err_detail(&v));
    }
    Ok(v.get("events")
        .and_then(Value::as_array)
        .ok_or("缺 events 字段")?
        .iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect())
}

/// letter 响应 → 正文
pub fn parse_letter_content(body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 坏: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(err_detail(&v));
    }
    v.get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "缺 content 字段".to_string())
}

/// {"ok":false,"error":"..."} 的错误详情（兜底给整串）
fn err_detail(v: &Value) -> String {
    v.get("error")
        .and_then(Value::as_str)
        .unwrap_or("面回 ok:false")
        .to_string()
}

// ---- B 档：取数胶水（判卷 = redroid 实录：池页 vs curl 对表）----

/// 单次 GET 总超时（连/读同档）
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// body 上限（tail 60 事件 + 长结果也就几十 KB，防爆内存）
const BODY_CAP: usize = 256 * 1024;

/// 下池一行（路由）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRow {
    pub key: RouteKey,
    pub title: String,
    pub meta: String,
}

/// 上池一行（条目）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRow {
    pub label: String,
    pub value: String,
}

/// 查看器内容（取回即一包，epoch 区分同标题刷新）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub title: String,
    pub text: String,
    pub epoch: u64,
}

/// 会话池快照（涂装/行重建直读）
#[derive(Debug, Clone, Default)]
pub struct SessPoolSnap {
    pub routes: Vec<RouteRow>,
    pub entries: Vec<EntryRow>,
    pub selected: Option<RouteKey>,
    pub routes_loading: bool,
    pub entries_loading: bool,
    pub content: Option<Content>,
}

#[derive(Default)]
struct Inner {
    snap: SessPoolSnap,
    cfg_port: u16,
}

static INNER: OnceLock<Mutex<Inner>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(0);

fn inner() -> &'static Mutex<Inner> {
    INNER.get_or_init(|| Mutex::new(Inner::default()))
}

fn bump() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
    DIRTY.store(true, Ordering::Relaxed);
}

/// 配置（壳设置加载/重载时喂，svc_health::configure 旁同款）：隧道本地口
pub fn configure(local_port: u16) {
    let mut g = inner().lock().unwrap();
    g.cfg_port = local_port;
}

/// 读当前快照（rebuild_cfg_rows 每次拍一张；锁短）
pub fn snap() -> SessPoolSnap {
    inner().lock().unwrap().snap.clone()
}

/// 壳脏帧消耗口：有变化取走 true（每帧一查，零成本）
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

/// 刷下池路由表（开页/下拉刷新时调）：GET /agent/api/agent/lines
pub fn request_routes() {
    let port = {
        let mut g = inner().lock().unwrap();
        if g.snap.routes_loading {
            return;
        }
        g.snap.routes_loading = true;
        DIRTY.store(true, Ordering::Relaxed);
        g.cfg_port
    };
    std::thread::spawn(move || {
        let got = http_get(port, "/agent/api/agent/lines").and_then(|b| parse_lines(&b));
        let mut g = inner().lock().unwrap();
        g.snap.routes_loading = false;
        if let Ok(lines) = got {
            g.snap.routes = routes_of(&lines)
                .into_iter()
                .map(|(key, title, meta)| RouteRow { key, title, meta })
                .collect();
        }
        bump();
    });
}

/// 刷上池条目表（切路由时调）：线 → sessions 面；信箱 → letters 面
pub fn request_entries(key: RouteKey) {
    let port = {
        let mut g = inner().lock().unwrap();
        g.snap.selected = Some(key.clone());
        g.snap.entries_loading = true;
        g.snap.entries.clear();
        DIRTY.store(true, Ordering::Relaxed);
        g.cfg_port
    };
    std::thread::spawn(move || {
        let got = match &key {
            RouteKey::Line(line) => {
                http_get(port, &format!("/agent/api/agent/lines/{line}/sessions"))
                    .and_then(|b| parse_named_bytes(&b, "sessions"))
                    .map(|ss| entries_or_placeholder(session_entries(&ss), EMPTY_SESSIONS))
            }
            RouteKey::Mailbox => http_get(port, "/agent/api/agent/mailbox/letters")
                .and_then(|b| parse_named_bytes(&b, "letters"))
                .map(|ls| entries_or_placeholder(letter_entries(&ls), EMPTY_LETTERS)),
        };
        let mut g = inner().lock().unwrap();
        g.snap.entries_loading = false;
        if g.snap.selected.as_ref() == Some(&key)
            && let Ok(rows) = got
        {
            g.snap.entries = rows
                .into_iter()
                .map(|(label, value)| EntryRow { label, value })
                .collect();
        }
        bump();
    });
}

/// 取条目内容（点条目时调）：会话 → tail 面 + wire_render 渲染；
/// 信 → 正文直读。取回写入快照 content（壳脏帧喂查看器）。
pub fn request_content(key: RouteKey, name: &str) {
    let (port, title) = {
        let g = inner().lock().unwrap();
        (g.cfg_port, content_title(&key, name))
    };
    let name = name.to_string();
    std::thread::spawn(move || {
        let got: Result<String, String> = match &key {
            RouteKey::Line(line) => http_get(
                port,
                &format!("/agent/api/agent/lines/{line}/sessions/{name}/tail?n={TAIL_EVENTS}"),
            )
            .and_then(|b| parse_events(&b))
            .map(|evs| crate::wire_render::render_tail(&evs.join("\n"), TAIL_EVENTS)),
            RouteKey::Mailbox => {
                http_get(port, &format!("/agent/api/agent/mailbox/letters/{name}"))
                    .and_then(|b| parse_letter_content(&b))
            }
        };
        let text = got.unwrap_or_else(|e| format!("（取数失败：{e}）"));
        let mut g = inner().lock().unwrap();
        let epoch = EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
        g.snap.content = Some(Content { title, text, epoch });
        DIRTY.store(true, Ordering::Relaxed);
    });
}

/// 查看器标题（点条目开框时壳侧先算好，与取回件 title 同尺——
/// 壳脏帧据此对在途旧件：title 对不上的取回件不喂开着的框）
pub fn content_title(key: &RouteKey, name: &str) -> String {
    match key {
        RouteKey::Line(line) => format!("{line}/{name}"),
        RouteKey::Mailbox => format!("{MAILBOX_TITLE}/{name}"),
    }
}

/// GET 一个 JSON 面拿回 body（B 档胶水，svc_health::http_get 同款）：
/// 连接/写/读全带超时，非 200 即错
fn http_get(port: u16, path: &str) -> Result<String, String> {
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let st =
        TcpStream::connect_timeout(&addr, HTTP_TIMEOUT).map_err(|e| format!("连接失败: {e}"))?;
    st.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    st.set_write_timeout(Some(HTTP_TIMEOUT)).ok();
    let req = crate::http1::serialize_request(&crate::http1::Request {
        method: "GET".into(),
        path: path.into(),
        headers: vec![("Host".into(), "127.0.0.1".into())],
        body: Vec::new(),
    });
    let mut st = st;
    st.write_all(&req).map_err(|e| format!("写请求失败: {e}"))?;
    let mut io = crate::http1::BufIo::new(st);
    let head = io.read_head().map_err(|e| format!("读响应头失败: {e}"))?;
    if head.status != 200 {
        return Err(format!("HTTP {}", head.status));
    }
    let mut rd = io.body_reader(head.body_kind());
    let mut body = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match rd.read_body(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                body.extend_from_slice(&buf[..n]);
                if body.len() > BODY_CAP {
                    return Err("body 超限".into());
                }
            }
            Err(e) => return Err(format!("读 body 失败: {e}")),
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}
