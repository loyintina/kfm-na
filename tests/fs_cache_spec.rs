//! tests/fs_cache_spec.rs — A 档考题：文件树本地缓存纯核（BAR-187）
//!
//! 答案区：src/fs_fetch.rs 上半（BAR-187 缓存段）。本文件是考题，生成器不许改。
//! 咬点：缓存键编码（中文/斜杠/空串落平文件名）、读写回环、list 失败
//! 决策（有缓存留场不走 fail）、正文缓存出参闸（同源/binary/坏件）。

use kfm_na::fs_fetch;

#[test]
fn spec_bar187_缓存键_pct编码落平文件名() {
    assert_eq!(
        fs_fetch::cache_rel("list", ""),
        "list/ROOT.json",
        "根目录键不落隐形 .json"
    );
    assert_eq!(
        fs_fetch::cache_rel("list", "docs"),
        "list/docs.json",
        "ASCII 原样"
    );
    let cjk = fs_fetch::cache_rel("list", "文档/子目");
    assert!(
        cjk.starts_with("list/%") && cjk.ends_with(".json"),
        "中文/斜杠必编码：{cjk}"
    );
    assert!(
        !cjk["list/".len()..].contains('/'),
        "键内斜杠不许漏进文件名：{cjk}"
    );
    let tricky = fs_fetch::cache_rel("read", "a&b=c d.md");
    assert!(
        !tricky.contains('&') && !tricky.contains('=') && !tricky.contains(' '),
        "查询危险字符必编码：{tricky}"
    );
}

#[test]
fn spec_bar187_缓存读写回环() {
    let root = std::env::temp_dir().join(format!("bar187-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let rel = fs_fetch::cache_rel("list", "文档/子目");
    // 不存在 = None（缓存是加强不是命脉）
    assert_eq!(fs_fetch::read_cache(&root, &rel), None);
    // 写透 → 读回逐字节同（父目录自动建）
    fs_fetch::write_cache(&root, &rel, "{\"ok\":true}");
    assert_eq!(
        fs_fetch::read_cache(&root, &rel).as_deref(),
        Some("{\"ok\":true}")
    );
    // 空文件 = None（写半截的残骸不当缓存）
    fs_fetch::write_cache(&root, &rel, "");
    assert_eq!(fs_fetch::read_cache(&root, &rel), None, "空文件不当缓存");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn spec_bar187_list失败决策_有缓存留场() {
    assert!(
        !fs_fetch::list_fail_goes_old(true),
        "有缓存不走 fail（缓存树不许被拍成错误行）"
    );
    assert!(fs_fetch::list_fail_goes_old(false), "无缓存照旧错误路");
}

#[test]
fn spec_bar187_正文缓存出参闸() {
    let good = fs_fetch::read_cache_json("a/b.md", "正文");
    assert_eq!(
        fs_fetch::cached_read_text(&good, "a/b.md").as_deref(),
        Some("正文"),
        "落盘串一口径读回"
    );
    assert_eq!(
        fs_fetch::cached_read_text(&good, "a/c.md"),
        None,
        "path 不同源拒喂"
    );
    assert_eq!(
        fs_fetch::cached_read_text("{\"ok\":false,\"error\":\"x\"}", "a/b.md"),
        None,
        "ok:false 拒喂"
    );
    assert_eq!(
        fs_fetch::cached_read_text(
            "{\"ok\":true,\"path\":\"a/b.png\",\"binary\":true,\"size\":9}",
            "a/b.png"
        ),
        None,
        "binary 拒喂"
    );
    assert_eq!(
        fs_fetch::cached_read_text("not json", "a"),
        None,
        "坏 JSON 拒喂"
    );
    assert_eq!(
        fs_fetch::cached_read_text("{\"ok\":true,\"path\":\"a\"}", "a"),
        None,
        "缺 text 拒喂"
    );
    assert_eq!(
        fs_fetch::cached_read_text(&fs_fetch::read_cache_json("a", ""), "a"),
        None,
        "空正文不落缓存（读回即拒）"
    );
}
