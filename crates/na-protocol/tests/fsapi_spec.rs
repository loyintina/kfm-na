//! crates/na-protocol/tests/fsapi_spec.rs — A 档考题：文件树数据面纯函数核
//!
//! 答案区：crates/na-protocol/src/fsapi.rs。本文件是考题，生成器不许改。
//!
//! 造真目录树在 tempdir 里，用 `*_in(roots, …)` 注入根（考题不许改进程 env——
//! 并行考题里 set_var 是竞态源）；`roots_from`/`parse_*`/`pct_decode` 等无 IO
//! 的面直接打公开函数。

use std::path::PathBuf;

use na_protocol::fsapi;
use serde_json::Value;
use tempfile::TempDir;

/// 造一棵样例树：
/// ```text
/// root/
///   A.txt        "A"
///   a.txt        "a"
///   .hidden      (排除)
///   sub/         (内含 deep.txt —— 只列直接子层时不许出现)
///   node_modules/ (排除)
///   .git/         (排除)
/// ```
fn sample_tree() -> TempDir {
    let td = TempDir::new().expect("临时目录");
    let r = td.path();
    std::fs::write(r.join("A.txt"), "A").unwrap();
    std::fs::write(r.join("a.txt"), "a").unwrap();
    std::fs::write(r.join(".hidden"), "hidden").unwrap();
    std::fs::create_dir(r.join("sub")).unwrap();
    std::fs::write(r.join("sub/deep.txt"), "deep").unwrap();
    std::fs::create_dir(r.join("node_modules")).unwrap();
    std::fs::write(r.join("node_modules/x.js"), "x").unwrap();
    std::fs::create_dir(r.join(".git")).unwrap();
    std::fs::write(r.join(".git/HEAD"), "ref").unwrap();
    td
}

fn roots_of(td: &TempDir) -> Vec<PathBuf> {
    vec![td.path().to_path_buf()]
}

fn list(td: &TempDir, rel: &str) -> Value {
    let s = fsapi::list_json_in(&roots_of(td), rel).expect("列目录成功");
    serde_json::from_str(&s).expect("list 出参是合法 JSON")
}

fn names(v: &Value) -> Vec<String> {
    v["entries"]
        .as_array()
        .expect("entries 是数组")
        .iter()
        .map(|e| e["name"].as_str().expect("name 是字符串").to_string())
        .collect()
}

/// 出参 kind 序列（症④ 结构断言用：目录全在文件之前）
fn kinds(v: &Value) -> Vec<String> {
    v["entries"]
        .as_array()
        .expect("entries 是数组")
        .iter()
        .map(|e| e["kind"].as_str().expect("kind 是字符串").to_string())
        .collect()
}

// ---------- ① 只列直接子层 ----------

#[test]
fn spec_list_只列直接子层() {
    let td = sample_tree();
    let v = list(&td, "");
    assert_eq!(v["ok"], true);
    assert_eq!(v["dir"], "");
    assert_eq!(
        names(&v),
        vec!["sub".to_string(), "A.txt".to_string(), "a.txt".to_string()],
        "子目录里的东西不许冒头；**症④改约**：目录在前、文件在后"
    );
    let sub = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "sub")
        .expect("sub 在列");
    assert_eq!(sub["kind"], "dir");
    assert!(sub["size"].is_u64(), "size 永远在");
    assert!(sub["mtime"].is_i64(), "mtime 永远在");
    // 下钻一层：只出现 deep.txt
    let v2 = list(&td, "sub");
    assert_eq!(v2["dir"], "sub");
    assert_eq!(names(&v2), vec!["deep.txt".to_string()]);
    assert_eq!(v2["entries"][0]["kind"], "file");
}

// ---------- ② 排除规则 ----------

#[test]
fn spec_排除规则命中() {
    let td = sample_tree();
    let got = names(&list(&td, ""));
    for bad in [".git", "node_modules", ".hidden"] {
        assert!(!got.contains(&bad.to_string()), "{bad} 必须被排除");
    }
    for hit in [
        ".obsidian",
        ".smart-env",
        ".trash",
        "node_modules",
        ".git",
        ".any-hidden",
    ] {
        assert!(fsapi::excluded(hit), "{hit} 该排除");
    }
    for keep in [
        "src",
        "a.txt",
        "Cargo.toml",
        "node_modules.bak",
        "node_modulesx",
        "trash",
    ] {
        assert!(!fsapi::excluded(keep), "{keep} 不许误伤");
    }
}

// ---------- ③ 越界 = 不存在（同一种错） ----------

#[test]
fn spec_越界与不存在同错() {
    let td = sample_tree();
    let roots = roots_of(&td);
    assert_eq!(
        fsapi::list_json_in(&roots, "nope"),
        Err(fsapi::FsError::NotFound),
        "不存在的路径 = NotFound（基准样本）"
    );
    for evil in ["..", "../", "../../etc", "sub/../..", "/etc", "/", "..\\.."] {
        assert_eq!(
            fsapi::list_json_in(&roots, evil),
            Err(fsapi::FsError::NotFound),
            "越界 {evil} 必须与不存在同一种错（不透露存在性）"
        );
        assert_eq!(
            fsapi::read_json_in(&roots, evil, 1024),
            Err(fsapi::FsError::NotFound),
            "read 面同闸: {evil}"
        );
        assert_eq!(
            fsapi::resolve_in(&roots, evil),
            Err(fsapi::FsError::NotFound)
        );
    }
    // 类型不符与越界在纯函数层是两种错，但 HTTP 层塌成同一 404（na-server 考题钉）
}

#[test]
fn spec_safe_rel_闸() {
    assert_eq!(fsapi::safe_rel(""), None, "空路径拒");
    assert_eq!(fsapi::safe_rel("/etc/passwd"), None, "绝对路径拒");
    assert_eq!(fsapi::safe_rel(".."), None);
    assert_eq!(fsapi::safe_rel("../x"), None);
    assert_eq!(
        fsapi::safe_rel("a/../b"),
        None,
        "中途 .. 也拒（不许靠归一化蒙混）"
    );
    assert_eq!(fsapi::safe_rel("a/../../b"), None);
    assert_eq!(fsapi::safe_rel("."), None, "归一后为空 = 拒");
    assert_eq!(fsapi::safe_rel("a/b"), Some(PathBuf::from("a/b")));
    assert_eq!(
        fsapi::safe_rel("./a"),
        Some(PathBuf::from("a")),
        "前导 ./ 归一掉"
    );
}

// ---------- ④ 软链逃出根 ----------

#[cfg(unix)]
#[test]
fn spec_软链逃出根即未命中() {
    let outside = TempDir::new().expect("根外目录");
    std::fs::write(outside.path().join("secret.txt"), "top secret").unwrap();
    let td = sample_tree();
    // 根内软链指向根外目录 + 根外文件
    std::os::unix::fs::symlink(outside.path(), td.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        td.path().join("leak.txt"),
    )
    .unwrap();
    let roots = roots_of(&td);
    assert_eq!(
        fsapi::resolve_in(&roots, "escape"),
        Err(fsapi::FsError::NotFound),
        "目录软链逃逸"
    );
    assert_eq!(
        fsapi::resolve_in(&roots, "escape/secret.txt"),
        Err(fsapi::FsError::NotFound),
        "穿软链读根外文件"
    );
    assert_eq!(
        fsapi::read_json_in(&roots, "leak.txt", 1024),
        Err(fsapi::FsError::NotFound),
        "文件软链逃逸"
    );
    // 根内软链（靶在根内）照常可用——防的是逃逸不是软链本身
    std::os::unix::fs::symlink(td.path().join("sub"), td.path().join("alias")).unwrap();
    assert!(
        fsapi::resolve_in(&roots, "alias/deep.txt").is_ok(),
        "根内软链可用"
    );
}

// ---------- ⑤ read 截断 ----------

#[test]
fn spec_read_截断标记() {
    let td = sample_tree();
    let roots = roots_of(&td);
    // 恰好 max+1 字节：读到的比 max 多、且没比文件短——只有 >max 判据咬得住
    std::fs::write(td.path().join("eleven.txt"), "0123456789A").unwrap();
    let v: Value = serde_json::from_str(&fsapi::read_json_in(&roots, "eleven.txt", 10).unwrap())
        .expect("合法 JSON");
    assert_eq!(v["ok"], true);
    assert_eq!(v["binary"], false);
    assert_eq!(v["truncated"], true, "size = max+1 时也算截断（>max 判据）");
    assert_eq!(v["size"], 11, "size 报全文长度不是读到的长度");
    assert_eq!(v["path"], "eleven.txt");
    assert_eq!(v["text"], "0123456789", "文本按 max 收口");
    // max == size：读完就不算截断（两个判据都不能误报）
    let v2: Value = serde_json::from_str(&fsapi::read_json_in(&roots, "eleven.txt", 11).unwrap())
        .expect("合法 JSON");
    assert_eq!(v2["truncated"], false, "刚好读完不是截断");
    assert_eq!(v2["text"], "0123456789A");
    // max 大于文件：全文、不截断
    let v3: Value = serde_json::from_str(&fsapi::read_json_in(&roots, "A.txt", 65536).unwrap())
        .expect("合法 JSON");
    assert_eq!(v3["truncated"], false);
    assert_eq!(v3["size"], 1);
    assert_eq!(v3["text"], "A");
}

#[test]
fn spec_read_按字符边界收口() {
    let td = sample_tree();
    let roots = roots_of(&td);
    // 12 个「中」（每个 3 字节，共 36 字节）；max 取不到 3 的倍数时必切在字中间
    std::fs::write(td.path().join("cjk.txt"), "中".repeat(12)).unwrap();
    for max in [1usize, 2, 3, 4, 5, 10, 35] {
        let v: Value =
            serde_json::from_str(&fsapi::read_json_in(&roots, "cjk.txt", max).unwrap()).unwrap();
        let t = v["text"].as_str().expect("text 是字符串");
        assert!(t.len() <= max, "max={max} 时文本字节数 {} 超 max", t.len());
        assert!(!t.contains('\u{FFFD}'), "max={max} 切出半个字: {t:?}");
        assert_eq!(t, "中".repeat(t.len() / 3), "max={max} 只收整数个中");
        assert_eq!(v["truncated"], true, "max={max} 小于 36 字节必截断");
        assert_eq!(v["size"], 36);
    }
    // 边界正好对齐：max=3 → 一个完整的中
    let v: Value =
        serde_json::from_str(&fsapi::read_json_in(&roots, "cjk.txt", 3).unwrap()).unwrap();
    assert_eq!(v["text"], "中");
}

#[test]
fn spec_read_坏字节不劈字也不吞后半() {
    let td = sample_tree();
    let roots = roots_of(&td);
    // 合法前缀 + 坏字节 + 合法尾巴：坏字节 lossy 显形（U+FFFD），不 panic 不吞后半
    std::fs::write(td.path().join("bad.txt"), b"ok\xff\xfetail").unwrap();
    let v: Value =
        serde_json::from_str(&fsapi::read_json_in(&roots, "bad.txt", 1024).unwrap()).unwrap();
    assert_eq!(v["truncated"], false);
    let t = v["text"].as_str().unwrap();
    assert!(t.starts_with("ok"), "{t:?}");
    assert!(t.ends_with("tail"), "坏字节不许吞掉后半: {t:?}");
}

// ---------- ⑥ NUL 探测 ----------

#[test]
fn spec_nul探测二进制() {
    let td = sample_tree();
    let roots = roots_of(&td);
    std::fs::write(td.path().join("blob.bin"), b"abc\x00def").unwrap();
    let v: Value =
        serde_json::from_str(&fsapi::read_json_in(&roots, "blob.bin", 1024).unwrap()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["binary"], true);
    assert!(v.get("text").is_none(), "二进制不许带 text 键");
    assert_eq!(v["size"], 7);
    assert_eq!(v["truncated"], false);
    assert_eq!(v["path"], "blob.bin");
    // NUL 在 max 窗口之外 → 窗口内是纯文本（判据只看读到的窗）
    std::fs::write(td.path().join("late.bin"), b"abcdefghij\x00").unwrap();
    let v2: Value =
        serde_json::from_str(&fsapi::read_json_in(&roots, "late.bin", 5).unwrap()).unwrap();
    assert_eq!(v2["binary"], false, "NUL 在窗外：窗外的不算");
    assert_eq!(v2["truncated"], true);
}

// ---------- ⑦ 根不存在 / 类型不符 ----------

#[test]
fn spec_根不存在即未命中() {
    let ghost = PathBuf::from("/nonexistent-root-kfm-na-spec");
    assert!(!ghost.exists());
    let roots = vec![ghost];
    assert_eq!(fsapi::resolve_in(&roots, ""), Err(fsapi::FsError::NotFound));
    assert_eq!(
        fsapi::list_json_in(&roots, ""),
        Err(fsapi::FsError::NotFound)
    );
    assert_eq!(
        fsapi::read_json_in(&roots, "x", 16),
        Err(fsapi::FsError::NotFound)
    );
    // 零根（NA_FS_ROOTS="" 的 fail-closed）同样全 NotFound
    assert_eq!(fsapi::resolve_in(&[], ""), Err(fsapi::FsError::NotFound));
}

#[test]
fn spec_类型不符与多根逐试() {
    let td = sample_tree();
    let roots = roots_of(&td);
    assert_eq!(
        fsapi::list_json_in(&roots, "A.txt"),
        Err(fsapi::FsError::NotDir),
        "列文件 = 类型不符"
    );
    assert_eq!(
        fsapi::read_json_in(&roots, "sub", 16),
        Err(fsapi::FsError::NotDir),
        "读目录 = 类型不符"
    );
    // 根要能列（rel=""）
    assert!(fsapi::list_json_in(&roots, "").is_ok());
    // 多根逐试：第一根没有的路径，第二根里有 → 命中（rel="" 命中的是第一个根）
    let td2 = TempDir::new().unwrap();
    std::fs::write(td2.path().join("only2.txt"), "2").unwrap();
    let two = vec![td.path().to_path_buf(), td2.path().to_path_buf()];
    assert_eq!(
        fsapi::resolve_in(&two, "only2.txt").ok(),
        Some(std::fs::canonicalize(td2.path().join("only2.txt")).unwrap()),
        "多根要逐根试"
    );
    // 两根都没有 = NotFound（不是「第一个根说了算」就完事）
    assert_eq!(
        fsapi::resolve_in(&two, "nowhere.txt"),
        Err(fsapi::FsError::NotFound)
    );
}

// ---------- ⑧ query 解析顺序 ----------

#[test]
fn spec_query_解析顺序() {
    let q = fsapi::parse_query("a=%26&b=2");
    assert_eq!(
        q,
        vec![
            ("a".to_string(), "&".to_string()),
            ("b".to_string(), "2".to_string())
        ]
    );
    assert_eq!(fsapi::query_get("a=%26&b=2", "a"), Some("&".to_string()));
    assert_eq!(fsapi::query_get("a=%26&b=2", "b"), Some("2".to_string()));
    // %3D 在值里不许被当分隔符：先切再解码
    assert_eq!(fsapi::query_get("x=a%3Db", "x"), Some("a=b".to_string()));
    // 值里的 & 必须编成 %26 才不成分隔符——未编码的 & 就是真分隔符（顺序错的反证）
    assert_eq!(fsapi::query_get("a=1&b=2", "a"), Some("1".to_string()));
    assert_eq!(fsapi::query_get("a=1&b=2", "b"), Some("2".to_string()));
    // 无 = / 空段 / 缺键 / 重复键取首个
    assert_eq!(fsapi::query_get("flag", "flag"), Some(String::new()));
    assert_eq!(fsapi::query_get("", "a"), None);
    assert_eq!(fsapi::query_get("&&a=1", "a"), Some("1".to_string()));
    assert_eq!(fsapi::query_get("a=1&a=2", "a"), Some("1".to_string()));
    assert_eq!(
        fsapi::query_get("dir=%2Fsrv%2Fna", "dir"),
        Some("/srv/na".to_string())
    );
    // 非法 % 序列原样保留
    assert_eq!(fsapi::pct_decode("100%"), "100%");
    assert_eq!(fsapi::pct_decode("%zz"), "%zz");
    assert_eq!(fsapi::pct_decode("%4"), "%4");
    assert_eq!(fsapi::query_get("a=%zz", "a"), Some("%zz".to_string()));
    // 中文百分号编码（curl 式客户端）
    assert_eq!(fsapi::pct_decode("%E4%B8%AD"), "中");
}

#[test]
fn spec_max_解析() {
    assert_eq!(fsapi::parse_max(""), fsapi::DEFAULT_MAX, "缺 max 回落缺省");
    assert_eq!(fsapi::parse_max("path=x"), fsapi::DEFAULT_MAX);
    assert_eq!(
        fsapi::parse_max("max=abc"),
        fsapi::DEFAULT_MAX,
        "非数字回落"
    );
    assert_eq!(fsapi::parse_max("max=0"), fsapi::DEFAULT_MAX, "0 回落");
    assert_eq!(fsapi::parse_max("max=-1"), fsapi::DEFAULT_MAX);
    assert_eq!(fsapi::parse_max("max=1024"), 1024);
    assert_eq!(
        fsapi::parse_max("max=99999999"),
        fsapi::MAX_MAX,
        "上限 1MB 钳"
    );
    assert_eq!(fsapi::DEFAULT_MAX, 65536);
    assert_eq!(fsapi::MAX_MAX, 1024 * 1024);
}

// ---------- ⑨ 排序稳定 ----------

#[test]
fn spec_排序稳定() {
    let td = TempDir::new().unwrap();
    for n in ["b.txt", "a.txt", "A.txt", "z", "0.txt", "中.txt"] {
        std::fs::write(td.path().join(n), "x").unwrap();
    }
    let _roots = roots_of(&td);
    // **三修症④改约（2026-09-27 用户拍板）**：全部是文件 → 组内**大小写不
    // 敏感**字母序；`A.txt`/`a.txt` 同键，回落原名序（'A'(0x41) < 'a'(0x61)）
    assert_eq!(
        names(&list(&td, "")),
        vec![
            "0.txt".to_string(),
            "A.txt".to_string(),
            "a.txt".to_string(),
            "b.txt".to_string(),
            "z".to_string(),
            "中.txt".to_string()
        ],
        "组内大小写不敏感字母序（CJK 按 Unicode 码位排尾）"
    );
    // 同树两次列 = 逐字节相同（稳定）
    let s1 = list(&td, "");
    let s2 = list(&td, "");
    assert_eq!(s1, s2, "同树两次列逐字节相同");
}

#[test]
fn spec_bar165三修_目录在前文件在后_组内大小写不敏感() {
    // 症④（2026-09-27 用户拍板）：目录在前、文件在后；组内大小写不敏感
    // 字母序（CJK 按 Unicode 码位）。**排序只在数据面这一处**——客户端
    // 行构照单排（filetree.rs / fs_fetch.rs 里 grep 无 sort 为证）
    let td = TempDir::new().unwrap();
    for d in ["Zeta", "alpha", "beta", "中文目录"] {
        std::fs::create_dir(td.path().join(d)).unwrap();
    }
    for f in ["Zeta.txt", "alpha.txt", "Beta.md", "中文件.txt"] {
        std::fs::write(td.path().join(f), "x").unwrap();
    }
    let got = names(&list(&td, ""));
    assert_eq!(
        got,
        vec![
            // 目录组：alpha < beta < Zeta（忽略大小写）< 中文目录（码位大）
            "alpha".to_string(),
            "beta".to_string(),
            "Zeta".to_string(),
            "中文目录".to_string(),
            // 文件组：alpha.txt < Beta.md < Zeta.txt < 中文件.txt
            "alpha.txt".to_string(),
            "Beta.md".to_string(),
            "Zeta.txt".to_string(),
            "中文件.txt".to_string(),
        ],
        "目录全在文件之前；组内大小写不敏感字母序"
    );
    // 结构断言（比名字表更硬）：前缀里不许出现任何 file
    let kinds = kinds(&list(&td, ""));
    let first_file = kinds.iter().position(|k| k == "file").unwrap();
    assert!(
        kinds[first_file..].iter().all(|k| k == "file"),
        "文件之后不许再出现目录：{kinds:?}"
    );
}

// ---------- roots 解析（纯核，不改进程 env） ----------

#[test]
fn spec_roots_解析() {
    // 显式覆盖：冒号分隔，空段丢弃
    assert_eq!(
        fsapi::roots_from(Some("/a:/b")),
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert_eq!(
        fsapi::roots_from(Some("/a::/b:")),
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert!(
        fsapi::roots_from(Some("")).is_empty(),
        "显式空串 = 零根 fail-closed"
    );
    // 缺省 = /root（2026-09-27 用户裁决）：**不看 HOME**——服务端 systemd
    // 无 HOME 时旧实现把根落到 `/`（真机 44 条系统目录），故这里显式钉死
    // 缺省值，并顺带钉「无 HOME 也无关」
    assert_eq!(
        fsapi::roots_from(None),
        vec![std::path::PathBuf::from("/root")],
        "缺省根恒 /root（不依赖 HOME）"
    );
    // 真机现读 env 的形状：至少一个根
    let r = fsapi::roots();
    assert!(!r.is_empty(), "本机缺省根非空: {r:?}");
}

// ---------- ⑧ BAR-170 分块读（read_range_json_in）----------

/// 逐块取读到 EOF，返回（重组文本, 各块 (offset, next_offset, truncated)）
fn read_all(td: &TempDir, rel: &str, max: usize) -> (String, Vec<(u64, u64, bool)>) {
    let roots = roots_of(td);
    let mut text = String::new();
    let mut ledger = Vec::new();
    let mut offset = 0u64;
    for _ in 0..64 {
        let v: Value = serde_json::from_str(
            &fsapi::read_range_json_in(&roots, rel, offset, max).expect("分块读成功"),
        )
        .expect("合法 JSON");
        assert_eq!(v["offset"].as_u64().unwrap(), offset, "回显 offset 错账");
        let next = v["next_offset"].as_u64().expect("next_offset 是数");
        let trunc = v["truncated"].as_bool().expect("truncated 是布尔");
        assert!(
            next > offset || !trunc,
            "truncated 时 next_offset 必须前进（防死循环）"
        );
        text.push_str(v["text"].as_str().expect("text 是字符串"));
        ledger.push((offset, next, trunc));
        offset = next;
        if !trunc {
            break;
        }
    }
    (text, ledger)
}

#[test]
fn spec_bar170_分块重组等于原文件() {
    let td = sample_tree();
    let body = "白日依山尽，黄河入海流。abc123\n".repeat(500); // 15KB 级
    std::fs::write(td.path().join("poem.txt"), &body).unwrap();
    let (text, ledger) = read_all(&td, "poem.txt", 4096);
    assert_eq!(text, body, "分块重组必须逐字节等于原文件");
    assert!(ledger.len() >= 3, "15KB / 4KB 至少 4 块: {ledger:?}");
    let last = ledger.last().unwrap();
    assert!(!last.2, "末块 truncated=false");
    assert_eq!(last.1, body.len() as u64, "末块 next_offset = size");
}

#[test]
fn spec_bar170_字界不变式_max不整除字宽() {
    let td = sample_tree();
    let body = "中".repeat(100); // 300 字节
    std::fs::write(td.path().join("cjk.txt"), &body).unwrap();
    for max in [1usize, 2, 4, 5, 7, 10] {
        let (text, ledger) = read_all(&td, "cjk.txt", max);
        assert_eq!(text, body, "max={max} 重组不等于原文件");
        assert!(!text.contains('\u{FFFD}'), "max={max} 劈出半个字");
        for (_, next, _) in &ledger {
            assert_eq!(*next % 3, 0, "max={max} next_offset={next} 不在字界上");
        }
    }
}

#[test]
fn spec_bar170_坏字节显形且账照样前进() {
    let td = sample_tree();
    // 坏字节在第二块里：lossy 出 U+FFFD，但 next_offset 按原始侧推进（不重读）
    let mut raw = b"good-prefix-".to_vec();
    raw.extend_from_slice(&[0xff, 0xfe]);
    raw.extend_from_slice(b"-and-a-long-tail-to-cross-chunk");
    std::fs::write(td.path().join("bad.bin.txt"), &raw).unwrap();
    let (text, ledger) = read_all(&td, "bad.bin.txt", 16);
    assert!(text.starts_with("good-prefix-"), "{text:?}");
    assert!(
        text.ends_with("-and-a-long-tail-to-cross-chunk"),
        "坏字节不许吞后半: {text:?}"
    );
    assert_eq!(
        text.matches('\u{FFFD}').count(),
        2,
        "两个坏字节各显形一次（不重读）: {text:?}"
    );
    assert_eq!(
        ledger.last().unwrap().1,
        raw.len() as u64,
        "末块 next_offset = size"
    );
}

#[test]
fn spec_bar170_越界与不存在同一条404() {
    let td = sample_tree();
    let roots = roots_of(&td);
    std::fs::write(td.path().join("s.txt"), "0123456789").unwrap();
    let e1 = fsapi::read_range_json_in(&roots, "s.txt", 10, 4).unwrap_err();
    let e2 = fsapi::read_range_json_in(&roots, "s.txt", 999, 4).unwrap_err();
    let e3 = fsapi::read_range_json_in(&roots, "ghost.txt", 0, 4).unwrap_err();
    assert_eq!(e1, e2, "offset=size 与 offset>size 同错");
    assert_eq!(
        format!("{e1:?}"),
        format!("{e3:?}"),
        "越界与不存在同一条错（不透露存在性）"
    );
    // offset 恰在最后一个字界内 = 合法
    let v: Value =
        serde_json::from_str(&fsapi::read_range_json_in(&roots, "s.txt", 9, 4).unwrap()).unwrap();
    assert_eq!(v["text"], "9");
    assert_eq!(v["truncated"], false);
    // 空文件 offset=0 合法出空块
    std::fs::write(td.path().join("empty.txt"), "").unwrap();
    let v2: Value =
        serde_json::from_str(&fsapi::read_range_json_in(&roots, "empty.txt", 0, 4).unwrap())
            .unwrap();
    assert_eq!(v2["text"], "");
    assert_eq!(v2["truncated"], false);
    assert_eq!(v2["next_offset"], 0);
}

#[test]
fn spec_bar170_nul块判二进制不带text() {
    let td = sample_tree();
    let roots = roots_of(&td);
    std::fs::write(td.path().join("b.bin"), b"abc\x00def").unwrap();
    let v: Value =
        serde_json::from_str(&fsapi::read_range_json_in(&roots, "b.bin", 0, 1024).unwrap())
            .unwrap();
    assert_eq!(v["binary"], true);
    assert!(v.get("text").is_none(), "二进制块不带 text");
    assert_eq!(v["size"], 7);
}

#[test]
fn spec_bar170_offset解析() {
    assert_eq!(fsapi::parse_offset(""), 0);
    assert_eq!(fsapi::parse_offset("offset=123"), 123);
    assert_eq!(
        fsapi::parse_offset("offset=abc"),
        0,
        "非数字回落 0（旧行为）"
    );
    assert_eq!(fsapi::parse_offset("max=9&offset=7"), 7);
    // has_offset：新旧契约分野 = 键在不在（redroid 判卷定罪——首块
    // offset=0 被「值判」喂旧契约，客户端出参缺键；变异：恒 false →
    // 新客户端全灭；恒 true → kfmv4 旧客户端出参多出两键虽兼容但契约漂移）
    assert!(!fsapi::has_offset(""), "空 query 不带键");
    assert!(!fsapi::has_offset("max=9"), "只带 max 不带 offset 键");
    assert!(fsapi::has_offset("offset=123"), "带键 = 新契约");
    assert!(fsapi::has_offset("offset=0"), "首块 offset=0 也是新契约");
    assert!(fsapi::has_offset("offset=abc"), "值非法但键在");
}

/// BAR-213 walk 夹具树：
/// ```text
/// root/
///   a.md "A" / b.MD "B"（大写后缀照收）/ c.txt（不收）
///   sub/d.md / sub/deep/e.md（递归收）
///   node_modules/x.md / .git/y.md / .hid/z.md（排除剪枝）
///   linkdir -> sub（软链目录，整枝剪）/ link.md -> a.md（软链文件，剪）
/// ```
fn walk_tree() -> TempDir {
    let td = TempDir::new().expect("临时目录");
    let r = td.path();
    std::fs::write(r.join("a.md"), "A").unwrap();
    std::fs::write(r.join("b.MD"), "BB").unwrap();
    std::fs::write(r.join("c.txt"), "c").unwrap();
    std::fs::create_dir_all(r.join("sub/deep")).unwrap();
    std::fs::write(r.join("sub/d.md"), "d").unwrap();
    std::fs::write(r.join("sub/deep/e.md"), "e").unwrap();
    std::fs::create_dir(r.join("node_modules")).unwrap();
    std::fs::write(r.join("node_modules/x.md"), "x").unwrap();
    std::fs::create_dir(r.join(".git")).unwrap();
    std::fs::write(r.join(".git/y.md"), "y").unwrap();
    std::fs::create_dir(r.join(".hid")).unwrap();
    std::fs::write(r.join(".hid/z.md"), "z").unwrap();
    std::os::unix::fs::symlink(r.join("sub"), r.join("linkdir")).unwrap();
    std::os::unix::fs::symlink(r.join("a.md"), r.join("link.md")).unwrap();
    td
}

fn walk(td: &TempDir, ext: &str) -> Value {
    let s = fsapi::walk_json_in(&roots_of(td), ext, None, 0).expect("walk 成功");
    serde_json::from_str(&s).expect("walk 出参是合法 JSON")
}

fn walk_paths(v: &Value) -> Vec<String> {
    v["entries"]
        .as_array()
        .expect("entries 是数组")
        .iter()
        .map(|e| e["path"].as_str().expect("path 是字符串").to_string())
        .collect()
}

#[test]
fn spec_bar213_walk_递归收md与排除剪枝() {
    let td = walk_tree();
    let v = walk(&td, ".md");
    assert_eq!(v["ok"], true);
    assert_eq!(v["ext"], ".md");
    // 递归收 a/b.MD/sub/d/sub/deep/e；c.txt 不收；node_modules/.git/.hid
    // 整枝剪；linkdir/link.md 软链不跟（linkdir 剪 = sub 内容不得经链
    // 第二次进账）。序 = 路径序（确定性对账键）
    assert_eq!(
        walk_paths(&v),
        vec!["a.md", "b.MD", "sub/d.md", "sub/deep/e.md"]
    );
    // size/mtime 两对账键在位（b.MD 写的是 "BB" = 2 字节）
    let e = &v["entries"][1];
    assert_eq!(e["size"], 2);
    assert!(e["mtime"].as_i64().expect("mtime 是整数") > 0);
}

#[test]
fn spec_bar213_walk_ext闸failclosed() {
    let td = walk_tree();
    // 形状非法全拒（不开任意子串匹配口）：空/无点/裸点/夹脏/超长
    for bad in ["", "md", ".", ".m d", ".md;", ".toolongext", "..md"] {
        assert!(
            fsapi::walk_json_in(&roots_of(&td), bad, None, 0).is_err(),
            "ext「{bad}」必须拒"
        );
    }
    // 合法形状大小写都行（后缀比对本身大小写不敏感）
    assert!(fsapi::walk_json_in(&roots_of(&td), ".MD", None, 0).is_ok());
    assert!(fsapi::walk_json_in(&roots_of(&td), ".txt", None, 0).is_ok());
}

#[test]
fn spec_bar213_walk_缺席根跳过不连坐() {
    let td = walk_tree();
    let missing = PathBuf::from("/nonexistent-bar213-root");
    let roots = vec![missing, td.path().to_path_buf()];
    let s = fsapi::walk_json_in(&roots, ".md", None, 0).expect("缺席根不连坐");
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(walk_paths(&v).len(), 4);
}

#[test]
fn spec_bar213_walk_多根并集() {
    let td = walk_tree();
    let td2 = TempDir::new().unwrap();
    std::fs::write(td2.path().join("only.md"), "o").unwrap();
    let roots = vec![td.path().to_path_buf(), td2.path().to_path_buf()];
    let s = fsapi::walk_json_in(&roots, ".md", None, 0).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(walk_paths(&v).len(), 5);
}

/// BAR-213 翻案 NA0145 甲案：keyset 分页——after 开区间续页、limit 截页、
/// 后面还有才带 next_after（= 本页末条 path）；limit=0 = 旧契约整单无
/// next_after。变异留档：①摘 raw.retain(after 过滤) → 第一咬续页含旧条红；
/// ② next_after 恒不给 → 第四咬「还有页必须带游标」红；③ truncate 摘了
/// → 第二咬页长超 limit 红。三咬均抓回（cp 备份复原复跑绿）。
#[test]
fn spec_bar213_walk_keyset分页_na0145() {
    let td = walk_tree();
    let roots = roots_of(&td);
    // 第一页 limit=2：a.md/b.MD + next_after=b.MD（后面还有）
    let s = fsapi::walk_json_in(&roots, ".md", None, 2).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(walk_paths(&v), vec!["a.md", "b.MD"], "首页截到 limit");
    assert_eq!(
        v["next_after"].as_str().expect("还有页必须带游标"),
        "b.MD",
        "游标 = 本页末条 path"
    );
    // 续页 after=b.MD（开区间：b.MD 本身不得再现）→ 收完，无 next_after
    let s = fsapi::walk_json_in(&roots, ".md", Some("b.MD"), 2).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(walk_paths(&v), vec!["sub/d.md", "sub/deep/e.md"]);
    assert!(
        v.get("next_after").is_none(),
        "末页不许带游标（客户端以此收口）"
    );
    // after 越过末条 = 空页无游标（翻页期间树缩了也不炸）
    let s = fsapi::walk_json_in(&roots, ".md", Some("zzz.md"), 2).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert!(walk_paths(&v).is_empty());
    assert!(v.get("next_after").is_none());
    // limit=0 = 旧契约整单（在野乙案客户端兜底），无 next_after
    let s = fsapi::walk_json_in(&roots, ".md", None, 0).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(walk_paths(&v).len(), 4);
    assert!(v.get("next_after").is_none(), "整单模式无游标键");
    // after 落中间（不是任何条目 path）也合法：keyset 比字典序不比存在性
    let s = fsapi::walk_json_in(&roots, ".md", Some("sub/c"), 2).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(walk_paths(&v), vec!["sub/d.md", "sub/deep/e.md"]);
}
