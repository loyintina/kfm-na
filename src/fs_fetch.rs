//! fs_fetch.rs — 文件树数据面客户端（BAR-165，2026-09-26）
//!
//! 拉 na-server 的两条文件树端点（`/api/fs/list`、`/api/fs/read`），把结果
//! 灌进 `ui::filetree` 的共享状态核并置脏帧。链路与 `sess_pool`/`svc_health`
//! 同款：隧道本地口上的平面 HTTP + `http1` 序列化（零新口，走现有隧道）。
//!
//! 相判定在客户端（与 `/api/na/sys` 同构——服务端不做相判定）：**文件树是
//! 远程相能力**（根在服务器上），本地相 v1 给占位行，不发网络。
//!
//! 线程纪律：取数一律后台线程（主线程零阻塞网络），回执落状态核 +
//! `take_dirty()` 让壳的条件重绘接住；行表变更**不带屏代快照**——屏代由
//! 涂装那一刻落账（`filetree::note_baked_snap`），命中吃账不吃活体。
//!
//! 查询值一律过 `fsapi::pct_encode`（中文目录名/含 `&`/空格的路径不编码
//! 就会被服务端拆错参数——本册与解码侧是同 crate 的对偶件）。

use crate::ui::filetree::{self, Entry, RowKind};
use na_protocol::fsapi;

use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

static PORT: AtomicU16 = AtomicU16::new(0);
static DIRTY: AtomicBool = AtomicBool::new(false);

// ---- BAR-187：文件树本地缓存（宪法 = 0047 §三：本地一切状态都是服务器
// 真源的显示缓存——可旧、可缺，绝不与真源争对错；形制复用 BAR-174/185）----

static CACHE_ROOT: std::sync::OnceLock<std::sync::Mutex<Option<std::path::PathBuf>>> =
    std::sync::OnceLock::new();

/// 文件树本地缓存根（壳 configure 旁喂，幂等）：<私有目录>/cache/fs——
/// 未喂 = 缓存层整体关闭（纯远端行为不变）
pub fn set_cache_root(root: std::path::PathBuf) {
    *CACHE_ROOT
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap() = Some(root);
}

fn cache_root() -> Option<std::path::PathBuf> {
    CACHE_ROOT
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
}

/// 缓存文件相对路径（list/read 两柜）。键过 `fsapi::pct_encode` 一口径——
/// 保留字只放行 `A-Za-z0-9-._~`，中文/斜杠路径落平文件名天然安全；
/// 空串（根目录键）落 `ROOT.json`（不许落 `.json` 隐形文件）
pub fn cache_rel(kind: &str, key: &str) -> String {
    let k = if key.is_empty() {
        "ROOT".to_string()
    } else {
        fsapi::pct_encode(key)
    };
    format!("{kind}/{k}.json")
}

/// 读缓存（不存在/IO 失败/空文件 = None——缓存是加强不是命脉，读坏不当障）
pub fn read_cache(root: &std::path::Path, rel: &str) -> Option<String> {
    let s = std::fs::read_to_string(root.join(rel)).ok()?;
    if s.is_empty() { None } else { Some(s) }
}

/// 写透缓存（IO 失败静默——单目失败不连坐，不上报不炸）
pub fn write_cache(root: &std::path::Path, rel: &str, body: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(p, body);
}

/// list 失败决策（纯函数）：有缓存 = 缓存树留场（**不走 fail()**——不许把
/// 刚灌的缓存树拍成错误行/回退展开账）；无缓存 = 照旧错误路。
/// 返回值 = 是否走旧 fail 路
pub fn list_fail_goes_old(had_cache: bool) -> bool {
    !had_cache
}

/// 正文缓存出参 → 正文（path 必须同源，binary/坏 JSON/缺 text = None）
pub fn cached_read_text(body: &str, path: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return None;
    }
    if v.get("path").and_then(|p| p.as_str()) != Some(path) {
        return None;
    }
    if v.get("binary").and_then(|b| b.as_bool()) == Some(true) {
        return None;
    }
    v.get("text")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .filter(|t| !t.is_empty())
}

/// 正文缓存落盘串（与端点出参同形——cached_read_text 一口径读回）
pub fn read_cache_json(path: &str, text: &str) -> String {
    serde_json::json!({"ok": true, "path": path, "binary": false, "text": text}).to_string()
}

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// body 上限比 sess_pool（256KB）高一档：大目录的 JSON 会更长
const BODY_CAP: usize = 512 * 1024;

/// 配置（壳设置加载/重载时喂，svc_health/sess_pool 旁同款）：隧道本地口
pub fn configure(local_port: u16) {
    PORT.store(local_port, Ordering::Relaxed);
}

/// 壳脏帧消耗口：有变化取走 true（每帧一查，零成本）
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

/// 本地相（根在服务器上，本地相拿不到）——判在客户端，服务端保持相无关
fn local_phase() -> bool {
    crate::endpoint::current() == crate::endpoint::EndpointKind::Local
}

/// 根层刷新（页面召唤 / 底栏眼睛键）：拉 dir="" 的直接子层
pub fn request_root() {
    request_list(String::new());
}

/// 拉某目录的直接子层（后台线程）；`dir` 空串 = 根。
/// 展开态/loading 账由 `FileTreeState::toggle` 记账（本册只负责取数与回填）。
/// 回执里带出的**级联层**（六调④ 曾展开账恢复）递归走 `request_list_quiet`。
/// BAR-187①三段式：spawn 前先同步灌缓存（弱网秒开树）→ GET 成功换鲜 +
/// 写透 → GET 失败有缓存留场（不走 fail），无缓存照旧错误行
pub fn request_list(dir: String) {
    if local_phase() {
        fill(&dir, local_placeholder_rows(), false);
        return;
    }
    let had_cache = serve_list_cache(&dir, false);
    spawn_list(dir, false, had_cache);
}

/// 级联取层（六调④）：与 `request_list` 同一条链，只差回执落
/// `apply_list_quiet`（不起抽屉动画——曾展开账里长回来的层不是用户此刻点的）。
/// 由回执里的级联路径递归驱动，逐层把树长回来。BAR-187①同律缓存先灌
pub fn request_list_quiet(dir: String) {
    if local_phase() {
        return; // 本地相没有真树，级联无处可落
    }
    let had_cache = serve_list_cache(&dir, true);
    spawn_list(dir, true, had_cache);
}

/// ①缓存先灌：有缓存即同步落状态核（回执幂等，真源到了换鲜）；坏件当无缓存
fn serve_list_cache(dir: &str, quiet: bool) -> bool {
    let Some(body) = cache_root().and_then(|r| read_cache(&r, &cache_rel("list", dir))) else {
        return false;
    };
    match filetree::entries_of(&body) {
        Ok(entries) => {
            crate::report::report(
                "ftree",
                &format!("目录缓存先画 {dir:?} 条目 {}", entries.len()),
            );
            fill(dir, entries, quiet);
            true
        }
        Err(_) => false,
    }
}

/// 取层线程（两种回执共用；`quiet` 决定落哪个 apply；`had_cache` = ①灌过，
/// 失败时缓存留场不走 fail——BAR-187③）
fn spawn_list(dir: String, quiet: bool, had_cache: bool) {
    let port = PORT.load(Ordering::Relaxed);
    if port == 0 {
        crate::report::report("ftree", "文件树取数：隧道口未配置，跳过");
        return;
    }
    std::thread::spawn(move || {
        let path = format!("/api/fs/list?dir={}", fsapi::pct_encode(&dir));
        let body = match http_get(port, &path) {
            Ok(b) => b,
            Err(e) => {
                crate::report::report("ftree", &format!("列目录失败 {dir:?}: {e}"));
                if list_fail_goes_old(had_cache) {
                    fail(&dir, &format!("取数失败：{e}"));
                } else {
                    DIRTY.store(true, Ordering::Relaxed);
                }
                return;
            }
        };
        match filetree::entries_of(&body) {
            Ok(entries) => {
                // ②缓存写透（IO 在锁外；写坏静默——缓存是加强不是命脉）
                if let Some(root) = cache_root() {
                    write_cache(&root, &cache_rel("list", &dir), &body);
                }
                crate::report::report(
                    "ftree",
                    &format!("列目录到位 {dir:?} 条目 {}", entries.len()),
                );
                fill(&dir, entries, quiet);
            }
            Err(e) => {
                crate::report::report("ftree", &format!("列目录出参不认 {dir:?}: {e}"));
                if list_fail_goes_old(had_cache) {
                    fail(&dir, &format!("出参不认：{e}"));
                } else {
                    DIRTY.store(true, Ordering::Relaxed);
                }
            }
        }
    });
}

/// 拉一个文件的文本预览 → 开查看器（BAR-163 查看器，文件树槽也会画出它）。
/// 先开「加载中…」框：同标题换文不残留旧像素（与会话点条目同规）
pub fn request_read(path: String, name: String) {
    if local_phase() {
        open_viewer(name, LOCAL_MSG.to_string());
        return;
    }
    let port = PORT.load(Ordering::Relaxed);
    if port == 0 {
        crate::report::report("ftree", "文件预览：隧道口未配置，跳过");
        return;
    }
    open_viewer(name.clone(), "加载中…".to_string());
    std::thread::spawn(move || {
        let p = format!("/api/fs/read?path={}", fsapi::pct_encode(&path));
        let body = match http_get(port, &p) {
            Ok(b) => b,
            Err(e) => {
                open_viewer(name, format!("读取失败：{e}"));
                DIRTY.store(true, Ordering::Relaxed);
                return;
            }
        };
        let v: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                open_viewer(name, format!("出参不是 JSON：{e}"));
                DIRTY.store(true, Ordering::Relaxed);
                return;
            }
        };
        let text = if v.get("binary").and_then(|b| b.as_bool()) == Some(true) {
            format!(
                "（二进制文件，不预览）\n大小 {} 字节",
                v.get("size").and_then(|s| s.as_u64()).unwrap_or(0)
            )
        } else {
            let t = v
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("（空文件）")
                .to_string();
            if v.get("truncated").and_then(|b| b.as_bool()) == Some(true) {
                format!("{t}\n\n…（已截断，仅前 64KB）")
            } else {
                t
            }
        };
        open_viewer(name, text);
        DIRTY.store(true, Ordering::Relaxed);
    });
}

/// 本地相占位说明（一句话说清为什么没有树）
const LOCAL_MSG: &str = "本地相：文件树不可用\n（根在服务器上——远程相才有；切回远程相即见）";

fn local_placeholder_rows() -> Vec<Entry> {
    vec![Entry {
        name: "本地相：文件树不可用（远程相才有）".to_string(),
        kind: RowKind::File,
        size: 0,
        mtime: 0,
    }]
}

/// 回执落状态核：根层走 `apply_root_list`，子层走 `apply_list`（树序插入 +
/// 抽屉起步）或 `apply_list_quiet`（级联到位，不动画），并置脏帧。
/// **返回值即六调④ 的级联层**：逐个递归发 quiet 请求——曾展开账里的深层
/// 由此一层层长回来（每层一个请求，到没有命中为止）
fn fill(dir: &str, entries: Vec<Entry>, quiet: bool) {
    let now = crate::report::boot_ms() as u64;
    let mut cascade: Vec<String> = Vec::new();
    if let Some(h) = filetree::filetree_handle() {
        let mut st = h.lock().unwrap();
        cascade = if dir.is_empty() {
            st.apply_root_list(entries, now)
        } else if quiet {
            st.apply_list_quiet(dir, entries, now)
        } else {
            st.apply_list(dir, entries, now)
        };
    }
    DIRTY.store(true, Ordering::Relaxed);
    for p in cascade {
        crate::report::report("ftree", &format!("曾展开账恢复：补取 {p:?}"));
        request_list_quiet(p);
    }
}

/// 取数失败：摘 loading 账（否则该目录永远卡在「转圈」），行表给一行说明
fn fail(dir: &str, msg: &str) {
    let now = crate::report::boot_ms() as u64;
    if let Some(h) = filetree::filetree_handle() {
        let mut st = h.lock().unwrap();
        st.list_failed(dir, now);
        if dir.is_empty() {
            // 错误行是文件——没有可级联的目录，返回值只能是空
            let _ = st.apply_root_list(
                vec![Entry {
                    name: msg.to_string(),
                    kind: RowKind::File,
                    size: 0,
                    mtime: 0,
                }],
                now,
            );
        }
    }
    DIRTY.store(true, Ordering::Relaxed);
}

fn open_viewer(title: String, content: String) {
    if let Some(page) = crate::ui::cfg_page::cfg_page_handle() {
        page.lock().unwrap().open_viewer(title, content);
    }
    DIRTY.store(true, Ordering::Relaxed);
}

// ── 阅读页取数（BAR-170）────────────────────────────────────────────
// 首块/续块同一条链：offset 由 reader 核的 next_request 账给出，回执喂
// reader 核（**不再进 cfg_page 查看器**——改道点二）。竞态守卫两条：
// ① 发请求前 mark_loading（防同块并发——壳每帧都可能问 need_prefetch）；
// ② 回执落地前对 path（换文件后迟到的块不许喂进新文件——静默丢弃，
//    新文件的请求自会补来）。

/// 取一块（offset 语义 = 源字节消费量，服务端回执 next_offset 续账）
pub fn request_read_chunk(offset: u64) {
    if local_phase() {
        feed_reader("", |st| {
            st.apply_error("本地相：阅读不可用（根在服务器上——远程相才有）")
        });
        return;
    }
    let port = PORT.load(Ordering::Relaxed);
    if port == 0 {
        crate::report::report("reader", "阅读取数：隧道口未配置，跳过");
        return;
    }
    let Some(h) = crate::ui::reader_page::reader_handle() else {
        return;
    };
    let path = {
        let mut st = h.lock().unwrap();
        if st.next_request() != Some(offset) {
            return; // 核账不认的 offset（重复/过期请求）不发
        }
        st.mark_loading();
        st.path.clone()
    };
    DIRTY.store(true, Ordering::Relaxed);
    // BAR-187②：首块且有缓存 = 缓存先画（声明头 + 正文立即上屏），网络
    // 回执改走 apply_refresh0 换芯；失败 refresh_failed 摘账缓存留场
    let served_cache = offset == 0 && serve_read_cache(&path);
    std::thread::spawn(move || {
        let p = format!(
            "/api/fs/read?path={}&offset={offset}",
            fsapi::pct_encode(&path)
        );
        let body = match http_get(port, &p) {
            Ok(b) => b,
            Err(e) => {
                crate::report::report("reader", &format!("读块失败 {path:?}@{offset}: {e}"));
                if served_cache {
                    feed_reader(&path, |st| st.refresh_failed());
                } else {
                    feed_reader(&path, |st| st.apply_error(&format!("读取失败：{e}")));
                }
                return;
            }
        };
        let v: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                if served_cache {
                    feed_reader(&path, |st| st.refresh_failed());
                } else {
                    feed_reader(&path, |st| st.apply_error(&format!("出参不是 JSON：{e}")));
                }
                return;
            }
        };
        if v.get("binary").and_then(|b| b.as_bool()) == Some(true) {
            feed_reader(&path, |st| st.apply_binary());
            return;
        }
        let (Some(text), Some(next), Some(trunc)) = (
            v.get("text").and_then(|t| t.as_str()),
            v.get("next_offset").and_then(|n| n.as_u64()),
            v.get("truncated").and_then(|b| b.as_bool()),
        ) else {
            // 缺键 = 出参不认（**不许拿 text.len() 顶 next_offset**——本体①）
            if served_cache {
                feed_reader(&path, |st| st.refresh_failed());
            } else {
                feed_reader(&path, |st| {
                    st.apply_error("出参缺键（text/next_offset/truncated）")
                });
            }
            return;
        };
        crate::report::report(
            "reader",
            &format!("块到位 {path:?}@{offset} → {next}（truncated={trunc}）"),
        );
        if served_cache {
            feed_reader(&path, |st| st.apply_refresh0(next, trunc, text));
        } else {
            feed_reader(&path, |st| st.apply_chunk(offset, next, trunc, text));
        }
        // ②写透：真源完整读完（eof 且未到帽）才落缓存——半截/到帽不存
        if trunc {
            return;
        }
        if let (Some(root), Some(h2)) = (cache_root(), crate::ui::reader_page::reader_handle()) {
            let snap = {
                let st = h2.lock().unwrap();
                if st.path == path && st.eof && !st.capped && !st.text.is_empty() {
                    Some(st.text.clone())
                } else {
                    None
                }
            };
            if let Some(t) = snap {
                write_cache(
                    &root,
                    &cache_rel("read", &path),
                    &read_cache_json(&path, &t),
                );
            }
        }
    });
}

/// ②缓存先画（BAR-187）：缓存副本带声明头立即上屏；坏件/不同源 = 不画
fn serve_read_cache(path: &str) -> bool {
    let Some(body) = cache_root().and_then(|r| read_cache(&r, &cache_rel("read", path))) else {
        return false;
    };
    let Some(text) = cached_read_text(&body, path) else {
        return false;
    };
    crate::report::report("reader", &format!("正文缓存先画 {path:?}"));
    feed_reader(path, |st| {
        st.apply_cached(&format!("{}{text}", crate::sess_pool::CACHE_NOTICE))
    });
    true
}

/// 回执落地（守卫②：核里当前 path 必须与回执同源；空串守卫 = 无条件喂，
/// 供本地相占位用）
fn feed_reader(path: &str, f: impl FnOnce(&mut crate::ui::reader_page::ReaderPage)) {
    if let Some(h) = crate::ui::reader_page::reader_handle() {
        let mut st = h.lock().unwrap();
        if path.is_empty() || st.path == path {
            f(&mut st);
        }
    }
    DIRTY.store(true, Ordering::Relaxed);
}

/// GET 一个 JSON 面拿回 body（sess_pool::http_get 同款：连接/写/读全带
/// 超时，非 200 即错）——第三份复制是有意的：另两份的 BODY_CAP 与本册
/// 不同档（64KB/256KB/512KB），合并不如各自显式
fn http_get(port: u16, path: &str) -> Result<String, String> {
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut st =
        TcpStream::connect_timeout(&addr, HTTP_TIMEOUT).map_err(|e| format!("连接失败: {e}"))?;
    st.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    st.set_write_timeout(Some(HTTP_TIMEOUT)).ok();
    let req = crate::http1::serialize_request(&crate::http1::Request {
        method: "GET".into(),
        path: path.into(),
        headers: vec![("Host".into(), "127.0.0.1".into())],
        body: Vec::new(),
    });
    st.write_all(&req).map_err(|e| format!("写请求失败: {e}"))?;
    let mut io = crate::http1::BufIo::new(st);
    let head = io.read_head().map_err(|e| format!("读响应头失败: {e}"))?;
    if head.status != 200 {
        return Err(format!("HTTP {}", head.status));
    }
    let mut rd = io.body_reader(head.body_kind());
    let mut body = Vec::new();
    let mut buf = [0u8; 8192];
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
