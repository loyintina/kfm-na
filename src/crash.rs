//! crash.rs — 信号级坠机记录(2026-08-27,自观测第四块①)
//!
//! panic 钩子是 Rust 层的;native 崩溃(SIGSEGV/SIGBUS/SIGILL/SIGABRT)
//! 绕过它——jni/ndk/字体光栅化真段错误,进程直接没,panic.log 一个字
//! 不留,只能看到「没 boot 行」。本模块装 last-gasp 信号处理器:
//! 异步信号安全(只 write 到预开 fd,零分配零锁),写一行后 re-raise
//! 交还系统(内核 tombstone/logcat 照留,我们不截胡)。
//!
//! SIGUSR1 = 测试探针:写行后**继续活**——信号路径端到端可装机判卷
//! (kill -USR1 $(cat na.pid) → panic.log 应多一行,进程不死)。

use std::sync::atomic::{AtomicI32, Ordering};

/// 预开的 panic.log fd(-1 = 未装)。handler 里只许碰这个
static CRASH_FD: AtomicI32 = AtomicI32::new(-1);

/// 坠机行格式(纯函数,钉死):`SIGNAL sig=11 addr=0xdeadbeef\n`。
/// 手写十/十六进制进固定栈缓冲——handler 里 format! 会分配,不许用;
/// 这函数是全模块唯一 formatting,也是唯一需要考题的地方
pub fn format_signal_line(sig: i32, addr: usize, buf: &mut [u8]) -> usize {
    let mut n = 0;
    let mut push = |bytes: &[u8], n: &mut usize| {
        for &b in bytes {
            if *n < buf.len() {
                buf[*n] = b;
                *n += 1;
            }
        }
    };
    push(b"SIGNAL sig=", &mut n);
    // 十进制信号号(倒序入临时,再倒回来)
    let mut tmp = [0u8; 20];
    let mut m = 0;
    let mut v = sig.unsigned_abs();
    loop {
        tmp[m] = b'0' + (v % 10) as u8;
        m += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while m > 0 {
        m -= 1;
        push(&[tmp[m]], &mut n);
    }
    push(b" addr=0x", &mut n);
    // 十六进制地址(同上倒序;0 也要出一个 0)
    let mut m = 0;
    let mut v = addr;
    loop {
        let d = (v & 0xf) as u8;
        tmp[m] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        m += 1;
        v >>= 4;
        if v == 0 {
            break;
        }
    }
    while m > 0 {
        m -= 1;
        push(&[tmp[m]], &mut n);
    }
    push(b"\n", &mut n);
    n
}

/// 信号处理器本体:写行(尽力而为)→ SIGUSR1 返回继续活,其余 re-raise
unsafe extern "C" fn on_signal(sig: i32, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    let addr = if info.is_null() {
        0
    } else {
        // si_addr:故障地址(SIGSEGV/SIGBUS 有,其余为 null)
        unsafe { (*info).si_addr() as usize }
    };
    let fd = CRASH_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let mut buf = [0u8; 128];
        let n = format_signal_line(sig, addr, &mut buf);
        unsafe {
            libc::write(fd, buf.as_ptr().cast(), n);
        }
    }
    if sig == libc::SIGUSR1 {
        return; // 测试探针:写行已证链路活,进程继续
    }
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// 装机(android_main 紧跟 panic 钩子之后):
/// ①预开 panic.log(append)fd 登记进 CRASH_FD——handler 里现 open 不
///   安全,只能先开好;
/// ②sigaction 挂 SIGSEGV/SIGBUS/SIGILL/SIGABRT(致命,写完 re-raise)
///   加 SIGUSR1(测试探针,写完继续活);
/// ③pid 落 na.pid——ssh 侧 kill 判卷。
/// 全部失败静默(观测铁律:信号钩子不许自己成为死因)。
pub fn install_signal_hook(dir: &str) {
    use std::os::unix::io::IntoRawFd;
    let path = std::path::PathBuf::from(dir).join(crate::gate::PANIC_FILE);
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        CRASH_FD.store(f.into_raw_fd(), Ordering::Relaxed);
    }
    std::fs::write(
        std::path::PathBuf::from(dir).join("na.pid"),
        format!("{}\n", std::process::id()),
    )
    .ok();
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as *const () as usize;
        // SA_SIGINFO = 拿 siginfo(故障地址);不加 SA_RESETHAND——
        // 交还系统走 handler 里手动 signal+raise,双保险会双杀
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigemptyset(&mut sa.sa_mask);
        for sig in [
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGILL,
            libc::SIGABRT,
            libc::SIGUSR1,
        ] {
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
    }
}
