//! singleton.rs — 单实例闸（BAR-127 转修，2026-09-21 夜立案）。
//!
//! 病：`android_main` 多实例（冷启动多进 / 热更重启后残留进程没死透 /
//! ROM 冻结保住旧进程）→ 每个实例各起一套子系统：各跑隧道看门狗**互抢
//! 反连口**（BAR-128 抓到的 `remote port forwarding failed for listen port
//! 9022` 真因之一）、各跑 nasup 看门狗（同一秒多次状态迁移，2026-09-21 夜
//! 实测：封掉在途相发布后日志仍在 1~3s 一跳 = 旧 .so 实例还活着）、各报
//! 一份心跳（×N 锁步污染遥测）。BAR-037 的 `ANDROID_MAIN_RAN` 只管**同
//! 进程重跑**，管不到多个进程。
//!
//! 修：**flock 独占锁**——锁文件在应用私有目录，进程死内核自动释放
//! （从构造上不存在「陈旧锁卡住下一次启动」），第二实例抢不到就写遗言
//! 退出，绝不让子系统起跑。
//!
//! 分层：`lock_exclusive`/`try_acquire_at` 是 B 档系统调用胶水（判卷 =
//! 真拿来两次抢，见 tests/singleton_spec.rs：第二把必败、松手（≈进程死）
//! 后第三把必胜）；路径与遗言文案是常量。

use std::path::Path;

/// 锁文件（应用私有目录；进程死 = 内核释放 fd = 锁自动解开）
pub const LOCK_PATH: &str = "/data/data/dev.kfm.na/files/na-instance.lock";

/// 持锁句柄：进程存活期间必须一直持有——被 drop 掉锁就没了（`static` 存着）
static HELD: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

/// flock 独占非阻塞（B 档）：true = 本进程拿到独占
pub fn lock_exclusive(f: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    rc == 0
}

/// 抢指定路径的锁（测试入口：路径注入，宿主可跑）。判败原因进 `LAST_ERRNO`
/// （诊断用：EWOULDBLOCK = 别人占着；别的 errno = 文件/fd 病）
pub static LAST_ERRNO: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn try_acquire_at(path: &Path) -> Option<std::fs::File> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()?;
    if lock_exclusive(&f) {
        // 写自己的 pid（诊断用：让位的人知道是谁占着）
        use std::io::{Seek, SeekFrom, Write};
        let mut f2 = &f;
        let _ = f2.seek(SeekFrom::Start(0));
        let _ = writeln!(f2, "{}", std::process::id());
        let _ = f2.flush();
        Some(f)
    } else {
        LAST_ERRNO.store(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            std::sync::atomic::Ordering::Relaxed,
        );
        None
    }
}

/// 占位者的 pid（读锁文件内容；读不到 = None——诊断用，不上判决路）
pub fn holder_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// 生产入口：抢应用私有目录那把锁。true = 本进程是唯一实例（句柄存进
/// `HELD` 直到进程结束）；false = 已有活实例，调用方必须**立刻让位退出**
pub fn try_acquire() -> bool {
    let p = Path::new(LOCK_PATH);
    match try_acquire_at(p) {
        Some(f) => {
            *HELD.lock().unwrap() = Some(f);
            true
        }
        None => false,
    }
}

// ---- 残留实例自清（BAR-131，2026-09-21 夜） ----
//
// 单实例闸只挡**新**实例；旧核留下的实例还活着继续抢反连口、刷状态
// （用户侧表现为「打开 na 得反复重启才勉强能用」——每次重启又养出一个新
// 实例，越重启越乱）。故拿到锁之后、任何子系统起跑之前，把**同 uid 的
// 其它 na 进程**清掉。判据三条（严）：①uid 必须等于自己的 uid（用户级
// 隔离，别人的进程一个不碰）②cmdline 必须含本包名（na 沙箱进程与其 ssh
// 子进程都带；Termux 是别的 uid 且不带）③pid != 自己。

/// 该进程该不该清（A 档纯函数——判据在这，扫描是胶水）
pub fn is_reapable(pid: u32, my_pid: u32, uid: u32, my_uid: u32, cmdline: &str, pkg: &str) -> bool {
    pid != my_pid && uid == my_uid && cmdline.contains(pkg)
}

/// 读某 pid 的 uid（/proc/<pid>/status 的 Uid: 首列）
fn proc_uid(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = s.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// 读某 pid 的 cmdline（NUL 分隔 → 空格连接）
fn proc_cmdline(pid: u32) -> Option<String> {
    let b = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if b.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&b).replace('\0', " "))
}

/// 扫 /proc 清残留（B 档胶水）：返回被清掉的 pid 列表。
/// **安卓专属**（2026-09-21 自测实咬）：宿主上跑这条扫描会命中「跑测试的
/// 外壳进程」（cmdline 里恰好带着包名字符串、uid 又是同一个 root）——当场
/// 把自己的 shell 杀了（KILL 语义在非沙箱里没有管辖权）。故非安卓一律空转：
/// 清场只在「同 uid = 同一 app 沙箱」的世界里才成立
pub fn reap_foreign_instances(pkg: &str) -> Vec<u32> {
    if !cfg!(target_os = "android") || pkg.is_empty() {
        return Vec::new();
    }
    let my_pid = std::process::id();
    let my_uid = unsafe { libc::getuid() };
    let mut killed = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return killed;
    };
    for e in rd.flatten() {
        let Some(name) = e.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let (Some(uid), Some(cmd)) = (proc_uid(pid), proc_cmdline(pid)) else {
            continue;
        };
        if is_reapable(pid, my_pid, uid, my_uid, &cmd, pkg) {
            // SIGKILL：旧实例可能已被 ROM 冻结，TERM 未必被处理
            if unsafe { libc::kill(pid as i32, libc::SIGKILL) } == 0 {
                killed.push(pid);
            }
        }
    }
    killed
}
