//! bar229_spec.rs — BAR-229 永久错误分类 + 坏 prefix 本地终端回落。
//!
//! 病灶（定罪链见 NA0159）：换机克隆把 files/usr 整体搬来但丢 owner
//! x 位（DAC）——nasup ensure spawn EACCES × 1034 次（60s 一拍空转）、
//! tunnel 伴生娃 6695 次无效重生、sshd 起不来、闸门/deploy 全瘫。
//! 修复三臂：①bootstrap 哨兵自愈闸（见 bootstrap_spec bar229 钉）；
//! ②spawn 错误分类——EACCES/ENOENT = 永久（环境态，重试不自愈），
//! 挂起重试循环（白露纪律 MAIN0113 §四-4 候选：重试必须带错误分类，
//! 永久错误不重试）；③shell_plan 判据从「文件在」升「可执行」——坏
//! prefix 时本地终端回落 /system/bin/sh，保住手机侧抢救通道。

#[test]
fn spec_bar229_spawn错误分类() {
    use std::io::ErrorKind;
    let f = kfm_na::na_server_sup::is_permanent_spawn_err;
    assert!(
        f(&std::io::Error::from(ErrorKind::PermissionDenied)),
        "EACCES=永久（克隆丢 x 位实证）"
    );
    assert!(
        f(&std::io::Error::from(ErrorKind::NotFound)),
        "ENOENT=永久（二进制缺失）"
    );
    // 瞬时档：网络态——重试是唯一出路，不许误挂起
    assert!(
        !f(&std::io::Error::from(ErrorKind::ConnectionRefused)),
        "拒连=瞬时"
    );
    assert!(!f(&std::io::Error::from(ErrorKind::TimedOut)), "超时=瞬时");
    assert!(
        !f(&std::io::Error::from(ErrorKind::WouldBlock)),
        "WouldBlock=瞬时"
    );
    assert!(
        !f(&std::io::Error::from(ErrorKind::BrokenPipe)),
        "BrokenPipe=瞬时"
    );
}

#[test]
fn spec_bar229_shell_plan_坏prefix回落系统sh() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr");
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    let bash = prefix.join("bin/bash");
    std::fs::write(&bash, b"#!/fake").unwrap();
    // 健康：可执行 → bash 计划（带 prefix env）
    std::fs::set_permissions(&bash, std::fs::Permissions::from_mode(0o700)).unwrap();
    let plan = kfm_na::local_pty::shell_plan(&prefix);
    assert!(plan.shell.contains("bash"), "健康走 bash：{}", plan.shell);
    assert!(!plan.env_extra.is_empty(), "健康计划带 prefix env");
    // 克隆丢 x 位形态：0600 → 回落系统 sh（变异：判据退回 is_file → 红）
    std::fs::set_permissions(&bash, std::fs::Permissions::from_mode(0o600)).unwrap();
    let plan = kfm_na::local_pty::shell_plan(&prefix);
    assert_eq!(
        plan.shell,
        kfm_na::local_pty::default_shell(),
        "坏 prefix 回落系统 sh（保住手机侧抢救通道）"
    );
    assert!(plan.env_extra.is_empty(), "回落计划不带 prefix env");
}
