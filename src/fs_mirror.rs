//! fs_mirror.rs — md 全量镜像核（BAR-213 v2，0139 口径全量放行）
//!
//! v1（BAR-187）只缓存「看过的」；v2 把服务器全量 md 镜像成
//! `<私有目录>/mirror/fs/` 下的**真 md 文件**（目录结构镜像，非 pct 缓存
//! json），断网时没看过的也能直读翻开（判卷点：断网翻开任意 md）。
//!
//! 本册全是纯核（A 档）：manifest 台账解析/落盘串、reconcile 三判计划、
//! 镜像路径映射、合成 list 出参。网络拉取与回退链接线在 `fs_fetch`。
//!
//! 宪法同 BAR-187：本地一切状态都是服务器真源的显示缓存——可旧、可缺，
//! 绝不与真源争对错；一期只读（写回/冲突另案）。
//!
//! 形制约定：
//! - 镜像树保持纯 md——manifest 台账放 `<mirror根>/fs.manifest.json`，
//!   **不设文件 mtime**（std 无法 set mtime，台账当对账凭据）；
//! - 合成 list 出参与 `fsapi::list_json_in` 同形同律（目录在前 + 大小写
//!   不敏感 + 原名 tie-break）——断网/在线两源行序不许跳变。

use std::path::{Path, PathBuf};

/// 镜像态声明头（与 sess_pool::CACHE_NOTICE 一族，措辞区分「镜像」与「缓存」）
pub const MIRROR_NOTICE: &str = "> （本地镜像副本，联网后自动同步）\n\n";

/// 台账条目（对账键 = path，新旧判 = size+mtime 双同才算没变）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

/// manifest 台账（entries 按 path 序——确定性落盘，diff 不抖）
#[derive(Debug, Default, Clone)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// 查条目（entries 按 path 序，二分）
    pub fn get(&self, path: &str) -> Option<&ManifestEntry> {
        self.entries
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.entries[i])
    }

    /// 台账落盘串（version 键留演进余地）
    pub fn to_json(&self) -> String {
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|e| serde_json::json!({"path": e.path, "size": e.size, "mtime": e.mtime}))
            .collect();
        serde_json::json!({"version": 1, "entries": entries}).to_string()
    }

    /// 台账解析：坏 JSON/缺 entries/条目缺键 = **空台账**（缓存是加强不是
    /// 命脉——台账坏了 = 全量重拉一遍，不许当障更不许 panic）
    pub fn parse(body: &str) -> Manifest {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
            return Manifest::default();
        };
        let Some(arr) = v.get("entries").and_then(|e| e.as_array()) else {
            return Manifest::default();
        };
        let mut entries = Vec::with_capacity(arr.len());
        for it in arr {
            let (Some(p), Some(s), Some(m)) = (
                it.get("path").and_then(|p| p.as_str()),
                it.get("size").and_then(|s| s.as_u64()),
                it.get("mtime").and_then(|m| m.as_i64()),
            ) else {
                continue; // 半条跳过
            };
            entries.push(ManifestEntry {
                path: p.to_string(),
                size: s,
                mtime: m,
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries.dedup_by(|a, b| a.path == b.path);
        Manifest { entries }
    }
}

/// manifest 台账路径：`<mirror根>/fs.manifest.json`（镜像树 fs/ 保持纯 md）
pub fn manifest_path(mirror_root: &Path) -> PathBuf {
    mirror_root.join("fs.manifest.json")
}

/// 镜像文件相对路径（`<mirror根>/fs/<path>`）：path 过 `fsapi::safe_rel`
/// 同一闸——绝对路径/`..` 逃逸一律 None（镜像映射不许出 mirror_root）
pub fn mirror_file_rel(path: &str) -> Option<String> {
    na_protocol::fsapi::safe_rel(path)?;
    Some(format!("fs/{path}"))
}

/// walk 端点出参 → 远端清单：body 坏 = None（同步整趟放弃，下一趟自愈）；
/// 条目缺键 = 跳过该条（半条不进对账）
pub fn walk_entries_of(body: &str) -> Option<Vec<ManifestEntry>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
        return None;
    }
    let arr = v.get("entries")?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for it in arr {
        let (Some(p), Some(s), Some(m)) = (
            it.get("path").and_then(|p| p.as_str()),
            it.get("size").and_then(|s| s.as_u64()),
            it.get("mtime").and_then(|m| m.as_i64()),
        ) else {
            continue;
        };
        out.push(ManifestEntry {
            path: p.to_string(),
            size: s,
            mtime: m,
        });
    }
    Some(out)
}

/// reconcile 计划：fetch = 要拉的（新/变/本地缺），delete = 要删的
/// （远端消失）。计划即账——执行侧照单动手，不二次判
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReconcilePlan {
    pub fetch: Vec<ManifestEntry>,
    pub delete: Vec<String>,
}

/// reconcile 三判（纯核；`exists` 注入本地存在性判——考题不必真建文件）：
/// ① 远端有、台账无 = 新 → 拉；② 台账有但 size/mtime 任一不同 = 变 → 拉；
/// ③ 台账有但本地文件缺（被清/首装）= 拉；④ 台账有、远端无 = 消失 → 删
pub fn reconcile(
    remote: &[ManifestEntry],
    manifest: &Manifest,
    exists: impl Fn(&str) -> bool,
) -> ReconcilePlan {
    let mut plan = ReconcilePlan::default();
    for r in remote {
        let stale = match manifest.get(&r.path) {
            None => true,
            Some(m) => m.size != r.size || m.mtime != r.mtime,
        };
        if stale || !exists(&r.path) {
            plan.fetch.push(r.clone());
        }
    }
    for m in &manifest.entries {
        if !remote.iter().any(|r| r.path == m.path) {
            plan.delete.push(m.path.clone());
        }
    }
    plan
}

/// 合成 list 出参（断网回退喂 `filetree::entries_of`）：从台账推出 `dir`
/// 的直接子层，出参与 `fsapi::list_json_in` 同形同律——目录在前、组内
/// 大小写不敏感字母序、同键回落原名序（排序单源同律，两源行序不跳变）。
/// 合成目录条目：size=0，mtime=子孙文件最大 mtime（有信息量的确定值）。
/// 返回值：`None` = 镜像里查无此目录（含台账为空）——调用方落旧错误路
pub fn synth_list_body(manifest: &Manifest, dir: &str) -> Option<String> {
    if manifest.entries.is_empty() {
        return None;
    }
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    // name → (is_dir, size, mtime)
    let mut kids: Vec<(String, bool, u64, i64)> = Vec::new();
    let mut any = false;
    for e in &manifest.entries {
        let Some(rest) = e.path.strip_prefix(&prefix) else {
            continue;
        };
        any = true;
        match rest.split_once('/') {
            Some((d, _)) => {
                if let Some(k) = kids.iter_mut().find(|k| k.0 == d && k.1) {
                    k.3 = k.3.max(e.mtime);
                } else {
                    kids.push((d.to_string(), true, 0, e.mtime));
                }
            }
            None => kids.push((rest.to_string(), false, e.size, e.mtime)),
        }
    }
    if !any {
        return None; // 该目录镜像里没有任何子孙 = 查无此目录
    }
    kids.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
            .then_with(|| a.0.cmp(&b.0))
    });
    let entries: Vec<_> = kids
        .into_iter()
        .map(|(name, is_dir, size, mtime)| {
            serde_json::json!({
                "name": name,
                "kind": if is_dir { "dir" } else { "file" },
                "size": size,
                "mtime": mtime,
            })
        })
        .collect();
    Some(serde_json::json!({"ok": true, "dir": dir, "entries": entries}).to_string())
}
