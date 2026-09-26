//! fsapi.rs — 文件树数据面的纯函数核（`GET /api/fs/list`、`GET /api/fs/read`）
//!
//! 这是什么：两条只读端点的**全部语义**——允许根解析、路径安全闸、排除规则、
//! 列目录/读文件出参、query 编解码。没有一行网络代码。
//!
//! 为什么在 na-protocol 而不在 na-server：na-protocol 是双端共享的纯函数落点
//! （客户端要用同一份 `pct_decode`/query 形状/出参解析，各写一份必然漂移）。
//! 它并非零依赖（serde_json 早已在），本模块只用 `std::fs` + `std::path`——
//! 无网络、无平台依赖，「核心层禁碰平台依赖」纪律不破。
//!
//! 为什么全是同步函数：文件 IO 就是同步的。**调用方负责别把 `std::fs` 直接
//! 跑在 current_thread 运行时上**——na-server 的 handler 用
//! `tokio::task::spawn_blocking` 包住（见 crates/na-server/src/main.rs）。
//!
//! 安全模型（照 nz `src/server/fs.ts`，判据稿 §二）：路径一律相对允许根；
//! `..`/绝对路径/根组件在 `safe_rel` 就拒；`canonicalize` 后必须落在根内
//! （防软链逃逸）；越界与不存在返回同一个 `FsError::NotFound`，不透露存在性。
//!
//! 已知差异（与 nz/Node 实现）：
//! - 排序用 Rust `sort()` = 字节序（UTF-8 码点序）。Node 的 `sort()` 是 UTF-16
//!   码元序——ASCII/基本平面一致，只有 U+10000 以上（代理对）与
//!   U+E000..U+FFFF 混排时理论序不同，文件树按名排的实测影响可忽略。
//! - `?max=12.9` 在 nz 取 floor=12，这里视为坏值回落缺省（整数语义更窄更安全）。
//! - 出参键序 = `serde_json` 的字母序（`json!` 走 BTreeMap），与 nz 手写对象
//!   的字面序不同——JSON 对象键序无语义，两端都走 serde 解析，键集/类型一致
//!   即为同契约（`{"ok","dir","entries"}` / `{"ok","path","binary","truncated",
//!   "size","text"}` 的键一个不少）。
//!
//! 考题：crates/na-protocol/tests/fsapi_spec.rs（A 档，带变异抽检）。

use std::path::{Component, Path, PathBuf};

/// `?max=` 缺省值：64KB（判据稿 §2.2 同款）
pub const DEFAULT_MAX: usize = 65536;
/// `?max=` 上限 1MB——单次预览的读放大封顶（nz 的 1MB 读窗同源）
pub const MAX_MAX: usize = 1024 * 1024;

/// 文件面错误：越界与不存在**必须是同一种**（NotFound），不透露存在性。
/// 映射：NotFound / NotDir → 404 同一文案；Io → 500 显形内部故障。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotDir,
    Io(String),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::NotFound => write!(f, "not found"),
            FsError::NotDir => write!(f, "路径类型不符（列目录要目录 / 读文件要普通文件）"),
            FsError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for FsError {}

/// 段级排除清单（判据稿 §2.1；任何 `.` 开头的名字另行拦下）
const EXCLUDE_DIRS: [&str; 5] = [".obsidian", ".smart-env", ".trash", "node_modules", ".git"];

/// 允许根（每请求现读 env——考题/运营可在运行时改）：
/// `NA_FS_ROOTS` 冒号分隔覆盖；缺省 = 库本体存在则用它，否则 `$HOME`。
/// env 设成空串 = 显式零根（fail-closed，一切 404），不退缺省。
pub fn roots() -> Vec<PathBuf> {
    let spec = std::env::var("NA_FS_ROOTS").ok();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    roots_from(spec.as_deref(), Path::new(&home))
}

/// `roots()` 的纯核（考题可注入，不必改进程 env——env 在并行考题里不安全）：
/// 缺省根照 nz 定案收窄到库本体 `$HOME/00-Loyintina`（服务器 HOME=/root
/// ⇒ `/root/00-Loyintina`），库不在才退回 HOME（全量 HOME 会把源码树/
/// toolchain 全索进索引，nz 8.3MB 索引实锤）。条目可为相对路径，按进程
/// cwd 解析（canonicalize 在 resolve 侧做）。
pub fn roots_from(spec: Option<&str>, home: &Path) -> Vec<PathBuf> {
    if let Some(spec) = spec {
        return spec
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    let lib = home.join("00-Loyintina");
    if lib.exists() {
        vec![lib]
    } else {
        vec![home.to_path_buf()]
    }
}

/// 排除判定：隐藏项（`.` 开头）或段级命中清单（nz 同款：命中即整枝剪除）
pub fn excluded(name: &str) -> bool {
    name.starts_with('.') || EXCLUDE_DIRS.contains(&name)
}

/// 相对路径安全闸：拒空/绝对路径/任何 `..` 或根组件；返回归一化后的相对路径
/// （不落盘、不拼根——拼根在 `resolve_in`，因为要逐根试）。
/// 结果可能为空（如 `"."`），调用方按拒处理。
pub fn safe_rel(rel: &str) -> Option<PathBuf> {
    if rel.is_empty() {
        return None;
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            // `.` 段跳过（components 已吃掉 `a/./b`，只剩前导 `./`）；
            // `..`/根/盘符前缀 = 逃逸企图，一律拒
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// 在允许根内解析：命中根 → canonicalize 后的绝对路径。全不中 → NotFound。
/// `rel` 空串 = 列根目录本身（根即允许根，安全）。
pub fn resolve(rel: &str) -> Result<PathBuf, FsError> {
    resolve_in(&roots(), rel)
}

/// `resolve()` 的纯核：逐根试；`canonicalize` 成功且落在该根 canonical 之内
/// 才算命中（软链逃出根 = 与不存在同错）。返回 canonical 路径而非拼接路径——
/// 后续 open/readdir 也走 real，软链换靶的窗口收窄一档。
pub fn resolve_in(roots: &[PathBuf], rel: &str) -> Result<PathBuf, FsError> {
    for root in roots {
        let abs = if rel.is_empty() {
            root.clone()
        } else {
            root.join(safe_rel(rel).ok_or(FsError::NotFound)?)
        };
        let (Ok(real), Ok(root_real)) = (std::fs::canonicalize(&abs), std::fs::canonicalize(root))
        else {
            continue; // 不存在/根不可达 → 换下一个根，绝不透露原因
        };
        if real.starts_with(&root_real) {
            return Ok(real);
        }
    }
    Err(FsError::NotFound)
}

/// `GET /api/fs/list`：只列**直接子层**，排除规则过滤，按名字排序。
/// 出参 `{"ok":true,"dir":rel,"entries":[{name,kind,size,mtime}]}`。
pub fn list_json(rel: &str) -> Result<String, FsError> {
    list_json_in(&roots(), rel)
}

/// `list_json()` 的纯核。被列对象必须是目录，否则 NotDir（HTTP 层同样 404）。
/// 条目走 `metadata`（跟软链：根内软链照常列出，断了/无权限的跳过）——
/// 与 nz 同款；逃逸软链由 `resolve_in` 在进入时拦下。
pub fn list_json_in(roots: &[PathBuf], rel: &str) -> Result<String, FsError> {
    let real = resolve_in(roots, rel)?;
    let meta = std::fs::metadata(&real).map_err(|_| FsError::NotFound)?;
    if !meta.is_dir() {
        return Err(FsError::NotDir);
    }
    let rd = std::fs::read_dir(&real).map_err(|e| FsError::Io(e.to_string()))?;
    let mut names: Vec<String> = Vec::new();
    for ent in rd {
        let Ok(ent) = ent else { continue }; // 竞态消失的条目跳过
        let name = ent.file_name().to_string_lossy().into_owned();
        if excluded(&name) {
            continue;
        }
        names.push(name);
    }
    names.sort();
    let mut entries = Vec::new();
    for name in names {
        let Ok(m) = std::fs::metadata(real.join(&name)) else {
            continue; // 列出后消失/无权限：跳过不留半条
        };
        entries.push(serde_json::json!({
            "name": name,
            "kind": if m.is_dir() { "dir" } else { "file" },
            "size": m.len(),
            "mtime": mtime_ms(&m),
        }));
    }
    Ok(serde_json::json!({"ok": true, "dir": rel, "entries": entries}).to_string())
}

/// `GET /api/fs/read?path=&max=`：文本预览（NUL 探测二进制；max 截断）。
/// 文本出参 `{"ok":true,"path","binary":false,"truncated","size","text"}`；
/// 二进制不带 text。`max` 由调用方给（HTTP 层缺省 DEFAULT_MAX、上限 MAX_MAX）。
pub fn read_json(rel: &str, max: usize) -> Result<String, FsError> {
    read_json_in(&roots(), rel, max)
}

/// `read_json()` 的纯核。读窗 = `min(max+1, size, 1MB)`（多读 1 字节才能分辨
/// 「刚好读完」与「被 max 截断」）；被读对象必须是普通文件，否则 NotDir。
pub fn read_json_in(roots: &[PathBuf], rel: &str, max: usize) -> Result<String, FsError> {
    use std::io::Read as _;

    let real = resolve_in(roots, rel)?;
    let meta = std::fs::metadata(&real).map_err(|_| FsError::NotFound)?;
    if !meta.is_file() {
        return Err(FsError::NotDir);
    }
    let size = meta.len();
    let cap = max.min(MAX_MAX);
    let want = (cap.saturating_add(1) as u64).min(size).min(MAX_MAX as u64);
    let mut buf = Vec::new();
    std::fs::File::open(&real)
        .and_then(|f| f.take(want).read_to_end(&mut buf))
        .map_err(|e| FsError::Io(e.to_string()))?;
    let read = buf.len();
    // 截断 = 读到的比 max 多（说明还有下续）或 读到的比文件短（1MB 读窗封顶）
    let truncated = read > cap || (read as u64) < size;
    let binary = buf.contains(&0);
    let out = if binary {
        serde_json::json!({
            "ok": true, "path": rel, "binary": true,
            "truncated": truncated, "size": size,
        })
    } else {
        serde_json::json!({
            "ok": true, "path": rel, "binary": false,
            "truncated": truncated, "size": size,
            "text": text_prefix(&buf, cap),
        })
    };
    Ok(out.to_string())
}

/// 取前 `limit` 字节内的 UTF-8 文本，**绝不劈出半个字**：
/// - 整段合法 → 原样；
/// - 只被 max 切在多字节序列上（`error_len=None`）→ 收到上一个完整字符；
/// - 内容本来就有坏字节 → lossy 出 U+FFFD 显形（不静默吞掉后半文件）。
fn text_prefix(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(s) => s.to_string(),
        Err(e) if e.error_len().is_none() => {
            String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned()
        }
        Err(_) => String::from_utf8_lossy(&bytes[..end]).into_owned(),
    }
}

/// mtime = 毫秒 epoch（与 nz `Math.floor(mtimeMs)` 同一口径）；时钟异常 → 0
fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 解析 query 段为键值对：先按 `&` 切、再按**第一个** `=` 切、**最后**百分号
/// 解码值。顺序不可换——值与键先解码会把 `%26`/`%3D` 解成分隔符，参数拆错。
/// 空段跳过；无 `=` 的段值取空串；键保持原样不解码（面的入参键都是 ASCII 字面）。
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k.to_string(), pct_decode(v))
        })
        .collect()
}

/// 取 query 上某键的第一个值（缺键 None）
pub fn query_get(query: &str, key: &str) -> Option<String> {
    parse_query(query)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// 百分号解码：非法 `%` 序列原样保留（语义照 na-agentd httpd.rs:82-107，
/// 那份是私有函数跨 crate 用不了；越界/短尾一律不收）
pub fn pct_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bs = s.as_bytes();
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

/// 百分号编码（客户端的 `pct_decode` 对偶）：查询值进 URL 前必须过它——
/// 中文目录名/含 `&`/`=`/`#`/空格的路径不编码就会被服务端拆错参数。
/// 保留字比 RFC 3986 的 unreserved 稍严（只放行 `A-Za-z0-9-._~`），
/// 空格编 `%20` 不编 `+`（服务端按 RFC 3986 解，`+` 不是空格）。
pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `?max=` 解析：缺省/非数字/0 回落 `DEFAULT_MAX`，超上限钳 `MAX_MAX`
/// （钳在解析侧，路由出的值本身就在合法域内）
pub fn parse_max(query: &str) -> usize {
    query_get(query, "max")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .map(|n| n.min(MAX_MAX))
        .unwrap_or(DEFAULT_MAX)
}
