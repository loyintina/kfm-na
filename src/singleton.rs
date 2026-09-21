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
