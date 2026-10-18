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

/// 幂等闸谓词：prefix 存在且非空 = 环境已装好（BAR-074 剥给壳前置用——
/// 壳必须先问它再决定读不读 32MB 资产；ensure_prefix 内部同闸双保险，
/// 语义单源在此，不许各写一份）
///
/// BAR-229 追平「可用」语义：非空之外还须 bin/ 下有任一可执行位——
/// 换机克隆会把 files/usr 整体搬来但丢 owner x 位（DAC），旧闸放行
/// 环境死样本（Neo 11 实证：ssh 娃 spawn EACCES × 1034、sshd 起不来、
/// 闸门/deploy 全瘫）。哨兵取「bin/ 任一 x 位」而非点名 bash：判据
/// 通用（bin/ 是 bootstrap 必带目录），不绑死件名。
pub fn prefix_ready(prefix: &Path) -> bool {
    // ReadyNeedsConf 也算就绪（conf 由 keeper 自生成；见该档注释的鸡生蛋死锁）
    matches!(
        prefix_state(prefix),
        PrefixState::Ready | PrefixState::ReadyNeedsConf
    )
}

/// prefix 体检分档（2026-10-18 MAIN0192 楼4/6：原判据「目录非空＋bin 任一 x 位」
/// 太宽——bin/sshd 缺也能判就绪，keeper 一启动就 ENOENT 永久挂起，报一行了事，
/// 现场只能靠人猜。分档后报账直接写是哪一档，取证不用再猜）。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PrefixState {
    /// 环境可用：bin 有 x 位文件、bin/sh 链接健康、**bin/sshd 与
    /// etc/ssh/kfm-sshd.conf 同时在位**（后两项＝本次收紧的判据：
    /// 8024 闸门与 kfm-sshd 是这套环境的对外两条腿，缺一个就不算就绪）
    Ready,
    /// prefix 目录本身不存在
    NoDir,
    /// prefix 目录是空的
    Empty,
    /// bin/ 目录读不到
    NoBin,
    /// bin/ 下没有一个可执行位文件（换机克隆丢 owner x 位，BAR-229）
    NoExec,
    /// bin/sh 链接断或缺失（BAR-230 三态里的非健康两种）
    ShBroken,
    /// **bin/sshd 不在位**（本次新增档：8024 闸门起不来的直接原因）
    NoSshdBin,
    /// **etc/ssh/kfm-sshd.conf 不在位**，但 bin/sshd 在——**这一档仍算就绪**
    /// （BAR-259：conf 由 keeper 的 ensure_conf 自生成）。若把它判成不就绪，
    /// keeper 不启动 ⇒ conf 永远没人写 ⇒ 鸡生蛋死锁（2026-10-18 11 机实证：
    /// openssh 装好、host key 已生成，8024 就是不起，根子正是这条）。
    ReadyNeedsConf,
    /// 保留档位名以便报表可读；**不再由 prefix_state 产出**（并入 ReadyNeedsConf）
    NoSshdConf,
}

/// prefix 分档体检（纯函数式的目录检查，可判卷）
pub fn prefix_state(prefix: &Path) -> PrefixState {
    if !prefix.is_dir() {
        return PrefixState::NoDir;
    }
    let Ok(mut entries) = fs::read_dir(prefix) else {
        return PrefixState::NoBin;
    };
    if entries.next().is_none() {
        return PrefixState::Empty;
    }
    let Ok(bin) = fs::read_dir(prefix.join("bin")) else {
        return PrefixState::NoBin;
    };
    let any_exec = bin.filter_map(|e| e.ok()).any(|e| {
        fs::metadata(e.path())
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    });
    if !any_exec {
        return PrefixState::NoExec;
    }
    // BAR-230：符号链接判据——克隆搬运整类丢符号链接，只堵 x 位漏这一半
    if sh_link_state(prefix) != ShLink::Healthy {
        return PrefixState::ShBroken;
    }
    if !prefix.join("bin").join("sshd").exists() {
        return PrefixState::NoSshdBin;
    }
    if !prefix
        .join("etc")
        .join("ssh")
        .join("kfm-sshd.conf")
        .exists()
    {
        // conf 缺 → 仍算就绪（keeper 的 ensure_conf 会自生成）；单独成档只为报表可读。
        // 判成不就绪＝keeper 不启＝conf 永无人生成＝鸡生蛋死锁（11 机实证）。
        return PrefixState::ReadyNeedsConf;
    }
    PrefixState::Ready
}

/// bin/sh 链接三态（BAR-230，编址照 kfmv4 旧路径存在性三分类——小满
/// MAIN0116 指路）：链接在且目标可解析＝健康；链接在但目标没了＝真断
/// （克隆/搬运伤链接的典型形态）；链接本身没了＝可能重装没建链接。
/// 三档分开报——处置不同。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ShLink {
    /// 链接在且目标可解析（fs::metadata 跟随成功）
    Healthy,
    /// 链接在但目标解析失败
    Dangling,
    /// 链接本身不存在
    Missing,
}

pub fn sh_link_state(prefix: &Path) -> ShLink {
    let sh = prefix.join("bin/sh");
    if fs::symlink_metadata(&sh).is_err() {
        return ShLink::Missing;
    }
    if fs::metadata(&sh).is_ok() {
        ShLink::Healthy
    } else {
        ShLink::Dangling
    }
}

/// 首启安装入口:prefix 非空则跳过;否则 staging 解包 + 补链 + rename。
/// BAR-229 自愈闸：prefix 存在但哨兵不可执行（克隆丢 x 位等环境死）
/// → wipe 重装——重装产物走 install_to_staging 的显式 chmod，免疫
/// umask，装完即恢复可用。
pub fn ensure_prefix(prefix: &Path, zip_bytes: &[u8]) -> Result<InstallStatus, String> {
    // 幂等闸:prefix 存在且可用 = 环境已装好,zip 看都不看
    if prefix_ready(prefix) {
        return Ok(InstallStatus::AlreadyPresent);
    }

    let parent = prefix.parent().ok_or("prefix 无父目录")?;
    let leaf = prefix.file_name().ok_or("prefix 无末段名")?;
    let staging = parent.join(format!("{}-staging", leaf.to_string_lossy()));

    // 半途失败/上次残留的 staging 一律清掉重来;失败路径也清(可重试)
    let result = install_to_staging(&staging, zip_bytes).and_then(|()| {
        // rename 前若 prefix 以目录形态存在(空目录,或 BAR-229 哨兵
        // 不可执行的环境死样本)——一律摘掉再 rename(remove_dir 对
        // 非空目录会失败,故用 remove_dir_all)
        if prefix.exists() {
            fs::remove_dir_all(prefix).map_err(|e| format!("摘旧 prefix 失败: {e}"))?;
        }
        fs::rename(&staging, prefix).map_err(|e| format!("staging → prefix rename 失败: {e}"))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(InstallStatus::Installed)
}

/// 资产能力判定（BAR-272，MAIN0192 楼36/37 白露拍）：**资产里有没有 bin/sshd**。
/// 这条是 wipe 的前置——判据收紧（BAR-269 要求 bin/sshd 在位）＋旧资产（不含
/// sshd）＝每次启动都判不就绪 ⇒ 每启 wipe 重装，比一次卸载重装更坏（静默丢
/// keys/overlay）。故：资产不含 sshd 时**不许 wipe**，按旧判据视为已装并报一行。
pub fn asset_has_sshd(zip_bytes: &[u8]) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)) else {
        return false; // 打不开就当没有——保守，不 wipe
    };
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        if entry
            .enclosed_name()
            .map(|p| p == Path::new("bin/sshd"))
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// wipe 后密钥回灌（BAR-272，MAIN0192 楼41/43：白露按代码改正——wipe 丢的是
/// **公钥清单**，不是私钥；host key 由 sshd 首启自生成，故回灌不阻塞 wipe，
/// 只是「搬了就有探针钥匙，没搬就报一行」）。
/// 源目录＝共享存储固定落点（na 有 MANAGE_EXTERNAL_STORAGE，读得到；
/// 跨 uid 只有这一个共同面）。
pub const KEY_SRC_DIR: &str = "/storage/emulated/0/kfm-na-keys";
/// 默认回灌清单：**只有公钥清单一件**（MAIN0192 楼54/55：11 机需求是「服务器连进来」
/// ⇒ 只需公钥；私钥是出向用的客户端钥匙，默认不落共享存储——今天下午刚花一轮防
/// 私钥上公网，不能为修 prefix 又摊出去）。
pub const RESTORE_KEYS: [(&str, u32); 1] = [("na_authorized_keys", 0o600)];

/// 私钥**显式开关**（一次性动作）：源目录里放这个标记文件才回灌私钥。
/// 存在即视为「有人明确决定这次要做」，并落两行账留痕。
pub const PRIVATE_KEY_ALLOW_MARK: &str = "ALLOW_PRIVATE_KEY";
/// 私钥件名（仅在显式开关在位时回灌）
pub const PRIVATE_KEY_FILE: &str = "id_ed25519";

/// 资产来源选择（BAR-274，纯函数可判卷）：**外部覆盖优先**。
/// 返回 (选中字节, 是否为覆盖源)。两处（能力判定／安装）都用它，防半截。
pub fn pick_asset_source(
    override_bytes: Option<Vec<u8>>,
    embedded: Option<Vec<u8>>,
) -> Option<(Vec<u8>, bool)> {
    match (override_bytes, embedded) {
        (Some(v), _) => Some((v, true)),
        (None, Some(v)) => Some((v, false)),
        (None, None) => None,
    }
}

/// 测试夹具：含 `bin/sshd` 的最小 zip（宿主可判卷，不进生产路径）
pub fn test_fixture_zip_with_sshd() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default();
    w.start_file("bin/sshd", opt).unwrap();
    std::io::Write::write_all(&mut w, b"fake-sshd").unwrap();
    w.finish().unwrap().into_inner()
}

/// 测试夹具：不含 `bin/sshd` 的最小 zip（模拟旧资产）
pub fn test_fixture_zip_without_sshd() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default();
    w.start_file("bin/bash", opt).unwrap();
    std::io::Write::write_all(&mut w, b"fake-bash").unwrap();
    w.finish().unwrap().into_inner()
}

/// 私钥是否被显式放行（标记文件在位）
pub fn private_key_allowed(src_dir: &Path) -> bool {
    src_dir.join(PRIVATE_KEY_ALLOW_MARK).is_file()
}

/// 从源目录把密钥搬进 `prefix/etc/ssh`（幂等覆盖）。返回每件的搬运结果。
/// **默认只搬公钥清单**；私钥仅在显式开关在位时搬（见 private_key_allowed）。
/// 缺件不报错（不阻塞）——调用方按返回结果落账即可。
pub fn restore_keys_from(src_dir: &Path, prefix: &Path) -> Vec<(String, bool)> {
    let dst_dir = prefix.join("etc").join("ssh");
    let allow_priv = private_key_allowed(src_dir);
    let mut plan: Vec<(&str, u32)> = RESTORE_KEYS.to_vec();
    if allow_priv {
        plan.push((PRIVATE_KEY_FILE, 0o600));
    }
    let mut out = Vec::new();
    for (name, mode) in plan {
        let src = src_dir.join(name);
        let ok = if src.is_file() {
            fs::create_dir_all(&dst_dir).is_ok() && fs::copy(&src, dst_dir.join(name)).is_ok() && {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(dst_dir.join(name), fs::Permissions::from_mode(mode))
                        .is_ok()
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
        } else {
            false
        };
        out.push((name.to_owned(), ok));
    }
    out
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

// ---- Android 壳(B 档平台胶水,判卷 = 真机实拍 + [boot] 上报行) ----

#[cfg(target_os = "android")]
mod android_shell {
    use std::io::Read;
    use std::path::PathBuf;

    use jni::objects::JObject;
    use jni::{JavaVM, jni_sig, jni_str};
    use winit::platform::android::activity::AndroidApp;

    /// assets 里的 bootstrap 包名(只产 aarch64——手机架构;其余架构未来再说)
    const BOOTSTRAP_ASSET: &str = "bootstrap-aarch64.zip";
    /// 外部覆盖资产名（BAR-274，MAIN0193 楼67 卡萝实证「本地执行壳通了」）：
    /// 若 `files/<此名>` 在位，**优先于 APK 内嵌资产**——这样投递通道不必是
    /// 「装新包」：任何能写 na 私有目录的腿（本地执行壳／热更通道）丢一份
    /// 新资产进去，下次首启即用它重装，gate 不用等安装器。
    const BOOTSTRAP_ASSET_OVERRIDE: &str = "bootstrap-override.zip";

    /// 应用私有 files 目录(JNI getFilesDir——不硬编码,多用户/工作资料
    /// 档案自适应;AndroidApp 句柄只在壳侧有,local_pty 线程那份是硬编码)
    fn app_files_dir(app: &AndroidApp) -> Option<PathBuf> {
        // SAFETY: vm_as_ptr 是 android-activity 保证有效的 JavaVM 指针
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let raw = app.activity_as_ptr() as jni::sys::jobject;
        vm.attach_current_thread(|env| -> jni::errors::Result<PathBuf> {
            // SAFETY: 全局引用归 android-activity 所有,只包视图不接管
            let activity = unsafe { JObject::from_raw(env, raw) };
            let dir = env
                .call_method(
                    &activity,
                    jni_str!("getFilesDir"),
                    jni_sig!("()Ljava/io/File;"),
                    &[],
                )?
                .l()?;
            let path_obj = env
                .call_method(
                    &dir,
                    jni_str!("getAbsolutePath"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let s = env
                .cast_local::<jni::objects::JString>(path_obj)?
                .try_to_string(env)?;
            Ok(PathBuf::from(s))
        })
        .ok()
    }

    /// kfm-pkg 运行时安装器资产名(L2 overlay,设计 docs/active/l2-overlay.md)
    const PKG_TOOL_ASSET: &str = "kfm-pkg";

    /// kfm-pkg 每启覆盖铺进 $PREFIX/bin:版本随 APK 自然滚动,升级零仪式。
    /// prefix 不在(裸包)就跳过;kfm-pkg 自己是 shell 脚本,0700。
    pub fn ensure_pkg_tool(app: &AndroidApp) {
        let Some(files) = app_files_dir(app) else {
            return;
        };
        let bin = files.join("usr/bin");
        if !bin.is_dir() {
            return;
        }
        let assets = app.asset_manager();
        let name = std::ffi::CString::new(PKG_TOOL_ASSET).unwrap();
        let Some(mut asset) = assets.open(&name) else {
            return; // 资产缺席(裸包)——不吵,装包脚本永远带上它
        };
        let mut bytes = Vec::with_capacity(asset.length());
        if std::io::Read::read_to_end(&mut asset, &mut bytes).is_err() {
            return;
        }
        let dst = bin.join(PKG_TOOL_ASSET);
        if std::fs::write(&dst, &bytes).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o700));
        }
    }

    /// 首启安装(在 conn-provider-local 装载前调):assets 读 zip →
    /// ensure_prefix → 新装则同步跑 second-stage。全程 report_sync
    /// (安装期终端网格还没建,日志是唯一观测面)
    pub fn first_boot_install(app: &AndroidApp) {
        let Some(files) = app_files_dir(app) else {
            crate::report::report_sync("boot", "L3: getFilesDir 失败——跳过 bootstrap 安装");
            return;
        };
        let prefix = files.join("usr");
        // BAR-074：幂等闸前置——已装好就一个字节都不读。旧序把 32MB
        // 资产全读进内存才调 ensure_prefix(它内部第一行就跳过)——启动
        // 关键路径每启裸读 32MB。模块文档原契约即「zip 字节都不解析」,
        // 壳把闸挪到读之前才算兑现。(注:PIN-boot 挂卷族真凶后查明是
        // 熄屏 freezer 中段冻结,见 BAR-075——本修复仍成立:白读该摘)
        if super::prefix_ready(&prefix) {
            crate::report::report("boot", "L3: 环境已装——跳过(幂等闸前置)");
            return;
        }
        // BAR-229：存在但不可用（克隆丢 x 位等环境死）——自愈 wipe 重装
        // BAR-272（MAIN0192 楼36/37 白露拍）：**wipe 前置＝资产内 bin/sshd 存在**。
        // 判据收紧（BAR-269 要 bin/sshd）＋旧资产（不含 sshd）＝每启都判不就绪 ⇒
        // 每启 wipe（静默丢数据，比一次卸载重装更坏）。故先核资产能力：
        // 供不出 sshd 就不 wipe（wipe 也装不出就绪态）；供得出就必须让它 wipe 一次。
        let assets = app.asset_manager();
        let name = std::ffi::CString::new(BOOTSTRAP_ASSET).unwrap();
        // BAR-274：外部覆盖优先（同一份字节流，后面全走同一路径）
        let override_path = files.join(BOOTSTRAP_ASSET_OVERRIDE);
        let override_bytes = if override_path.is_file() {
            match std::fs::read(&override_path) {
                Ok(v) => {
                    crate::report::report_sync(
                        "boot",
                        &format!(
                            "L3: 用**外部覆盖资产**（{} 字节，源=files/{}）",
                            v.len(),
                            BOOTSTRAP_ASSET_OVERRIDE
                        ),
                    );
                    Some(v)
                }
                Err(e) => {
                    crate::report::report_sync(
                        "boot",
                        &format!("L3: 外部覆盖资产读失败（{e}）——回落内嵌"),
                    );
                    None
                }
            }
        } else {
            None
        };
        if prefix.is_dir() {
            let cap = match override_bytes.clone() {
                Some(v) => v,
                None => {
                    let mut v = Vec::new();
                    if let Some(mut a) = assets.open(&name) {
                        let _ = a.read_to_end(&mut v);
                    }
                    v
                }
            };
            if !super::asset_has_sshd(&cap) {
                crate::report::report_sync(
                    "boot",
                    "L3: 资产不含 bin/sshd——按旧判据视为已装、**不 wipe**（BAR-272：wipe 也装不出就绪态）",
                );
                return;
            }
            crate::report::report(
                "boot",
                "L3: prefix 存在但哨兵不可执行——自愈 wipe 重装（BAR-229；BAR-272 已核资产含 sshd）",
            );
        }
        // BAR-274：安装字节流优先取外部覆盖资产（与上面判定段同一份）
        let mut bytes = match override_bytes {
            Some(v) => v,
            None => {
                let Some(mut asset) = assets.open(&name) else {
                    crate::report::report_sync(
                        "boot",
                        "L3: 无内嵌 bootstrap(zip 未入包)——本地会话回落系统 sh",
                    );
                    return;
                };
                let mut v = Vec::with_capacity(asset.length());
                if let Err(e) = asset.read_to_end(&mut v) {
                    crate::report::report_sync("boot", &format!("L3: 读 bootstrap 资产失败: {e}"));
                    return;
                }
                v
            }
        };
        crate::report::report(
            "boot",
            &format!("L3: bootstrap 资产 {} 字节,开始安装", bytes.len()),
        );
        match super::ensure_prefix(&prefix, &bytes) {
            Ok(super::InstallStatus::AlreadyPresent) => {
                crate::report::report("boot", "L3: 环境已装——跳过");
            }
            Ok(super::InstallStatus::Installed) => {
                crate::report::report_sync("boot", "L3: 解包完成,跑 second-stage");
                match super::second_stage_command(&prefix).status() {
                    Ok(s) if s.success() => {
                        crate::report::report_sync("boot", "L3: second-stage 完成 ✅")
                    }
                    other => crate::report::report_sync(
                        "boot",
                        &format!("L3: second-stage 异常: {other:?}(环境在,下轮重启自愈)"),
                    ),
                }
                // BAR-272（MAIN0192 楼54/55 白露/清和裁）：装完从共享存储回灌密钥。
                // **默认只搬公钥清单**（11 机需求是「服务器连进来」）；出向私钥
                // 仅在显式开关（源目录放 ALLOW_PRIVATE_KEY）在位时搬——留两行账。
                let src_dir = std::path::Path::new(super::KEY_SRC_DIR);
                let allow_priv = super::private_key_allowed(src_dir);
                crate::report::report_sync(
                    "boot",
                    &format!(
                        "L3: 密钥回灌清单＝{}；私钥开关={}（源={}）",
                        if allow_priv {
                            "na_authorized_keys＋id_ed25519"
                        } else {
                            "na_authorized_keys（私钥默认不落）"
                        },
                        if allow_priv {
                            format!("显式开启（{} 在位）", super::PRIVATE_KEY_ALLOW_MARK)
                        } else {
                            "关".to_owned()
                        },
                        super::KEY_SRC_DIR
                    ),
                );
                for (name, ok) in super::restore_keys_from(src_dir, &prefix) {
                    crate::report::report_sync(
                        "boot",
                        &format!(
                            "L3: 密钥回灌 {name} {}（源={}）",
                            if ok {
                                "成功"
                            } else {
                                "未备（不阻塞）"
                            },
                            super::KEY_SRC_DIR
                        ),
                    );
                }
            }
            Err(e) => crate::report::report_sync("boot", &format!("L3: 安装失败: {e}")),
        }
    }
}

#[cfg(target_os = "android")]
pub use android_shell::{ensure_pkg_tool, first_boot_install};
