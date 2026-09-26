//! install.rs — na 自更新原语（BAR-162，2026-09-26）：把私有目录里的 APK
//! 递交给系统安装器，**走 Activity 上下文**。
//!
//! 为什么必须走 Activity 上下文（本单缘起，研究线定罪）：经 QUIC 反连桥在
//! na 沙箱里跑的 `am start`（termux-am 壳，na uid）被 vivo 按进程态判 BAL
//! 静默吞——连浏览器 VIEW 都不弹、exit=0 无输出，而 `pm list packages` 照通
//! （IPC 活着）= 不是壳坏，是桥 spawn 的 app_process 不是可见 Activity 的
//! 宿主进程。用户已给「安装未知应用」全权限仍然无效——BAL 判的是进程态，
//! 不是权限。MainActivity 所在进程是前台进程，从 Activity 上下文
//! startActivity 必通。
//!
//! 分工：**文件/URI 半边 = 本模块纯函数**（A 档考题：名字净化 / URI 拼装 /
//! 落定入 incoming 免白拷 / 递交前落定必须原子）；**UI 半边** = Java 皮
//! `installApkFromGate(uri)`（runOnUiThread → ACTION_VIEW + grant 标志），
//! 以及 android_app 里的**引导腿**（现行装机 APK 没有新方法时，门线程直调
//! `activity.startActivity(Intent)`——自更新原语的零引导义：不装包也能用，
//! 装了包换回 UI 线程正道）。两腿判决都落 usr/tmp/install-status 一行账。
//!
//! 与 KfmFileProvider 的契约（注意：那是**手写** ContentProvider，不是
//! androidx FileProvider，没有 res/xml/file_paths 那套）：根锁死在
//! `{files}/incoming/`，只认 `content://dev.kfm.na.provider/apk/<单层名>`，
//! 名字含 `..` / 空 / 带层数一律拒（resolve 里抛 FileNotFound）。故
//! **落定动作必须发生在递交之前**——本模块的 stage_apk 就是那一步。

/// 递交通道 authority（与 AndroidManifest 的 provider 同源，改一处必改两处）
pub const PROVIDER_AUTHORITY: &str = "dev.kfm.na.provider";

/// 递交通道根目录名（KfmFileProvider.root() 锁死的唯一目录）
pub const INCOMING_DIR: &str = "incoming";

/// 安装包 MIME（ACTION_VIEW 认这个才交给安装器）——与 provider 的 getType 同源
pub const APK_MIME: &str = "application/vnd.android.package-archive";

/// 从路径里取安装包名：取末段、拒空/含 `..`/非 .apk——与 provider 的
/// resolve 同口径（名字不合规就不必走到递交那一步白弹一次）。
/// 变异方向：放行 `..`（穿越进 provider）/ 不取末段（URI 多一层 = 必被拒）。
pub fn apk_name(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?.trim();
    if name.is_empty() || name.contains("..") || name.contains('\0') {
        return None;
    }
    if !name.to_ascii_lowercase().ends_with(".apk") {
        return None;
    }
    Some(name.to_string())
}

/// content:// URI（形态唯一合法：/apk/<单层名>）
pub fn apk_uri(name: &str) -> String {
    format!("content://{PROVIDER_AUTHORITY}/apk/{name}")
}

/// 落定结果：落地路径（provider 能读的那一份）+ 递交 URI + 字节数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub path: String,
    pub uri: String,
    pub bytes: u64,
}

/// 落定：把 src 拷进 `{data_dir}/incoming/<名>`，返回可递交的 URI。
/// 已在 incoming 内则原地用（免 42MB 白拷——脚本推包本来就落那里）。
/// **先写 .new 再改名**：递交前落定必须原子，半截 APK 递给安装器 = 装出
/// 个坏包（同 hot/ 热更与 BAR-150 的教训，同一条纪律的第三个落点）。
pub fn stage_apk(data_dir: &str, src: &str) -> Result<Staged, String> {
    let name = apk_name(src).ok_or_else(|| format!("非法安装包名/路径: {src}"))?;
    let src_path = std::path::Path::new(src);
    let meta = std::fs::metadata(src_path).map_err(|e| format!("读不到源包 {src}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("源不是文件: {src}"));
    }
    let incoming = std::path::Path::new(data_dir).join(INCOMING_DIR);
    std::fs::create_dir_all(&incoming).map_err(|e| format!("建 {INCOMING_DIR}/ 失败: {e}"))?;
    let dst = incoming.join(&name);
    // 「已在通道内」判据吃规范化路径（相对/软链/多斜杠都归一），判不出就照拷
    // ——多拷一次无害，漏拷一次 = 递给安装器一个不存在的 URI
    let already = match (
        std::fs::canonicalize(src_path),
        std::fs::canonicalize(&incoming),
    ) {
        (Ok(s), Ok(inc)) => s.parent().map(|p| p == inc.as_path()).unwrap_or(false),
        _ => false,
    };
    if !already {
        let tmp = incoming.join(format!("{name}.new"));
        let _ = std::fs::remove_file(&tmp);
        std::fs::copy(src_path, &tmp).map_err(|e| format!("拷贝进 {INCOMING_DIR}/ 失败: {e}"))?;
        std::fs::rename(&tmp, &dst).map_err(|e| format!("落定改名失败: {e}"))?;
    }
    let bytes = std::fs::metadata(&dst)
        .map_err(|e| format!("落定后读不到 {}: {e}", dst.display()))?
        .len();
    Ok(Staged {
        path: dst.to_string_lossy().into_owned(),
        uri: apk_uri(&name),
        bytes,
    })
}

/// 判决落账：`{dump_dir}/install-status` 快照覆盖写（判卷一条账）。Java 腿
/// 由 Java 皮自己写同一份（同族 web-status/rec-status 的写法），引导腿由这里
/// 写——谁跑谁写，一次递交只落一行。
pub fn write_status(dump_dir: &str, verdict: &str) -> std::io::Result<()> {
    std::fs::write(
        std::path::Path::new(dump_dir).join("install-status"),
        format!("{verdict}\n"),
    )
}
