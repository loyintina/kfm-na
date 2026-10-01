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
/// `NA_FS_ROOTS` 冒号分隔覆盖；**缺省 = `/root`**。
/// env 设成空串 = 显式零根（fail-closed，一切 404），不退缺省。
pub fn roots() -> Vec<PathBuf> {
    roots_from(std::env::var("NA_FS_ROOTS").ok().as_deref())
}

/// `roots()` 的纯核（考题可注入，不必改进程 env——env 在并行考题里不安全）。
///
/// **缺省根 = `/root`，不依赖 HOME**（2026-09-27 用户裁决，BAR-165 打回
/// 定罪）：服务端跑在 systemd 里，unit 没写 `Environment=HOME`——旧实现
/// 「HOME 缺省回退 `/`」于是把根落到**根文件系统**，真机打开的是 44 条
/// 系统目录（logcat「列目录到位 "" 条目 44」铁证）。nz 那套「收窄到库本体
/// `$HOME/00-Loyintina`」一并作废：用户要的是 `/root` 本身（00-Loyintina
/// 是它的孩子，看得见、点得进）。条目可为相对路径，按进程 cwd 解析
/// （canonicalize 在 resolve 侧做）。
pub fn roots_from(spec: Option<&str>) -> Vec<PathBuf> {
    if let Some(spec) = spec {
        return spec
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    vec![PathBuf::from("/root")]
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
    // 先各 stat 一次（排序要 kind；列完再 stat 会两边重复系统调用），
    // 顺手拿到 name/dir 位
    let mut raw: Vec<(String, bool)> = Vec::new();
    for ent in rd {
        let Ok(ent) = ent else { continue }; // 竞态消失的条目跳过
        let name = ent.file_name().to_string_lossy().into_owned();
        if excluded(&name) {
            continue;
        }
        let Ok(m) = std::fs::metadata(real.join(&name)) else {
            continue; // 列出后消失/无权限：跳过不留半条
        };
        raw.push((name, m.is_dir()));
    }
    // **三修症④（2026-09-27 用户拍板）**：目录在前、文件在后；组内**大小写
    // 不敏感**字母序（`to_lowercase()` 是 Unicode 感知的，CJK 原样 → 按
    // Unicode 码位比），同键回落原名序（`A` vs `a` 确定性）。排序**只此一处**
    // （数据面单源），客户端行构照单排，不二次排
    raw.sort_by(|a, b| {
        b.1.cmp(&a.1) // dir(true) 在前
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut entries = Vec::new();
    for (name, is_dir) in raw {
        let Ok(m) = std::fs::metadata(real.join(&name)) else {
            continue;
        };
        entries.push(serde_json::json!({
            "name": name,
            "kind": if is_dir { "dir" } else { "file" },
            "size": m.len(),
            "mtime": mtime_ms(&m),
        }));
    }
    Ok(serde_json::json!({"ok": true, "dir": rel, "entries": entries}).to_string())
}

/// `GET /api/fs/walk?ext=.md`：递归清单（BAR-213 md 全量镜像数据面）——
/// 全根递归遍历，排除规则同一 `excluded()` 段级剪枝，只收 ext 后缀
/// （大小写不敏感）的普通文件，出参按路径序（确定性对账键）。
/// 出参 `{"ok":true,"ext":ext,"entries":[{path,size,mtime}]}`。
/// 软链不跟（目录软链整枝剪 = 不许借链出根；文件软链同剪——镜像语义
/// 要真文件，链目标可能在根外）。
///
/// **keyset 分页（BAR-213 翻案 NA0145 甲案，承影判卷 lane 倾向正治）**：
/// `after=<path>`（开区间，吃上一页 next_after）+ `limit=<n>`（0 = 不分页
/// = 旧契约整单，供在野乙案客户端兜底）；分页模式且后面还有 → 出参多
/// `next_after` 键 = 本页末条 path（客户端循环收页到无此键为止）。
/// keyset 不吃位移账——翻页期间树变了也只是本页边界新旧差，reconcile
/// 下一趟自愈（镜像语义本来就不与真源争对错）。
pub fn walk_json(ext: &str, after: Option<&str>, limit: usize) -> Result<String, FsError> {
    walk_json_in(&roots(), ext, after, limit)
}

/// `walk_json()` 的纯核（考题注入 roots，不必改进程 env）。
/// ext 闸 fail-closed：必须 `.` 开头 + 其余 1..=8 位全 ASCII 字母数字
/// ——形状非法与越界同一条 NotFound（不透露口存在性）。
pub fn walk_json_in(
    roots: &[PathBuf],
    ext: &str,
    after: Option<&str>,
    limit: usize,
) -> Result<String, FsError> {
    if !valid_walk_ext(ext) {
        return Err(FsError::NotFound);
    }
    let mut raw: Vec<(String, u64, i64)> = Vec::new();
    for root in roots {
        // 根本身读不出 = 该根缺席（多根逐试同 list 律），不连坐其余根
        if std::fs::metadata(root).map(|m| m.is_dir()).unwrap_or(false) {
            walk_dir(root, root, ext, &mut raw)?;
        }
    }
    raw.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(a) = after {
        raw.retain(|(p, _, _)| p.as_str() > a);
    }
    let more = limit > 0 && raw.len() > limit;
    if more {
        raw.truncate(limit);
    }
    let next_after = more.then(|| raw.last().unwrap().0.clone());
    let entries: Vec<_> = raw
        .into_iter()
        .map(|(path, size, mtime)| serde_json::json!({"path": path, "size": size, "mtime": mtime}))
        .collect();
    let mut out = serde_json::json!({"ok": true, "ext": ext, "entries": entries});
    if let Some(n) = next_after {
        out["next_after"] = serde_json::Value::String(n);
    }
    Ok(out.to_string())
}

/// ext 闸纯核：`.md` 合法；空/无点/带点外字符/超 8 位全拒。
fn valid_walk_ext(ext: &str) -> bool {
    let Some(rest) = ext.strip_prefix('.') else {
        return false;
    };
    !rest.is_empty() && rest.len() <= 8 && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// walk 分页页长上限（NA0145 甲案）：防万位页撑爆单页 body；客户端
/// 页长 2000（fs_fetch::WALK_PAGE_LIMIT），上限留五倍余量
pub const WALK_PAGE_LIMIT_MAX: usize = 10000;

/// 递归体：逐层 read_dir，排除规则剪枝，ext 后缀（大小写不敏感）收文件。
/// 嵌套层 read_dir 失败跳过（竞态消失/权限——reconcile 下一趟自愈）；
/// symlink_metadata 判型，软链一律不跟。
fn walk_dir(
    root: &Path,
    dir: &Path,
    ext: &str,
    out: &mut Vec<(String, u64, i64)>,
) -> Result<(), FsError> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for ent in rd {
        let Ok(ent) = ent else { continue };
        let name = ent.file_name().to_string_lossy().into_owned();
        if excluded(&name) {
            continue;
        }
        let p = dir.join(&name);
        let Ok(m) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if m.file_type().is_symlink() {
            continue;
        }
        if m.is_dir() {
            walk_dir(root, &p, ext, out)?;
        } else if m.is_file()
            && name.len() > ext.len()
            && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext)
        {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, m.len(), mtime_ms(&m)));
        }
    }
    Ok(())
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
    text_prefix_metered(bytes, limit).0
}

/// `text_prefix` 的计量版（BAR-170 分块读）：返回（文本, **消费的源字节数**）。
/// 消费量必须走原始侧——lossy 显形时出参文本字节数 ≠ 源字节数（1 个坏字节
/// 变 3 字节的 U+FFFD），拿 text.len() 当消费量会把下一块的 offset 记飞：
/// - 整段合法 → 消费 = 段长；
/// - 切在多字节序列上 → 消费 = valid_up_to（半字留给下一块，**字界不变式**：
///   本函数保证 offset 恒指字界，下一块从完整字符起读）；
/// - 坏字节 → 消费 = 段长（U+FFFD 已顶了坏字节的账，**不许重读**——重读会在
///   每块边界把同一个坏字节反复显形）。
fn text_prefix_metered(bytes: &[u8], limit: usize) -> (String, usize) {
    let end = bytes.len().min(limit);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(s) => (s.to_string(), end),
        Err(e) if e.error_len().is_none() => (
            String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned(),
            e.valid_up_to(),
        ),
        Err(_) => (String::from_utf8_lossy(&bytes[..end]).into_owned(), end),
    }
}

/// `GET /api/fs/read?path=&offset=&max=` 的分块变体（BAR-170 阅读页）：
/// 从 `offset`（字节，协议不变式 = 恒为 UTF-8 字界，由出参 `next_offset`
/// 维持）起读至多 `max` 字节。出参比 `read_json` 多 `offset`/`next_offset`
/// 两键；`truncated = next_offset < size`（后面还有货）。
/// **offset ≥ size（非空文件）= NotFound**——越界与不存在同一条 404 同文案
/// （不透露存在性）；空文件 offset=0 合法，出空块 `truncated=false`。
pub fn read_range_json(rel: &str, offset: u64, max: usize) -> Result<String, FsError> {
    read_range_json_in(&roots(), rel, offset, max)
}

/// `read_range_json()` 的纯核。读窗 = `min(max+1, size−offset, 1MB)`
/// （多读 1 字节分辨「刚好读完」与「被 max 截断」，与 read_json_in 同律）。
pub fn read_range_json_in(
    roots: &[PathBuf],
    rel: &str,
    offset: u64,
    max: usize,
) -> Result<String, FsError> {
    use std::io::{Read as _, Seek as _};

    let real = resolve_in(roots, rel)?;
    let meta = std::fs::metadata(&real).map_err(|_| FsError::NotFound)?;
    if !meta.is_file() {
        return Err(FsError::NotDir);
    }
    let size = meta.len();
    if offset >= size && size > 0 {
        return Err(FsError::NotFound);
    }
    // cap 下限 8：max 小于一个 UTF-8 字宽时字界收口会零消费（下一块原地
    // 不动 = 死循环，考题 spec_bar170_字界不变式 实捕）。分块协议的**前进性**
    // 优先于块宽自律——两个最宽字符（4B×2）必能整块通过，消费 ≥ 4。
    // （read_json_in 无此下限：它是一枪读，没有前进性契约）
    let cap = max.min(MAX_MAX).clamp(8, MAX_MAX);
    let want = (cap.saturating_add(1) as u64)
        .min(size - offset)
        .min(MAX_MAX as u64);
    let mut buf = Vec::new();
    std::fs::File::open(&real)
        .and_then(|mut f| {
            f.seek(std::io::SeekFrom::Start(offset))?;
            f.take(want).read_to_end(&mut buf)
        })
        .map_err(|e| FsError::Io(e.to_string()))?;
    let binary = buf.contains(&0);
    if binary {
        // 二进制不出 text；next_offset 按读窗给（消费方只看首块的 binary 旗，
        // 见到即停，后续块不会来）
        let next_offset = offset + buf.len().min(cap) as u64;
        return Ok(serde_json::json!({
            "ok": true, "path": rel, "binary": true,
            "truncated": next_offset < size, "size": size,
            "offset": offset, "next_offset": next_offset,
        })
        .to_string());
    }
    let (text, consumed) = text_prefix_metered(&buf, cap);
    let next_offset = offset + consumed as u64;
    Ok(serde_json::json!({
        "ok": true, "path": rel, "binary": false,
        "truncated": next_offset < size, "size": size,
        "offset": offset, "next_offset": next_offset,
        "text": text,
    })
    .to_string())
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

/// `?offset=` 解析（BAR-170 分块读）：缺省/非数字 = 0——**缺省即旧行为**，
/// 分块面对旧客户端是纯粹加法
pub fn parse_offset(query: &str) -> u64 {
    query_get(query, "offset")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

/// query 里**显式带了** offset 键（哪怕值是 0/非数字）= 新契约分块读
/// （BAR-170 定罪：新客户端首块也是 offset=0，拿「值是不是 0」当新旧
/// 分野会把首块喂旧契约——出参缺 next_offset 键，客户端报出参缺键。
/// 分野只能是「键在不在」：旧客户端（kfmv4/查看器）根本不带这个键）
pub fn has_offset(query: &str) -> bool {
    query_get(query, "offset").is_some()
}
