//! bootstrap_spec.rs — L3 bootstrap 解压核心考题（考题先行：先红后绿）
//!
//! 对应设计页 /root/kfmv4/experiments/dsh-na/na/l3-bootstrap.md §5。
//! 语义对照 termux-app TermuxInstaller：staging 解包 → SYMLINKS.txt 补链 →
//! 原子 rename → 幂等跳过。核心层纯文件逻辑，host 可判卷。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// 造一个迷你 bootstrap zip：两个文件 + 可选 SYMLINKS.txt
fn fixture_zip(with_symlinks: bool) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    w.start_file("bin/dash", opt).unwrap();
    std::io::Write::write_all(&mut w, b"fake-dash-binary").unwrap();
    w.start_file("lib/apt/methods/http", opt).unwrap();
    std::io::Write::write_all(&mut w, b"fake-apt-method").unwrap();
    w.start_file("etc/motd", opt).unwrap();
    std::io::Write::write_all(&mut w, b"welcome").unwrap();
    if with_symlinks {
        // 真 bootstrap 的 SYMLINKS.txt 格式：target←linkpath(U+2190 分隔)
        w.start_file("SYMLINKS.txt", opt).unwrap();
        std::io::Write::write_all(&mut w, "dash←bin/sh\n".as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn mode_of(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn spec_l3_空prefix_完整安装() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    let status = kfm_na::bootstrap::ensure_prefix(&prefix, &fixture_zip(true)).unwrap();
    assert!(matches!(
        status,
        kfm_na::bootstrap::InstallStatus::Installed
    ));
    // 文件落盘
    assert_eq!(
        fs::read(prefix.join("bin/dash")).unwrap(),
        b"fake-dash-binary"
    );
    assert_eq!(fs::read(prefix.join("etc/motd")).unwrap(), b"welcome");
    // SYMLINKS.txt 不留在盘上（它是指令,不是内容）
    assert!(!prefix.join("SYMLINKS.txt").exists());
    // 符号链接补建：bin/sh → dash
    let link = fs::read_link(prefix.join("bin/sh")).unwrap();
    assert_eq!(link, Path::new("dash"));
    // staging 已原子 rename,不残留
    assert!(!tmp.path().join("files/usr-staging").exists());
}

#[test]
fn spec_l3_非空prefix_幂等跳过() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    fs::create_dir_all(&prefix).unwrap();
    fs::write(prefix.join("marker"), b"old").unwrap();
    // 垃圾字节也不该被解析——非空 prefix 直接跳过
    let status = kfm_na::bootstrap::ensure_prefix(&prefix, b"not-a-zip").unwrap();
    assert!(matches!(
        status,
        kfm_na::bootstrap::InstallStatus::AlreadyPresent
    ));
    assert_eq!(fs::read(prefix.join("marker")).unwrap(), b"old");
}

#[test]
fn spec_l3_chmod规则() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    kfm_na::bootstrap::ensure_prefix(&prefix, &fixture_zip(true)).unwrap();
    // bin/ 与 lib/apt/methods/ 下 0700(可执行)
    assert_eq!(mode_of(&prefix.join("bin/dash")), 0o700);
    assert_eq!(mode_of(&prefix.join("lib/apt/methods/http")), 0o700);
    // 其余 0644
    assert_eq!(mode_of(&prefix.join("etc/motd")), 0o644);
}

#[test]
fn spec_l3_无symlinks_报错不留残局() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    let err = kfm_na::bootstrap::ensure_prefix(&prefix, &fixture_zip(false)).unwrap_err();
    assert!(err.contains("SYMLINKS"), "报错要点名 SYMLINKS: {err}");
    // 失败不落成半截 prefix(staging 清走,可重试)
    assert!(!prefix.exists());
    assert!(!tmp.path().join("files/usr-staging").exists());
}

#[test]
fn spec_l3_second_stage命令组装() {
    let prefix = Path::new("/data/data/dev.kfm.na/files/usr");
    let cmd = kfm_na::bootstrap::second_stage_command(prefix);
    assert_eq!(cmd.get_program(), prefix.join("bin/bash"));
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(
        args,
        [prefix
            .join("etc/termux/termux-bootstrap/second-stage/termux-bootstrap-second-stage.sh")
            .as_os_str()]
    );
    let envs: std::collections::HashMap<_, _> = cmd.get_envs().collect();
    let env_str = |k: &str| {
        envs.get(std::ffi::OsStr::new(k))
            .and_then(|v| v.and_then(|s| s.to_str()))
            .unwrap_or("")
    };
    assert_eq!(env_str("PATH"), format!("{}/bin", prefix.display()));
    assert_eq!(
        env_str("LD_LIBRARY_PATH"),
        format!("{}/lib", prefix.display())
    );
}
