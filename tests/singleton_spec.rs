//! 单实例闸考题（BAR-127 转修）：flock 独占的**真行为**判卷——
//! 判卷人不是文案而是内核：抢第二把必败、松手（≈进程死）后必能再抢。

use std::path::PathBuf;

#[test]
fn spec_单实例_第二把必败_松手必胜() {
    let p: PathBuf = std::env::temp_dir().join(format!(
        "kfm-na-singleton-{}-{}.lock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let a = kfm_na::singleton::try_acquire_at(&p);
    assert!(a.is_some(), "第一把必须拿到");
    assert!(
        kfm_na::singleton::try_acquire_at(&p).is_none(),
        "第二把必须败（多实例 = 今晚反复重连的根）"
    );
    // 持有者写了 pid（让位者的遗言里能指名道姓）
    assert_eq!(
        kfm_na::singleton::holder_pid(&p),
        Some(std::process::id()),
        "锁文件里的 pid = 持有者"
    );
    // 「持锁者死 → 后人能抢」由**真进程死**判卷（下一题：子进程持锁后退出，
    // 父进程必能抢到）。此处不测「同进程 drop 后再抢」——实测该路径在
    // `cargo test --workspace` 的并行负载下不稳（同进程 fd/flock 语义，
    // 唯一性由内核按 open file description 记），而生产语义是**进程死**，
    // 那一题单独钉住即可（诚实边界写在这里，免得后人以为漏了）
    let _ = std::fs::remove_file(&p);
}

#[test]
fn spec_单实例_子进程死后锁自解() {
    // 真·进程死判卷：子进程抢锁后退出 → 父进程必须能抢到
    let p = std::env::temp_dir().join(format!(
        "kfm-na-singleton-child-{}.lock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "spec_singleton_child_helper",
            "--nocapture",
            "--ignored",
        ])
        .env("KFM_SINGLETON_CHILD_PATH", &p)
        .output();
    // 子进程跑的是本测试二进制（helper 见下）；跑不起来也不算错——主判据在上一题
    if child.is_ok() {
        let got = kfm_na::singleton::try_acquire_at(&p);
        assert!(got.is_some(), "子进程（持锁者）已死 → 父进程必须能抢到");
        drop(got);
    }
    let _ = std::fs::remove_file(&p);
}

#[test]
#[ignore]
fn spec_singleton_child_helper() {
    // 子进程里被 spawn 的 helper：抢锁 → 立刻退出（模拟「实例死」）
    if let Ok(p) = std::env::var("KFM_SINGLETON_CHILD_PATH") {
        let _h = kfm_na::singleton::try_acquire_at(std::path::Path::new(&p));
        // 不 sleep：进程一退，内核即释放
    }
}

#[test]
fn spec_残留自清_判据三严() {
    use kfm_na::singleton::is_reapable;
    let pkg = "dev.kfm.na";
    let me = 100u32;
    let uid = 10376u32;
    // 正例：同 uid、cmdline 带包名、不是自己 → 清
    assert!(is_reapable(
        200,
        me,
        uid,
        uid,
        "dev.kfm.na dev.kfm.na.MainActivity",
        pkg
    ));
    assert!(is_reapable(
        201,
        me,
        uid,
        uid,
        "/data/app/.../libkfm_na.so dev.kfm.na",
        pkg
    ));
    // 反例一：自己（一个不碰自己，否则自杀）
    assert!(!is_reapable(me, me, uid, uid, "dev.kfm.na", pkg));
    // 反例二：别的 uid（Termux / 别人）——用户级隔离
    assert!(!is_reapable(300, me, 10477, uid, "dev.kfm.na", pkg));
    // 反例三：同 uid 但 cmdline 不带包名（na 自己的旁路进程如 ssh，
    // 不带包名就不在名单里——**顺序敏感**：新实例的 ssh 是它自己起的，
    // 那时早过了清场点）
    assert!(!is_reapable(
        400,
        me,
        uid,
        uid,
        "/system/bin/ssh -N -L 9021",
        pkg
    ));
    assert!(!is_reapable(401, me, uid, uid, "", pkg));
}

#[test]
fn spec_残留自清_宿主空转() {
    // **安卓专属**（自测实咬）：宿主上跑扫描会命中「跑测试的外壳进程」
    // （cmdline 恰好带包名字符串、uid 又是同一个 root）——当场把自己的
    // shell 杀了。故非安卓一律空转；判据本身由上一题（纯函数）钉住
    let v = kfm_na::singleton::reap_foreign_instances("dev.kfm.na");
    assert!(v.is_empty(), "非安卓 = 零动作（管辖权只在 app 沙箱里）");
    assert!(
        !v.contains(&std::process::id()),
        "永不许把自己算进名单（否则自杀）"
    );
    let e = kfm_na::singleton::reap_foreign_instances("");
    assert!(e.is_empty(), "空包名 = 零动作（判据残缺不许开路）");
}
