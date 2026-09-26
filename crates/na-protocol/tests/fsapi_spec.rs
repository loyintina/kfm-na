//! crates/na-protocol/tests/fsapi_spec.rs — A 档考题：文件树数据面纯函数核
//!
//! 答案区：crates/na-protocol/src/fsapi.rs。本文件是考题，生成器不许改。
//!
//! 造真目录树在 tempdir 里，用 `*_in(roots, …)` 注入根（考题不许改进程 env——
//! 并行考题里 set_var 是竞态源）；`roots_from`/`parse_*`/`pct_decode` 等无 IO
//! 的面直接打公开函数。

use std::path::{Path, PathBuf};

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

// ---------- ① 只列直接子层 ----------

#[test]
fn spec_list_只列直接子层() {
    let td = sample_tree();
    let v = list(&td, "");
    assert_eq!(v["ok"], true);
    assert_eq!(v["dir"], "");
    assert_eq!(
        names(&v),
        vec!["A.txt".to_string(), "a.txt".to_string(), "sub".to_string()],
        "子目录里的东西不许冒头"
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
        fsapi::query_get("dir=%2Froot%2F00-Loyintina", "dir"),
        Some("/root/00-Loyintina".to_string())
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
    let roots = roots_of(&td);
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
        "字节序（'0'<'A'<'a'；CJK 首字节 0xE4 最大）"
    );
    let s1 = fsapi::list_json_in(&roots, "").unwrap();
    let s2 = fsapi::list_json_in(&roots, "").unwrap();
    assert_eq!(s1, s2, "同树两次列 = 逐字节相同（稳定）");
}

// ---------- roots 解析（纯核，不改进程 env） ----------

#[test]
fn spec_roots_解析() {
    let home = Path::new("/tmp/kfm-na-spec-home");
    // 显式覆盖：冒号分隔，空段丢弃
    assert_eq!(
        fsapi::roots_from(Some("/a:/b"), home),
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert_eq!(
        fsapi::roots_from(Some("/a::/b:"), home),
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert!(
        fsapi::roots_from(Some(""), home).is_empty(),
        "显式空串 = 零根 fail-closed"
    );
    // 缺省：库本体存在（服务器 /root/00-Loyintina）→ 用它
    let td = TempDir::new().unwrap();
    let lib = td.path().join("00-Loyintina");
    std::fs::create_dir(&lib).unwrap();
    assert_eq!(fsapi::roots_from(None, td.path()), vec![lib]);
    // 库不在 → 退回 HOME 本身
    let td2 = TempDir::new().unwrap();
    assert_eq!(
        fsapi::roots_from(None, td2.path()),
        vec![td2.path().to_path_buf()]
    );
    // 真机现读 env 的形状：至少一个根
    let r = fsapi::roots();
    assert!(!r.is_empty(), "本机缺省根非空: {r:?}");
}
