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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

// ---- A 档：数据形状与纯函数（考题先行钉死）----

/// 下池路由键：线 / 本机信箱 / 全局评审信箱（特殊路由）/
/// 主册 / NA信箱（BAR-212：新两册只走解析页信箱入口，不进会话池
/// routes_of——旧入口用户验收前不下线，故 routes_of 不推新行；
/// 变体挂在这是为了正文取数/缓存机（request_content/plan_open）
/// 与信箱 key 表一处同源）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteKey {
    Line(String),
    Mailbox,
    /// BAR-167 工单③：全局评审信箱（agentd inbox_root 映射表同表）
    AgentInbox,
    /// BAR-212：00-主册（$HOME/90-信箱 下的服务器级资产，agentd key=main-book）
    MainBook,
    /// BAR-212：10-NA信箱（agentd key=na-book）
    NaBook,
}

/// 信箱路由标题（与 session/MAILBOX_DIR 同一面，UI 文案层）
pub const MAILBOX_TITLE: &str = "信箱";
/// 全局评审信箱路由标题（用户点名用原文件夹名）
pub const AGENT_INBOX_TITLE: &str = "agent-inbox";
/// 空态占位行文案
pub const EMPTY_SESSIONS: &str = "（无会话）";
pub const EMPTY_LETTERS: &str = "（无信件）";
/// 查看器一次渲染的尾部事件数
pub const TAIL_EVENTS: usize = 60;

/// 下池路由表：各线一行（meta = "线"）+ 全局评审信箱固定行 + 信箱
/// 特殊路由收尾（meta = "信件"；agent-inbox 在线表后、信箱前，位置钉死）
pub fn routes_of(lines: &[String]) -> Vec<(RouteKey, String, String)> {
    let mut rows: Vec<(RouteKey, String, String)> = lines
        .iter()
        .map(|l| (RouteKey::Line(l.clone()), l.clone(), "线".to_string()))
        .collect();
    rows.push((
        RouteKey::AgentInbox,
        AGENT_INBOX_TITLE.to_string(),
        "信件".to_string(),
    ));
    rows.push((
        RouteKey::Mailbox,
        MAILBOX_TITLE.to_string(),
        "信件".to_string(),
    ));
    rows
}

/// 路由键 → agentd 信箱 key（BAR-167，与 agentd inbox_root 映射表同表）
pub fn inbox_api_key(key: &RouteKey) -> Option<&'static str> {
    match key {
        RouteKey::Mailbox => Some("mailbox"),
        RouteKey::AgentInbox => Some("agent-inbox"),
        RouteKey::MainBook => Some(crate::mail_feed::MailKey::MainBook.api_key()),
        RouteKey::NaBook => Some(crate::mail_feed::MailKey::NaBook.api_key()),
        RouteKey::Line(_) => None,
    }
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

// ---- BAR-174：信件本地缓存 + 增量加载（纯核）----

/// 信件元数据（列表面：name/bytes/mtime，mtime = 增量同步比对键，
/// 旧响应无 mtime 字段兼容为 0 = 全量重抓）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterMeta {
    pub name: String,
    pub bytes: u64,
    pub mtime: u64,
}

/// 缓存行 stale 标记（value 后缀：远端未到前给用户看的缓存行）
pub const STALE_MARK: &str = " · 缓存";
/// 缓存正文回退时的引用块头（正文前一行）
pub const CACHE_NOTICE: &str = "> （本地缓存副本，联网后自动刷新）\n\n";

/// reconcile 的产物：新清单 + 待抓正文名单 + 待删正文名单
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncPlan {
    pub list: Vec<LetterMeta>,
    pub fetch: Vec<String>,
    pub delete: Vec<String>,
}

/// letters 面 → 信件元数据表（无 mtime 旧响应兼容为 0；坏 JSON / ok:false 即错）
pub fn parse_letter_list(body: &str) -> Result<Vec<LetterMeta>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 坏: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(err_detail(&v));
    }
    Ok(v.get("letters")
        .and_then(Value::as_array)
        .ok_or("缺 letters 字段")?
        .iter()
        .filter_map(|x| {
            let name = x.get("name").and_then(Value::as_str)?.to_string();
            Some(LetterMeta {
                name,
                bytes: x.get("bytes").and_then(Value::as_u64).unwrap_or(0),
                mtime: x.get("mtime").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect())
}

/// 增量同步比对（纯函数零 IO）：
/// fetch = 远端有而缓存无 / (bytes,mtime) 变了 / 有条目但缺正文文件；
/// delete = 缓存有而远端无；list = 远端新清单（落 manifest 用）
pub fn reconcile(cached: &[LetterMeta], remote: &[LetterMeta], have_body: &[String]) -> SyncPlan {
    let mut fetch = Vec::new();
    for r in remote {
        let stale = match cached.iter().find(|c| c.name == r.name) {
            None => true,
            Some(c) => c.bytes != r.bytes || c.mtime != r.mtime,
        };
        if stale || !have_body.iter().any(|n| n == &r.name) {
            fetch.push(r.name.clone());
        }
    }
    let delete = cached
        .iter()
        .filter(|c| !remote.iter().any(|r| r.name == c.name))
        .map(|c| c.name.clone())
        .collect();
    SyncPlan {
        list: remote.to_vec(),
        fetch,
        delete,
    }
}

/// manifest 落盘串（与端点同形 {"ok":true,"letters":[...]}——parse_letter_list
/// 一口径读回；ok:true 防「ok:false 不许吞表」闸咬自家缓存）
pub fn manifest_json(list: &[LetterMeta]) -> String {
    serde_json::json!({
        "ok": true,
        "letters": list.iter().map(|l| serde_json::json!({
            "name": l.name, "bytes": l.bytes, "mtime": l.mtime,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

/// 展示序（BAR-175，用户拍板「最新的信在最上面」）：按名降序——
/// v2.1/存量信名均 NNNN 零填充开头，字典序降序 = 编号降序 = 新上旧下。
/// 只动展示层：agentd 契约/manifest/reconcile 顺序无关不吃影响。
fn newest_first(list: &[LetterMeta]) -> Vec<&LetterMeta> {
    let mut v: Vec<&LetterMeta> = list.iter().collect();
    v.sort_by(|a, b| b.name.cmp(&a.name));
    v
}

/// 缓存行（stale）：value 带「 · 缓存」后缀，远端到了换鲜行去标
pub fn stale_letter_entries(list: &[LetterMeta]) -> Vec<(String, String)> {
    newest_first(list)
        .into_iter()
        .map(|l| {
            (
                l.name.clone(),
                format!("{}{STALE_MARK}", fmt_bytes(l.bytes)),
            )
        })
        .collect()
}

/// 新鲜行（远端刚到）：与旧面同形（label = 信名，value = 字节数）
pub fn fresh_letter_entries(list: &[LetterMeta]) -> Vec<(String, String)> {
    newest_first(list)
        .into_iter()
        .map(|l| (l.name.clone(), fmt_bytes(l.bytes)))
        .collect()
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

// ---- BAR-225 时间去时区（0152-B 用户拍板「时间一律去时区」）----
// 查看器详情页直渲信件 md，楼层头（`> N楼：… · 2026-10-02 15:20 +08:00`）
// 与信封行（`> 日期: …`）的时区在正文里——显示路发布前过本件，缓存存
// 原文（写透在变换前）。只对「日期 时分 ±时区」三段连排动刀，其余文本
// 一字不动（正文里的孤日期/孤时分不误伤）。

/// 行内去时区：`YYYY-MM-DD HH:MM ±HH:MM` 三段连排 → 前两段（日期+时分）
pub fn strip_tz_inline(line: &str) -> String {
    fn is_date_b(b: &[u8]) -> bool {
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    }
    fn is_hm(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 5
            && b[2] == b':'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 2 || c.is_ascii_digit())
    }
    fn is_tz(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 6
            && (b[0] == b'+' || b[0] == b'-')
            && b[3] == b':'
            && b[1..3].iter().all(|c| c.is_ascii_digit())
            && b[4..6].iter().all(|c| c.is_ascii_digit())
    }
    /// 日期段尾验（允许「（2026-10-02」式前缀——信封状态行实拍形态；
    /// 前缀随原文照留，只摘时区段）。字节级验尾 10 字节（日期形纯
    /// ASCII，多字节前缀不误切）
    fn tail_is_date(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() >= 10 && is_date_b(&b[b.len() - 10..])
    }
    // 空白分段的 span 流（原文空白形态逐段拼回，不重排）
    let mut spans: Vec<(usize, usize)> = Vec::new();
    {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_whitespace() {
                i += 1;
            } else {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                spans.push((start, i));
            }
        }
    }
    // 找三连 span（日期/时分/时区）——命中即摘第三段及其前空白；
    // 同一行多个三连全摘（楼层头/信封日期各一）
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i + 2 < spans.len() {
        let (a0, a1) = spans[i];
        let (b0, b1) = spans[i + 1];
        let (c0, c1) = spans[i + 2];
        if tail_is_date(&line[a0..a1]) && is_hm(&line[b0..b1]) && is_tz(&line[c0..c1]) {
            out.push_str(&line[cursor..b1]);
            cursor = c1;
            i += 3;
        } else {
            i += 1;
        }
    }
    if cursor == 0 {
        return line.to_string();
    }
    out.push_str(&line[cursor..]);
    out
}

/// 整篇 md 去时区（逐行同律；无三连的行原样过）
pub fn strip_tz_md(text: &str) -> String {
    let joined = text
        .lines()
        .map(strip_tz_inline)
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        joined + "\n"
    } else {
        joined
    }
}

/// URL query 值百分号编码（BAR-212 摘要批量端点的信名——中文句法名
/// 必须编码进 query；unreserved 字符原样，其余按 UTF-8 字节 %XX）
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
    /// 信件本地缓存根（BAR-174，壳 configure 旁喂）：<私有目录>/cache/letters
    cache_root: Option<PathBuf>,
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

/// 信件本地缓存根（BAR-174，壳 configure 旁喂，幂等）：
/// <internal_data_path>/cache/letters——未喂 = 缓存层整体关闭（纯远端行为不变）
pub fn set_cache_root(root: PathBuf) {
    let mut g = inner().lock().unwrap();
    g.cache_root = Some(root);
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

/// 刷上池条目表（切路由时调）：线 → sessions 面；信箱 → letters 面。
/// 信箱分支三段（BAR-174）：①先同步灌本地缓存行（stale 标，首屏即时）
/// → ②后台 GET 成功换新鲜行 + 缓存写透 → ③GET 失败保缓存行/现有占位
pub fn request_entries(key: RouteKey) {
    let (port, cache_root) = {
        let mut g = inner().lock().unwrap();
        g.snap.selected = Some(key.clone());
        g.snap.entries_loading = true;
        g.snap.entries.clear();
        DIRTY.store(true, Ordering::Relaxed);
        (g.cfg_port, g.cache_root.clone())
    };
    // ①发起前先灌缓存（fs 读在锁外；有缓存 = 信号差也立刻有列表看）
    if let (RouteKey::Mailbox | RouteKey::AgentInbox, Some(root)) = (&key, &cache_root) {
        let inbox = inbox_api_key(&key).expect("信箱键");
        if let Some(cached) = read_manifest(&root.join(inbox))
            && !cached.is_empty()
        {
            let rows = entries_or_placeholder(stale_letter_entries(&cached), EMPTY_LETTERS);
            let mut g = inner().lock().unwrap();
            if g.snap.selected.as_ref() == Some(&key) {
                g.snap.entries = rows
                    .into_iter()
                    .map(|(label, value)| EntryRow { label, value })
                    .collect();
            }
            bump();
        }
    }
    std::thread::spawn(move || {
        let got = match &key {
            RouteKey::Line(line) => {
                http_get(port, &format!("/agent/api/agent/lines/{line}/sessions"))
                    .and_then(|b| parse_named_bytes(&b, "sessions"))
                    .map(|ss| entries_or_placeholder(session_entries(&ss), EMPTY_SESSIONS))
            }
            RouteKey::Mailbox | RouteKey::AgentInbox | RouteKey::MainBook | RouteKey::NaBook => {
                let inbox = inbox_api_key(&key).expect("信箱键");
                http_get(port, &format!("/agent/api/agent/inboxes/{inbox}/letters"))
                    .and_then(|b| parse_letter_list(&b))
                    .map(|ls| {
                        // ②缓存写透（增量 reconcile；fs/网络 IO 全在锁外）
                        if let Some(root) = &cache_root {
                            sync_inbox_cache(root, inbox, &ls, port);
                        }
                        entries_or_placeholder(fresh_letter_entries(&ls), EMPTY_LETTERS)
                    })
            }
        };
        let mut g = inner().lock().unwrap();
        g.snap.entries_loading = false;
        // ③失败不动 entries：有缓存保持 ①灌的 stale 行，无缓存走现有空态
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

/// BAR-185：点开正文的发布计划（纯逻辑，钉在 sess_pool_spec）。
/// 缓存先画——有缓存立即发布缓存副本（带 CACHE_NOTICE），再后台 GET；
/// GET 成功换新鲜件（同 title epoch 递增，开着的查看器自动换芯），
/// 失败则缓存件留场。无缓存退化为旧路：GET 成败一次发布。
pub enum OpenStep {
    Cached(String),
    Fresh(String),
    Failed(String),
}

pub fn plan_open(cached: Option<String>, got: &Result<String, String>) -> Vec<OpenStep> {
    let mut steps = Vec::new();
    if let Some(c) = cached {
        steps.push(OpenStep::Cached(format!("{CACHE_NOTICE}{c}")));
    }
    match got {
        Ok(fresh) => steps.push(OpenStep::Fresh(fresh.clone())),
        Err(e) if steps.is_empty() => steps.push(OpenStep::Failed(format!("（取数失败：{e}）"))),
        Err(_) => {}
    }
    steps
}

/// 取条目内容（点条目时调）：会话 → tail 面 + wire_render 渲染；
/// 信 → 正文直读。**BAR-185 缓存先画**（旧路 GET-first：弱网连接活着但慢时
/// 每点一封信硬等超时——「总是加载中」病灶）：有缓存副本先发布（带声明头），
/// 后台 GET 成功写透缓存并换新鲜件，失败缓存留场；缓存也没有才落
/// 「（取数失败：…）」。取回写入快照 content（壳脏帧喂查看器）。
pub fn request_content(key: RouteKey, name: &str) {
    let (port, title, cache_root) = {
        let g = inner().lock().unwrap();
        (g.cfg_port, content_title(&key, name), g.cache_root.clone())
    };
    let name = name.to_string();
    std::thread::spawn(move || {
        let publish = |text: String| {
            let mut g = inner().lock().unwrap();
            let epoch = EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
            g.snap.content = Some(Content {
                title: title.clone(),
                text,
                epoch,
            });
            DIRTY.store(true, Ordering::Relaxed);
        };
        match &key {
            RouteKey::Line(line) => {
                let got = http_get(
                    port,
                    &format!("/agent/api/agent/lines/{line}/sessions/{name}/tail?n={TAIL_EVENTS}"),
                )
                .and_then(|b| parse_events(&b))
                .map(|evs| crate::wire_render::render_tail(&evs.join("\n"), TAIL_EVENTS));
                publish(got.unwrap_or_else(|e| format!("（取数失败：{e}）")));
            }
            RouteKey::Mailbox | RouteKey::AgentInbox | RouteKey::MainBook | RouteKey::NaBook => {
                let inbox = inbox_api_key(&key).expect("信箱键");
                let dir = cache_root.as_ref().map(|r| r.join(inbox));
                // ①缓存先画（fs 读在工作者线程，不卡 UI）②后台 GET 成败
                // 都过 plan_open 裁决发布序列（纯逻辑钉在 sess_pool_spec）
                let cached = cache_root
                    .as_ref()
                    .and_then(|root| read_body(&root.join(inbox), &name));
                let got = http_get(
                    port,
                    &format!("/agent/api/agent/inboxes/{inbox}/letters/{name}"),
                )
                .and_then(|b| parse_letter_content(&b));
                for step in plan_open(cached, &got) {
                    match step {
                        OpenStep::Fresh(t) => {
                            // 写透正文缓存（单封失败不连坐，缓存是加强不是命脉）
                            if let Some(d) = &dir {
                                let _ = write_body(d, &name, &t);
                            }
                            // BAR-225：显示路去时区（缓存存原文，0152-B）
                            publish(strip_tz_md(&t));
                        }
                        OpenStep::Cached(t) | OpenStep::Failed(t) => publish(strip_tz_md(&t)),
                    }
                }
            }
        }
    });
}

/// 查看器标题（点条目开框时壳侧先算好，与取回件 title 同尺——
/// 壳脏帧据此对在途旧件：title 对不上的取回件不喂开着的框）
pub fn content_title(key: &RouteKey, name: &str) -> String {
    match key {
        RouteKey::Line(line) => format!("{line}/{name}"),
        RouteKey::Mailbox => format!("{MAILBOX_TITLE}/{name}"),
        RouteKey::AgentInbox => format!("{AGENT_INBOX_TITLE}/{name}"),
        RouteKey::MainBook => format!("{}/{name}", crate::mail_feed::MailKey::MainBook.title()),
        RouteKey::NaBook => format!("{}/{name}", crate::mail_feed::MailKey::NaBook.title()),
    }
}

// ---- BAR-174：信件本地缓存 fs 面（全锁外 IO；缓存是加强不是命脉，
// 读写失败一律跳过/回退，不上报不炸）----

/// 信箱缓存目录：<cache_root>/<inbox_key>
fn cache_dir(root: &Path, inbox: &str) -> PathBuf {
    root.join(inbox)
}

/// 读 manifest（文件缺/坏 JSON 一律 None = 无缓存，不炸）
fn read_manifest(dir: &Path) -> Option<Vec<LetterMeta>> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    parse_letter_list(&text).ok()
}

/// 写 manifest（父目录自动建）
fn write_manifest(dir: &Path, list: &[LetterMeta]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建缓存目录失败: {e}"))?;
    std::fs::write(dir.join("manifest.json"), manifest_json(list))
        .map_err(|e| format!("写 manifest 失败: {e}"))
}

/// 缓存目录里现存正文文件名（*.md；reconcile 的 have_body 入参）
fn cached_body_names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") {
                out.push(name);
            }
        }
    }
    out
}

/// 读缓存正文（缺/坏 = None）
fn read_body(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

/// 写透缓存正文（父目录自动建）
fn write_body(dir: &Path, name: &str, content: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建缓存目录失败: {e}"))?;
    std::fs::write(dir.join(name), content).map_err(|e| format!("写缓存正文失败: {e}"))
}

/// 列表 GET 成功后的缓存写透（reconcile 增量）：
/// 新 manifest 落盘 → 删消失信正文 → 逐封 GET fetch 名单正文写透
/// （单封失败跳过不连坐）
fn sync_inbox_cache(root: &Path, inbox: &str, remote: &[LetterMeta], port: u16) {
    let dir = cache_dir(root, inbox);
    let cached = read_manifest(&dir).unwrap_or_default();
    let have = cached_body_names(&dir);
    let plan = reconcile(&cached, remote, &have);
    let _ = write_manifest(&dir, &plan.list);
    for name in &plan.delete {
        let _ = std::fs::remove_file(dir.join(name));
    }
    for name in &plan.fetch {
        if let Ok(content) = http_get(
            port,
            &format!("/agent/api/agent/inboxes/{inbox}/letters/{name}"),
        )
        .and_then(|b| parse_letter_content(&b))
        {
            let _ = write_body(&dir, name, &content);
        }
    }
}

/// GET 一个 JSON 面拿回 body（B 档胶水，svc_health::http_get 同款）：
/// 连接/写/读全带超时，非 200 即错
pub(crate) fn http_get(port: u16, path: &str) -> Result<String, String> {
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
