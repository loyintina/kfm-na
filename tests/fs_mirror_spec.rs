//! fs_mirror_spec.rs — BAR-213 md 全量镜像核钉（A 档：考题先行 + 变异抽检）
//!
//! 变异留档（cp 备份复原，禁 git checkout）：
//! ① reconcile 摘 `|| !exists(&r.path)` → 三判钉「本地缺 = 拉」红；
//! ② synth_list_body 摘 `to_lowercase()` 那级排序 → 同律钉大小写序红；
//! ③ walk_entries_of 摘 ok:false 拒 → 解析钉「ok:false = None」红；
//! ④ manifest::parse 摘 dedup → 回环钉去重红。四咬均抓回。

use kfm_na::fs_mirror::{
    Manifest, ManifestEntry, mirror_file_rel, reconcile, synth_list_body, walk_entries_of,
};

fn ent(path: &str, size: u64, mtime: i64) -> ManifestEntry {
    ManifestEntry {
        path: path.to_string(),
        size,
        mtime,
    }
}

fn manifest(entries: Vec<ManifestEntry>) -> Manifest {
    let mut m = Manifest { entries };
    m.entries.sort_by(|a, b| a.path.cmp(&b.path));
    m
}

/// manifest 台账：落盘 → 解析回环恒等；坏件 = 空台账（不炸不障）
#[test]
fn spec_bar213_manifest_落盘解析回环() {
    let m = manifest(vec![ent("a/b.md", 10, 100), ent("c.md", 20, 200)]);
    let back = Manifest::parse(&m.to_json());
    assert_eq!(back.entries, m.entries, "落盘解析回环必须恒等");
    // 坏 JSON / 缺 entries / 条目缺键 = 空台账或跳半条，一律不 panic
    assert!(Manifest::parse("不是 json").entries.is_empty());
    assert!(Manifest::parse("{}").entries.is_empty());
    let half = Manifest::parse(r#"{"version":1,"entries":[{"path":"x.md","size":1}]}"#);
    assert!(half.entries.is_empty(), "缺 mtime 的半条跳过");
    // 重复 path 去重 + 按 path 序（确定性落盘）
    let dup = Manifest::parse(
        r#"{"version":1,"entries":[{"path":"b.md","size":1,"mtime":1},{"path":"a.md","size":2,"mtime":2},{"path":"b.md","size":1,"mtime":1}]}"#,
    );
    assert_eq!(dup.entries.len(), 2, "重复 path 去重");
    assert_eq!(dup.entries[0].path, "a.md", "按 path 序");
}

/// reconcile 三判：新 → 拉；变（size 或 mtime 任一不同）→ 拉；本地缺 → 拉；
/// 远端消失 → 删；双同且本地在 = 不动
#[test]
fn spec_bar213_reconcile_三判() {
    let remote = vec![
        ent("same.md", 10, 100),    // 双同且本地在 = 不动
        ent("new.md", 1, 1),        // 台账无 = 新 → 拉
        ent("size.md", 11, 100),    // size 变 → 拉
        ent("mtime.md", 10, 101),   // mtime 变 → 拉
        ent("missing.md", 10, 100), // 台账双同但本地缺 → 拉
    ];
    let man = manifest(vec![
        ent("same.md", 10, 100),
        ent("size.md", 10, 100),
        ent("mtime.md", 10, 100),
        ent("missing.md", 10, 100),
        ent("gone.md", 5, 50), // 远端无 = 消失 → 删
    ]);
    let plan = reconcile(&remote, &man, |p| p != "missing.md");
    let fetch: Vec<&str> = plan.fetch.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        fetch,
        vec!["new.md", "size.md", "mtime.md", "missing.md"],
        "新/变/本地缺三判全在拉取表"
    );
    assert_eq!(plan.delete, vec!["gone.md"], "远端消失进删除表");
    // 全同且全在 = 空计划（断点续传的空转形态）
    let plan2 = reconcile(&remote[..1], &man, |_| true);
    assert!(plan2.fetch.is_empty() && !plan2.delete.is_empty());
}

/// 镜像路径映射闸：正常 path 落 fs/<path>；绝对路径/.. 逃逸一律 None
/// （与 fsapi::safe_rel 同一闸——镜像映射不许出 mirror_root）
#[test]
fn spec_bar213_mirror_file_rel_安全闸() {
    assert_eq!(mirror_file_rel("a/b.md").as_deref(), Some("fs/a/b.md"));
    assert_eq!(mirror_file_rel("x.md").as_deref(), Some("fs/x.md"));
    assert!(mirror_file_rel("/etc/passwd").is_none(), "绝对路径拒");
    assert!(mirror_file_rel("../out.md").is_none(), ".. 逃逸拒");
    assert!(mirror_file_rel("a/../../out.md").is_none());
    assert!(mirror_file_rel("").is_none(), "空串不是文件");
}

/// walk 端点出参解析：好件全收；ok:false/坏 JSON = None（整趟放弃）；
/// 条目缺键 = 跳半条
#[test]
fn spec_bar213_walk_entries_of_解析() {
    let good = r#"{"ok":true,"ext":".md","entries":[{"path":"a.md","size":3,"mtime":7},{"path":"d/b.md","size":4,"mtime":8}]}"#;
    let got = walk_entries_of(good).expect("好件必须解析得出");
    assert_eq!(got, vec![ent("a.md", 3, 7), ent("d/b.md", 4, 8)]);
    // ok:false 但带 entries——只有 ok 拒那一道能拦（专咬摘 ok 判的变异）
    assert!(
        walk_entries_of(r#"{"ok":false,"entries":[{"path":"a.md","size":3,"mtime":7}]}"#).is_none()
    );
    assert!(walk_entries_of("坏掉了").is_none());
    assert!(
        walk_entries_of(r#"{"ok":true}"#).is_none(),
        "缺 entries = None"
    );
    let half = r#"{"ok":true,"entries":[{"path":"a.md","size":3,"mtime":7},{"path":"bad.md"}]}"#;
    assert_eq!(walk_entries_of(half).unwrap().len(), 1, "半条跳过");
}

/// 合成 list：与 fsapi 同形同律（目录在前 + 大小写不敏感 + 原名 tie-break），
/// 消费面 filetree::entries_of 必须照单收下；查无此目录/空台账 = None
#[test]
fn spec_bar213_synth_list_同形同律() {
    let man = manifest(vec![
        ent("docs/a.md", 10, 100),
        ent("docs/Deep/x.md", 20, 300),
        ent("docs/b.md", 30, 200),
        ent("README.md", 5, 50),
        ent("zoo/Zebra.md", 1, 10),
        ent("zoo/banana.md", 2, 20),
    ]);
    // 根层：docs/zoo 两目录在前，README.md 殿后；目录 mtime = 子孙最大
    let body = synth_list_body(&man, "").expect("根层必须合成得出");
    let entries = kfm_na::ui::filetree::entries_of(&body).expect("消费面必须收");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["docs", "zoo", "README.md"], "目录在前文件在后");
    assert_eq!(entries[0].mtime, 300, "docs 的 mtime = 子孙最大");
    // 子层：目录在前 + 大小写不敏感（Deep < a? 否——d<eep 小写 deep，
    // 目录组只有 Deep；文件组 a.md < b.md）
    let body = synth_list_body(&man, "docs").expect("docs 层必须合成得出");
    let entries = kfm_na::ui::filetree::entries_of(&body).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Deep", "a.md", "b.md"]);
    assert_eq!(entries[1].size, 10, "文件条目带真 size");
    // 大小写不敏感序：banana < Zebra（ASCII 序 Z(90) < b(98) 会抢跑——
    // 这对夹具专咬 to_lowercase 那级被摘的变异）
    let body = synth_list_body(&man, "zoo").unwrap();
    let entries = kfm_na::ui::filetree::entries_of(&body).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["banana.md", "Zebra.md"], "大小写不敏感字母序");
    // 查无此目录 / 空台账 = None（调用方落旧错误路）
    assert!(synth_list_body(&man, "nowhere").is_none());
    assert!(synth_list_body(&Manifest::default(), "").is_none());
    // 出参与端点同形（ok/dir 键在）
    let body = synth_list_body(&man, "docs").unwrap();
    assert!(body.contains(r#""ok":true"#) && body.contains(r#""dir":"docs""#));
}

/// 接线源码守卫（B 档胶水的机械钉——漏一处 = 断网翻开任意 md 的判卷链
/// 断在某个环节，实拍前钉死）：回退链两阶都在、声明头用对、同步器三处
/// 触发、模块接线
#[test]
fn spec_bar213_接线源码守卫() {
    let fetch = include_str!("../src/fs_fetch.rs");
    assert!(
        fetch.contains("serve_list_cache(&dir, false) || serve_mirror_list(&dir, false)"),
        "list 回退链：v1 缓存 → 镜像（request_list 臂）"
    );
    assert!(
        fetch.contains("serve_list_cache(&dir, true) || serve_mirror_list(&dir, true)"),
        "list 回退链：级联 quiet 臂同律"
    );
    assert!(
        fetch.contains("serve_read_cache(&path) || serve_mirror_read(&path)"),
        "read 回退链：v1 缓存 → 镜像（判卷点「断网翻开任意 md」靠这阶）"
    );
    assert!(
        fetch.contains("crate::fs_mirror::MIRROR_NOTICE"),
        "镜像副本必须带镜像声明头（与 CACHE_NOTICE 措辞分家）"
    );
    assert!(
        fetch.contains("\"/api/fs/walk?ext=.md\""),
        "同步器必须吃 walk 递归清单端点"
    );
    assert!(
        fetch.contains("MIRROR_SYNCING.swap(true"),
        "同步器必须在飞闸（重入不叠线程）"
    );
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("crate::fs_fetch::set_mirror_root(dir.join(\"mirror\"))"),
        "壳 configure 必须喂镜像根 <私有目录>/mirror"
    );
    assert_eq!(
        app.matches("crate::fs_fetch::request_mirror_sync()")
            .count(),
        3,
        "同步触发三处：隧道口喂定 / 开树召唤沿 / 回前台"
    );
    let lib = include_str!("../src/lib.rs");
    assert!(lib.contains("pub mod fs_mirror;"), "镜像核必须接线进库");
}
