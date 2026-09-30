//! mail_feed.rs — 信箱列表页数据源（BAR-212，2026-09-30 用户立项：
//! 信箱迁为服务器级资产（$HOME/90-信箱）后，na 侧显示入口从设置页会话池
//! 迁解析页左下常驻槽；两册 = 00-主册 + 10-NA信箱）。
//!
//! 分层照 sess_pool 范式：上半 A 档纯核（agentd JSON 面解析/摘要合账/
//! 懒加载窗裁决，tests/mail_feed_spec.rs 钉死）；下半 B 档取数胶水
//! （sess_pool::http_get 同款隧道本地口反代进 agentd，零新依赖）。
//!
//! 与会话池旧入口的关系：旧路由（sess_pool RouteKey::Mailbox/AgentInbox）
//! 一行不动（用户拍板：新入口真机验收前旧入口不下线）；本册服务新两册，
//! agentd key = main-book / na-book（inbox_root 映射表同表）。
//!
//! 摘要懒加载（用户拍板：「最先渲染视口里和它上面几屏的摘要，滚动到了
//! 再慢慢获取，获取到了就固定」）：列表端点只回字头四件（time/from/to/
//! title，agentd 读信头解析），摘要走 summaries 批量端点按视口窗懒取，
//! 取到写透本地缓存（summaries.json，mtime 对账——信变了摘要作废重取），
//! 离线 = 缓存摘要照显。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

// ---- A 档：数据形状与纯函数（考题先行钉死）----

/// 信箱册键（两册固定；agentd inbox_root 映射表同表，fail-closed）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MailKey {
    /// 00-主册
    MainBook,
    /// 10-NA信箱
    NaBook,
}

impl MailKey {
    /// 入口卡行序（主册在上、NA信箱在下——位置钉死）
    pub fn all() -> [MailKey; 2] {
        [MailKey::MainBook, MailKey::NaBook]
    }

    /// agentd 信箱 key（crates/na-agentd service.rs inbox_root 同表）
    pub fn api_key(&self) -> &'static str {
        match self {
            MailKey::MainBook => "main-book",
            MailKey::NaBook => "na-book",
        }
    }

    /// UI 标题（入口行/列表卡头同一份）
    pub fn title(&self) -> &'static str {
        match self {
            MailKey::MainBook => "主册",
            MailKey::NaBook => "NA信箱",
        }
    }
}

/// 信件条目（列表页一行的全部素材）：列表端点给 name/bytes/mtime +
/// 字头四件；summary = 懒加载件（None = 未取到/未取，涂装层显占位）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEntry {
    pub name: String,
    pub bytes: u64,
    pub mtime: u64,
    pub time: String,
    pub from: String,
    pub to: String,
    pub title: String,
    pub summary: Option<String>,
}

/// 列表响应 → 条目表（无 summary——懒加载件不在这条面；旧响应缺
/// 字头四件容错为 ""；坏 JSON / ok:false 即错）
pub fn parse_mail_list(body: &str) -> Result<Vec<MailEntry>, String> {
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
            let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            Some(MailEntry {
                name,
                bytes: x.get("bytes").and_then(Value::as_u64).unwrap_or(0),
                mtime: x.get("mtime").and_then(Value::as_u64).unwrap_or(0),
                time: s("time"),
                from: s("from"),
                to: s("to"),
                title: s("title"),
                summary: None,
            })
        })
        .collect())
}

/// 摘要批量响应 → (信名, mtime, 摘要) 表（mtime = 服务端取摘要时刻的
/// 信件 mtime，合账对 staleness 用；缺 mtime 容错 0 = 永过期）
pub fn parse_summaries(body: &str) -> Result<Vec<(String, u64, String)>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON 坏: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(err_detail(&v));
    }
    Ok(v.get("summaries")
        .and_then(Value::as_array)
        .ok_or("缺 summaries 字段")?
        .iter()
        .filter_map(|x| {
            let name = x.get("name").and_then(Value::as_str)?.to_string();
            let mtime = x.get("mtime").and_then(Value::as_u64).unwrap_or(0);
            let summary = x
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some((name, mtime, summary))
        })
        .collect())
}

/// 摘要合账（纯函数零 IO）：摘要表合进条目表——只对 mtime 相同的条目
/// 生效（信改了旧摘要不许冒顶）；条目在摘要表里缺席 = 原样（None 保持
/// None，已有的保持已有）
pub fn merge_summaries(entries: &mut [MailEntry], sums: &[(String, u64, String)]) {
    for e in entries.iter_mut() {
        if let Some((_, _, s)) = sums.iter().find(|(n, m, _)| n == &e.name && *m == e.mtime) {
            e.summary = Some(s.clone());
        }
    }
}

/// 摘要待取名单（懒加载窗裁决的纯核）：窗内条目里摘要缺位的名。
/// 已取过（Some）/信已变（mtime 对不上由 merge 拦）的不重复取
pub fn summaries_wanted(entries: &[MailEntry], window: std::ops::Range<usize>) -> Vec<String> {
    entries
        .iter()
        .enumerate()
        .filter(|(i, e)| window.contains(i) && e.summary.is_none())
        .map(|(_, e)| e.name.clone())
        .collect()
}

/// manifest 落盘串（与列表端点同形 + 字头四件——parse_mail_list 一口径
/// 读回；ok:true 防「ok:false 不许吞表」闸咬自家缓存）
pub fn manifest_json(list: &[MailEntry]) -> String {
    serde_json::json!({
        "ok": true,
        "letters": list.iter().map(|l| serde_json::json!({
            "name": l.name, "bytes": l.bytes, "mtime": l.mtime,
            "time": l.time, "from": l.from, "to": l.to, "title": l.title,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

/// summaries.json 落盘串（与摘要端点同形——parse_summaries 一口径读回）
pub fn summaries_json(sums: &[(String, u64, String)]) -> String {
    serde_json::json!({
        "ok": true,
        "summaries": sums.iter().map(|(n, m, s)| serde_json::json!({
            "name": n, "mtime": m, "summary": s,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

/// 展示序（BAR-212 用户拍板：「最新的信在最下方，整个视口默认追底」——
/// 与会话池 BAR-175 顶锚相反，有意）：按名升序 = 旧上新下，列表尾 =
/// 最新。信名 NNNN 零填充保证字典序 = 编号序
pub fn oldest_first(list: &[MailEntry]) -> Vec<MailEntry> {
    let mut v = list.to_vec();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// 条目显示计数字（入口行 meta：「N 封」；未取到 = None 显「…」）
pub fn count_word(n: Option<usize>) -> String {
    match n {
        Some(n) => format!("{n} 封"),
        None => "…".to_string(),
    }
}

/// {"ok":false,"error":"..."} 的错误详情（兜底给整串）
fn err_detail(v: &Value) -> String {
    v.get("error")
        .and_then(Value::as_str)
        .unwrap_or("面回 ok:false")
        .to_string()
}

// ---- B 档：取数胶水（sess_pool 同款：全局快照 + 后台线程 + 缓存写透）----

/// 一册的状态（entries 已合摘要；loading = 列表在途）
#[derive(Debug, Clone, Default)]
pub struct BookState {
    pub entries: Vec<MailEntry>,
    pub loading: bool,
    /// 至少成功取到过一次列表（远端或缓存）——入口行 meta 的 None/… 裁决
    pub fetched: bool,
}

#[derive(Default)]
struct Inner {
    main: BookState,
    na: BookState,
    cfg_port: u16,
    /// 本地缓存根（壳 configure 旁喂）：<私有目录>/cache/mail
    cache_root: Option<PathBuf>,
    /// 摘要取数在途名集（防每帧重发——懒加载窗滚动时同批不重复取）
    summary_inflight: HashSet<String>,
    /// 摘要取数失败名集（本次会话内不再自动重试；下次 request_list 清）
    summary_failed: HashSet<String>,
}

static INNER: OnceLock<Mutex<Inner>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(0);

fn inner() -> &'static Mutex<Inner> {
    INNER.get_or_init(|| Mutex::new(Inner::default()))
}

fn bump() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
    DIRTY.store(true, Ordering::Relaxed)
}

/// 当前代（涂装 sig 维用）
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// 配置（壳设置加载/重载时喂，sess_pool::configure 旁同款）：隧道本地口
pub fn configure(local_port: u16) {
    inner().lock().unwrap().cfg_port = local_port;
}

/// 信箱本地缓存根（壳 configure 旁喂，幂等）：<internal_data_path>/cache/mail
/// ——未喂 = 缓存层整体关闭（纯远端行为不变）
pub fn set_cache_root(root: PathBuf) {
    inner().lock().unwrap().cache_root = Some(root);
}

fn book_mut(g: &mut Inner, key: MailKey) -> &mut BookState {
    match key {
        MailKey::MainBook => &mut g.main,
        MailKey::NaBook => &mut g.na,
    }
}

fn book(g: &Inner, key: MailKey) -> &BookState {
    match key {
        MailKey::MainBook => &g.main,
        MailKey::NaBook => &g.na,
    }
}

/// 读一册快照（涂装/几何直读；entries 已按 oldest_first 排好）
pub fn book_snap(key: MailKey) -> BookState {
    let g = inner().lock().unwrap();
    book(&g, key).clone()
}

/// 壳脏帧消耗口（sess_pool::take_dirty 同款）
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

/// 刷一册列表（开列表卡/下拉刷新时调）：三段式——①先灌本地缓存
/// （含已缓存摘要，首屏即时）→ ②后台 GET 成功换新鲜表 + manifest
/// 写透 → ③GET 失败保缓存表。列表到了再按当前视口窗补摘要
/// （ensure_summaries 由壳滚动/开帧喂窗）
pub fn request_list(key: MailKey) {
    let (port, cache_root) = {
        let mut g = inner().lock().unwrap();
        let b = book_mut(&mut g, key);
        if b.loading {
            return;
        }
        b.loading = true;
        g.summary_failed.clear();
        DIRTY.store(true, Ordering::Relaxed);
        (g.cfg_port, g.cache_root.clone())
    };
    // ①发起前先灌缓存（fs 读在锁外；有缓存 = 信号差也立刻有列表看）
    if let Some(root) = &cache_root {
        let dir = cache_dir(root, key.api_key());
        if let Some(mut cached) = read_manifest(&dir)
            && !cached.is_empty()
        {
            merge_summaries(&mut cached, &read_summaries(&dir));
            let cached = oldest_first(&cached);
            let mut g = inner().lock().unwrap();
            let b = book_mut(&mut g, key);
            if !b.fetched {
                b.entries = cached;
                b.fetched = true;
            }
            bump();
        }
    }
    std::thread::spawn(move || {
        let got = crate::sess_pool::http_get(
            port,
            &format!("/agent/api/agent/inboxes/{}/letters", key.api_key()),
        )
        .and_then(|b| parse_mail_list(&b));
        if let Ok(list) = &got {
            // ②缓存写透（fs/网络 IO 全在锁外）：manifest 换新鲜表；
            // 摘要缓存不动（mtime 对账自然作废旧摘要）
            if let Some(root) = &cache_root {
                let _ = write_manifest(&cache_dir(root, key.api_key()), list);
            }
        }
        let mut g = inner().lock().unwrap();
        let b = book_mut(&mut g, key);
        b.loading = false;
        // ③失败不动 entries：有缓存保持 ①灌的表
        if let Ok(list) = got {
            let mut list = list;
            // 合已在内存里的摘要（同册 entries 现有的 Some 按名+mtime 带过去）
            let old_sums: Vec<(String, u64, String)> = b
                .entries
                .iter()
                .filter_map(|e| {
                    e.summary
                        .as_ref()
                        .map(|s| (e.name.clone(), e.mtime, s.clone()))
                })
                .collect();
            merge_summaries(&mut list, &old_sums);
            b.entries = oldest_first(&list);
            b.fetched = true;
        }
        bump();
    });
}

/// 懒加载摘要（壳每次涂装/滚动算出视口窗喂入）：窗内缺摘要的名去重
/// （在途/已败不重发），后台批量 GET 成功 → 合账 + summaries.json
/// 写透；失败名进 failed 集（本次会话不再自动重试，防弱网每帧风暴）
pub fn ensure_summaries(key: MailKey, wanted: Vec<String>) {
    let (port, cache_root, names) = {
        let mut g = inner().lock().unwrap();
        let names: Vec<String> = wanted
            .into_iter()
            .filter(|n| !g.summary_inflight.contains(n) && !g.summary_failed.contains(n))
            .collect();
        if names.is_empty() {
            return;
        }
        for n in &names {
            g.summary_inflight.insert(n.clone());
        }
        (g.cfg_port, g.cache_root.clone(), names)
    };
    std::thread::spawn(move || {
        let qs = names
            .iter()
            .map(|n| crate::sess_pool::url_encode(n))
            .collect::<Vec<_>>()
            .join(",");
        let got = crate::sess_pool::http_get(
            port,
            &format!(
                "/agent/api/agent/inboxes/{}/summaries?names={qs}",
                key.api_key()
            ),
        )
        .and_then(|b| parse_summaries(&b));
        let mut g = inner().lock().unwrap();
        match got {
            Ok(sums) => {
                // 合账 + 缓存写透（单批失败不连坐；fs IO 本在工作者线程）
                merge_summaries(book_mut(&mut g, key).entries.as_mut_slice(), &sums);
                if let Some(root) = &cache_root {
                    let dir = cache_dir(root, key.api_key());
                    let mut all: HashMap<String, (u64, String)> = read_summaries(&dir)
                        .into_iter()
                        .map(|(n, m, s)| (n, (m, s)))
                        .collect();
                    for (n, m, s) in sums {
                        all.insert(n, (m, s));
                    }
                    let flat: Vec<(String, u64, String)> =
                        all.into_iter().map(|(n, (m, s))| (n, m, s)).collect();
                    let _ = write_summaries(&dir, &flat);
                }
                for n in &names {
                    g.summary_inflight.remove(n);
                }
            }
            Err(_) => {
                for n in names {
                    g.summary_inflight.remove(&n);
                    g.summary_failed.insert(n);
                }
            }
        }
        bump();
    });
}

// ---- 本地缓存 fs 面（全锁外/工作者线程 IO；缓存是加强不是命脉，
// 读写失败一律跳过/回退，不上报不炸——sess_pool BAR-174 同律）----

/// 册缓存目录：<cache_root>/<api_key>
fn cache_dir(root: &Path, inbox: &str) -> PathBuf {
    root.join(inbox)
}

/// 读 manifest（文件缺/坏 JSON 一律 None = 无缓存，不炸）
fn read_manifest(dir: &Path) -> Option<Vec<MailEntry>> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    parse_mail_list(&text).ok()
}

/// 写 manifest（父目录自动建）
fn write_manifest(dir: &Path, list: &[MailEntry]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建缓存目录失败: {e}"))?;
    std::fs::write(dir.join("manifest.json"), manifest_json(list))
        .map_err(|e| format!("写 manifest 失败: {e}"))
}

/// 读 summaries.json（缺/坏 = 空表）
fn read_summaries(dir: &Path) -> Vec<(String, u64, String)> {
    std::fs::read_to_string(dir.join("summaries.json"))
        .ok()
        .and_then(|t| parse_summaries(&t).ok())
        .unwrap_or_default()
}

/// 写 summaries.json（父目录自动建）
fn write_summaries(dir: &Path, sums: &[(String, u64, String)]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建缓存目录失败: {e}"))?;
    std::fs::write(dir.join("summaries.json"), summaries_json(sums))
        .map_err(|e| format!("写 summaries 失败: {e}"))
}
