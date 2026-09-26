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
/// 展开态/loading 账由 `FileTreeState::toggle` 记账（本册只负责取数与回填）
pub fn request_list(dir: String) {
    if local_phase() {
        fill(&dir, local_placeholder_rows());
        return;
    }
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
                fail(&dir, &format!("取数失败：{e}"));
                return;
            }
        };
        match filetree::entries_of(&body) {
            Ok(entries) => {
                crate::report::report(
                    "ftree",
                    &format!("列目录到位 {dir:?} 条目 {}", entries.len()),
                );
                fill(&dir, entries);
            }
            Err(e) => {
                crate::report::report("ftree", &format!("列目录出参不认 {dir:?}: {e}"));
                fail(&dir, &format!("出参不认：{e}"));
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
/// 抽屉起步），并置脏帧
fn fill(dir: &str, entries: Vec<Entry>) {
    let now = crate::report::boot_ms() as u64;
    if let Some(h) = filetree::filetree_handle() {
        let mut st = h.lock().unwrap();
        if dir.is_empty() {
            st.apply_root_list(entries, now);
        } else {
            st.apply_list(dir, entries, now);
        }
    }
    DIRTY.store(true, Ordering::Relaxed);
}

/// 取数失败：摘 loading 账（否则该目录永远卡在「转圈」），行表给一行说明
fn fail(dir: &str, msg: &str) {
    let now = crate::report::boot_ms() as u64;
    if let Some(h) = filetree::filetree_handle() {
        let mut st = h.lock().unwrap();
        st.list_failed(dir, now);
        if dir.is_empty() {
            st.apply_root_list(
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
