//! exec_probe.rs — L2/L3 总开关探针(2026-08-20 用户拍板):私有目录 exec 验证
//!
//! 判的问题:Android 10+ 对 targetSdk≥29 应用摘除 app_data_file 的 exec 权
//! (SELinux untrusted_app_29+ 域;我们的 APK aapt2 链接时 target-sdk=35,
//! 理论上在封锁区)。真机实证一次,答案决定 busybox(L2)与 apt 生态(L3)
//! 走哪条路:
//!   放行 → 私有目录直接 exec,L3(换前缀 termux-packages)可行;
//!   拒绝(EACCES)→ 私有目录永封,只剩 jniLibs lib*.so 伪装(= 打包期固定
//!   二进制集,L2 可行、L3 的 apt 运行时装包不可行)。
//!
//! 载荷:assets/probe/hello-aarch64(NDK clang 编,源码 hello.c 同目录)。
//! 机制:写进 internal_data_path → chmod 755 → std::process::Command
//! (std 内部用 pipe 把子进程 execve 的 errno 带回父进程,正是我们要的)。
//! 结果走飞鸽传书,真机判卷 = 日志里一行「exec 探针: 放行/拒绝」。

/// 探针载荷字节(编译期内嵌,APK 资产不落盘也能到)
static PAYLOAD: &[u8] = include_bytes!("../assets/probe/hello-aarch64");

/// 跑一次探针并上报。只冷启动时调一次(android_app init_terminal)。
/// 返回是否放行(调用方未来可按结果切 L2/L3 路线,v1 只上报)
pub fn run(files_dir: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let path = files_dir.join("probe-hello");
    let verdict = (|| -> Result<String, String> {
        std::fs::write(&path, PAYLOAD).map_err(|e| format!("写载荷失败: {e}"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod 失败: {e}"))?;
        match std::process::Command::new(&path).output() {
            Ok(out) => Ok(format!(
                "放行 ✅ exit={} stdout={}",
                out.status,
                String::from_utf8_lossy(&out.stdout).trim()
            )),
            Err(e) => Ok(format!(
                "拒绝 ❌ errno={:?}({e})——私有目录 exec 被封",
                e.raw_os_error()
            )),
        }
    })();
    match &verdict {
        Ok(v) => crate::report::report_sync("probe", &format!("exec 探针: {v}")),
        Err(e) => crate::report::report_sync("probe", &format!("exec 探针自身故障: {e}")),
    }
    verdict.is_ok_and(|v| v.starts_with("放行"))
}
