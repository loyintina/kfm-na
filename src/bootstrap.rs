//! bootstrap.rs — L3:首启安装 Linux 环境(bootstrap zip → $PREFIX)
//!
//! 语义对照 termux-app 的 TermuxInstaller(我们只借语义,不抄代码——
//! termux-app 是 GPL-3.0,本模块按设计页 l3-bootstrap.md §5 重写):
//!   1. $PREFIX 非空 → 跳过(幂等,zip 字节都不解析)
//!   2. 解到 staging(usr-staging),完成后原子 rename 成 $PREFIX
//!   3. zip 不含符号链接——SYMLINKS.txt 逐行 `target←linkpath`(U+2190),
//!      全部文件落盘后统一补建
//!   4. chmod 0700:bin/、libexec、lib/apt/apt-helper、lib/apt/methods
//!   5. second-stage:遍历 dpkg postinst 逐个 configure(命令组装在这,
//!      执行归壳——真机上经 $PREFIX/bin/bash 跑)
//!
//! 分层:本模块是核心层纯文件逻辑(host 可判卷);Android 壳只负责
//! 读 assets 字节、给 prefix 路径、执行 second_stage_command。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Debug)]
pub enum InstallStatus {
    Installed,
    AlreadyPresent,
}

/// 首启安装入口:prefix 非空则跳过;否则 staging 解包 + 补链 + rename
pub fn ensure_prefix(prefix: &Path, zip_bytes: &[u8]) -> Result<InstallStatus, String> {
    // 幂等闸:prefix 存在且非空 = 环境已装好,zip 看都不看
    if prefix.is_dir()
        && let Ok(mut it) = fs::read_dir(prefix)
        && it.next().is_some()
    {
        return Ok(InstallStatus::AlreadyPresent);
    }

    let parent = prefix.parent().ok_or("prefix 无父目录")?;
    let leaf = prefix.file_name().ok_or("prefix 无末段名")?;
    let staging = parent.join(format!("{}-staging", leaf.to_string_lossy()));

    // 半途失败/上次残留的 staging 一律清掉重来;失败路径也清(可重试)
    let result = install_to_staging(&staging, zip_bytes).and_then(|()| {
        // rename 前若 prefix 以空目录形态存在,先摘掉
        if prefix.exists() {
            fs::remove_dir(prefix).map_err(|e| format!("摘空 prefix 失败: {e}"))?;
        }
        fs::rename(&staging, prefix).map_err(|e| format!("staging → prefix rename 失败: {e}"))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(InstallStatus::Installed)
}

fn install_to_staging(staging: &Path, zip_bytes: &[u8]) -> Result<(), String> {
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|e| format!("清 staging 失败: {e}"))?;
    }
    fs::create_dir_all(staging).map_err(|e| format!("建 staging 失败: {e}"))?;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| format!("bootstrap zip 打不开: {e}"))?;
    let mut symlinks_txt: Option<String> = None;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读 zip 条目 #{i} 失败: {e}"))?;
        // enclosed_name 防路径逃逸(zip slip)
        let Some(name) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        if name == Path::new("SYMLINKS.txt") {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s)
                .map_err(|e| format!("读 SYMLINKS.txt 失败: {e}"))?;
            symlinks_txt = Some(s);
            continue;
        }
        let target = staging.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("建目录 {name:?} 失败: {e}"))?;
            continue;
        }
        if let Some(p) = target.parent() {
            fs::create_dir_all(p).map_err(|e| format!("建父目录 {name:?} 失败: {e}"))?;
        }
        let mut out =
            fs::File::create(&target).map_err(|e| format!("建文件 {name:?} 失败: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("写文件 {name:?} 失败: {e}"))?;
        // 官方规则:bin/、libexec、lib/apt/apt-helper、lib/apt/methods → 0700
        let mode: u32 = if name.starts_with("bin/")
            || name.starts_with("libexec")
            || name.starts_with("lib/apt/apt-helper")
            || name.starts_with("lib/apt/methods")
        {
            0o700
        } else {
            0o644
        };
        fs::set_permissions(&target, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("chmod {name:?} 失败: {e}"))?;
    }

    let symlinks = symlinks_txt.ok_or("zip 里没有 SYMLINKS.txt——不是合法 bootstrap")?;
    for line in symlinks.lines() {
        let (target, link) = line
            .split_once('\u{2190}')
            .ok_or_else(|| format!("SYMLINKS.txt 行畸形: {line}"))?;
        let link_path = staging.join(link);
        if let Some(p) = link_path.parent() {
            fs::create_dir_all(p).map_err(|e| format!("建链接父目录 {link} 失败: {e}"))?;
        }
        std::os::unix::fs::symlink(target, &link_path)
            .map_err(|e| format!("建符号链接 {link} ← {target} 失败: {e}"))?;
    }
    Ok(())
}

/// second-stage 命令组装:`$PREFIX/bin/bash <second-stage 入口>`,
/// 环境给 PATH/LD_LIBRARY_PATH/PREFIX/HOME(postinst 里的 dpkg 系工具
/// 全靠这俩变量找到自己和库)。执行归壳(同步等待,失败 = wipe prefix 重装)
pub fn second_stage_command(prefix: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(prefix.join("bin/bash"));
    cmd.arg(
        prefix.join("etc/termux/termux-bootstrap/second-stage/termux-bootstrap-second-stage.sh"),
    )
    .env("PATH", prefix.join("bin"))
    .env("LD_LIBRARY_PATH", prefix.join("lib"))
    .env("PREFIX", prefix)
    .env("TERMUX__PREFIX", prefix);
    if let Some(home) = prefix
        .parent()
        .and_then(|rootfs| rootfs.parent())
        .map(|files| files.join("home"))
    {
        cmd.env("HOME", home);
    }
    cmd
}
