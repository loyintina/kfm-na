//! bootstrap_spec.rs — L3 bootstrap 解压核心考题（考题先行：先红后绿）
//!
//! 对应设计页（na 方向档案）l3-bootstrap.md §5。
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
// BAR-229 追平（原 BAR-074 场景「非空=跳过」升「可用=跳过」）：非空
// 但哨兵不可执行（marker-only 即环境死形态——克隆丢 x 位实证）→ 不再
// 幂等跳过，走自愈重装；垃圾 zip 解不开 = Err（不静默当已装），失败
// 路径不破坏既有目录（staging 清、prefix 原样，可重试）。健康 prefix
// 的幂等跳过在 spec_bar229 末幕断言（重装后再 ensure = AlreadyPresent）。
fn spec_l3_环境死prefix_不静默跳过_失败不破坏() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    fs::create_dir_all(&prefix).unwrap();
    fs::write(prefix.join("marker"), b"old").unwrap();
    // 垃圾 zip + 环境死 prefix → 拒（Err），不静默跳过
    let r = kfm_na::bootstrap::ensure_prefix(&prefix, b"not-a-zip");
    assert!(
        r.is_err(),
        "环境死样本 + 垃圾 zip 必须 Err（变异：退回纯存在性闸 → 此处得 AlreadyPresent 即红）"
    );
    // 失败不破坏：marker 原样、无 staging 残留（可重试）
    assert_eq!(fs::read(prefix.join("marker")).unwrap(), b"old");
    assert!(
        !tmp.path().join("files/usr-staging").exists(),
        "staging 已清"
    );
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

// BAR-074 钉：幂等闸谓词——不存在/空目录/非空三态。壳靠它在读 32MB
// 资产之前就跳过（旧序读完才问 ensure_prefix，启动关键路径每启裸读
// 32MB，IO 挤兑期 boot 段 3s+，PIN-boot 挂卷族）。
// 变异抽检：谓词改成恒 true/恒 false 各咬一端。
// BAR-229 追平第四态：非空但 bin/ 无任何 x 位（克隆丢 x 位的环境死
// 样本）= false——「存在」升「可用」。
#[test]
fn spec_bar074_幂等闸谓词_三态() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    assert!(!kfm_na::bootstrap::prefix_ready(&prefix), "不存在=false");
    fs::create_dir_all(&prefix).unwrap();
    assert!(!kfm_na::bootstrap::prefix_ready(&prefix), "空目录=false");
    fs::write(prefix.join("marker"), b"x").unwrap();
    assert!(
        !kfm_na::bootstrap::prefix_ready(&prefix),
        "非空但 bin/ 无 x 位=false（BAR-229 环境死样本）"
    );
    fs::create_dir_all(prefix.join("bin")).unwrap();
    fs::write(prefix.join("bin/tool"), b"#!/x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(prefix.join("bin/tool"), fs::Permissions::from_mode(0o700)).unwrap();
        // BAR-230 链接判据：健康形态还需 bin/sh 链接在且目标可解析
        std::os::unix::fs::symlink("tool", prefix.join("bin/sh")).unwrap();
    }
    assert!(
        kfm_na::bootstrap::prefix_ready(&prefix),
        "bin/ 有 x 位且 bin/sh 链接健康=true"
    );
}

// BAR-229 钉：换机克隆丢 x 位——prefix 非空但 bin/ 全无 x 位 = 环境死，
// 幂等闸拒（变异：哨兵摘除回纯存在性判据 → 「丢x位=false」断言红）；
// ensure_prefix 自愈 wipe 重装（remove_dir_all 摘非空旧 prefix），装完
// x 位恢复（install_to_staging 显式 chmod，免疫 umask）。
// 实证链：Neo 11 克隆搬运 + nasup spawn EACCES × 1034 + ssh 不在
// bootstrap zip（存在即克隆来）+ 10-03 无 bootstrap 安装日志。
#[test]
fn spec_bar229_克隆丢x位_自愈重装() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("files/usr");
    let st = kfm_na::bootstrap::ensure_prefix(&prefix, &fixture_zip(true)).unwrap();
    assert!(matches!(st, kfm_na::bootstrap::InstallStatus::Installed));
    assert!(kfm_na::bootstrap::prefix_ready(&prefix), "健康装好后=true");
    // 克隆搬运模拟：bin/ 下全部摘 x 位（DAC 丢失形态）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for e in fs::read_dir(prefix.join("bin"))
            .unwrap()
            .filter_map(|e| e.ok())
        {
            let p = e.path();
            if fs::metadata(&p).map(|m| m.is_file()).unwrap_or(false) {
                let m = fs::metadata(&p).unwrap().permissions().mode();
                fs::set_permissions(&p, fs::Permissions::from_mode(m & 0o666)).unwrap();
            }
        }
    }
    assert!(
        !kfm_na::bootstrap::prefix_ready(&prefix),
        "丢 x 位=false（哨兵咬）"
    );
    // 自愈：wipe 重装而非 AlreadyPresent；重装产物恢复 x 位
    let st = kfm_na::bootstrap::ensure_prefix(&prefix, &fixture_zip(true)).unwrap();
    assert!(
        matches!(st, kfm_na::bootstrap::InstallStatus::Installed),
        "环境死样本应走重装（变异：摘哨兵 → 此处得 AlreadyPresent 即红）"
    );
    assert_eq!(mode_of(&prefix.join("bin/dash")), 0o700, "重装后 x 位恢复");
    assert!(kfm_na::bootstrap::prefix_ready(&prefix), "自愈后=true");
    // 末幕：健康 prefix 的幂等跳过（BAR-074 原契约在「可用」语义下续存）
    let st = kfm_na::bootstrap::ensure_prefix(&prefix, &fixture_zip(true)).unwrap();
    assert!(matches!(
        st,
        kfm_na::bootstrap::InstallStatus::AlreadyPresent
    ));
}
