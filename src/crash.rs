//! crash.rs — 信号级坠机记录(2026-08-27,自观测第四块①)
//!
//! panic 钩子是 Rust 层的;native 崩溃(SIGSEGV/SIGBUS/SIGILL/SIGABRT)
//! 绕过它——jni/ndk/字体光栅化真段错误,进程直接没,panic.log 一个字
//! 不留,只能看到「没 boot 行」。本模块装 last-gasp 信号处理器:
//! 异步信号安全(只 write 到预开 fd,零分配零锁),写一行后 re-raise
//! 交还系统(内核 tombstone/logcat 照留,我们不截胡)。
//!
//! SIGURG = 测试探针:写行后**继续活**——信号路径端到端可装机判卷
//! (kill -URG $(cat na.pid) → panic.log 应多一行,进程不死)。
//! 初版探针用 SIGUSR1,装机实测被 ART 吃掉(Android 运行时认领它做
//! 堆转储/GC,libsigchain 截获后不下传用户 handler);SIGURG 无人认领
//! 且默认动作本就是忽略,天然适合当探针。

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

/// 测试探针信号(装机实证钉死:不许换成 ART 认领的 SIGUSR1/SIGQUIT)
pub const PROBE_SIG: i32 = libc::SIGURG;

/// 预开的 panic.log fd(-1 = 未装)。handler 里只许碰这个
static CRASH_FD: AtomicI32 = AtomicI32::new(-1);

/// libkfm_na.so 的首映射基址/末映射末址(装机的 /proc/self/maps 解析,
/// 0 = 未解析)。handler 里只读:PC 落在段内 → 报库内偏移,服务器
/// addr2line 直达函数——尸检从「知道死哪条街」升级到「哪个门牌号」
static SO_BASE: AtomicUsize = AtomicUsize::new(0);
static SO_END: AtomicUsize = AtomicUsize::new(0);

/// 崩溃瞬间 /proc/self/maps 转存路径(C 串+NUL,装机时格式化好)——
/// PC in=foreign 时,这张图指认凶手住在哪座库
static CRASH_MAPS_PATH: AtomicUsize = AtomicUsize::new(0);
static CRASH_MAPS_PATH_BUF: std::sync::Mutex<[u8; 128]> = std::sync::Mutex::new([0u8; 128]);

/// 十六进制写入(hex 推进 *n;format_signal_line/format_pc_line 共用)
fn push_hex(v: usize, buf: &mut [u8], n: &mut usize) {
    let mut tmp = [0u8; 16];
    let mut m = 0;
    let mut v = v;
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
        if *n < buf.len() {
            buf[*n] = tmp[m];
            *n += 1;
        }
    }
}

fn push_bytes(bytes: &[u8], buf: &mut [u8], n: &mut usize) {
    for &b in bytes {
        if *n < buf.len() {
            buf[*n] = b;
            *n += 1;
        }
    }
}

/// 坠机行格式(纯函数,钉死):`SIGNAL sig=11 addr=0xdeadbeef\n`。
/// 手写十/十六进制进固定栈缓冲——handler 里 format! 会分配,不许用
pub fn format_signal_line(sig: i32, addr: usize, buf: &mut [u8]) -> usize {
    let mut n = 0;
    push_bytes(b"SIGNAL sig=", buf, &mut n);
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
        push_bytes(&[tmp[m]], buf, &mut n);
    }
    push_bytes(b" addr=0x", buf, &mut n);
    push_hex(addr, buf, &mut n);
    push_bytes(b"\n", buf, &mut n);
    n
}

/// PC 行格式(纯函数,2026-09-08 新增):`PC pc=0x… in=libkfm_na off=0x…\n`
/// 或库外 `PC pc=0x… in=foreign\n`。base/end 全零(未解析)按 foreign 报
pub fn format_pc_line(pc: usize, base: usize, end: usize, buf: &mut [u8]) -> usize {
    let mut n = 0;
    push_bytes(b"PC pc=0x", buf, &mut n);
    push_hex(pc, buf, &mut n);
    if base != 0 && pc >= base && pc < end {
        push_bytes(b" in=libkfm_na off=0x", buf, &mut n);
        push_hex(pc - base, buf, &mut n);
    } else {
        push_bytes(b" in=foreign", buf, &mut n);
    }
    push_bytes(b"\n", buf, &mut n);
    n
}

/// 信号处理器本体:写两行(信号行+PC 行,尽力而为)→ SIGURG 探针返回
/// 继续活,其余 re-raise。PC 从 ucontext 提取——bionic aarch64 布局
/// (NDK sysroot asm-arm64/asm/{ucontext,sigcontext}.h 为准,2026-09-08
/// SIGURG 探针实证 304 读到的是 uc_sigmask/unused 区=x16 寄存器野值):
/// uc_flags(8)+uc_link(8)+uc_stack(24)+uc_sigmask(8)+__linux_unused(120)
/// =168 进 sigcontext;fault_address@168/regs[31]@176/sp@424/**pc@432**。
/// 设备钉死 aarch64,勿移植
const UCTX_PC_OFF: usize = 432;
unsafe extern "C" fn on_signal(sig: i32, info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    let addr = if info.is_null() {
        0
    } else {
        // si_addr:故障地址(SIGSEGV/SIGBUS 有,其余为 null)
        unsafe { (*info).si_addr() as usize }
    };
    let pc = if ctx.is_null() {
        0
    } else {
        unsafe { *((ctx as *const u8).add(UCTX_PC_OFF) as *const usize) }
    };
    let fd = CRASH_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let mut buf = [0u8; 256];
        let n1 = format_signal_line(sig, addr, &mut buf);
        let n2 = format_pc_line(
            pc,
            SO_BASE.load(Ordering::Relaxed),
            SO_END.load(Ordering::Relaxed),
            &mut buf[n1..],
        );
        unsafe {
            libc::write(fd, buf.as_ptr().cast(), n1 + n2);
        }
    }
    if sig == PROBE_SIG {
        return; // 测试探针:写行已证链路活,进程继续
    }
    unsafe {
        dump_maps_for_autopsy();
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// 装机(android_main 紧跟 panic 钩子之后):
/// ①预开 panic.log(append)fd 登记进 CRASH_FD——handler 里现 open 不
///   安全,只能先开好;
/// ②sigaction 挂 SIGSEGV/SIGBUS/SIGILL/SIGABRT(致命,写完 re-raise)
///   加 PROBE_SIG(测试探针,写完继续活);
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
    // libkfm_na 映射段登记(PC→库内偏移换算的尺子;解析失败留 0=foreign)
    if let Ok(maps) = std::fs::read_to_string("/proc/self/maps") {
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        for line in maps.lines() {
            if !line.contains("libkfm_na.so") {
                continue;
            }
            if let Some((rng, _)) = line.split_once(' ')
                && let Some((a, b)) = rng.split_once('-')
                && let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16))
            {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        if lo != usize::MAX && hi > lo {
            SO_BASE.store(lo, Ordering::Relaxed);
            SO_END.store(hi, Ordering::Relaxed);
        }
    }
    std::fs::write(
        std::path::PathBuf::from(dir).join("na.pid"),
        format!("{}\n", std::process::id()),
    )
    .ok();
    // crash-maps 路径预格式化成 C 串(handler 里零格式化)
    {
        let cpath = format!(
            "{}\x00",
            std::path::Path::new(dir).join("crash-maps").display()
        );
        let mut guard = CRASH_MAPS_PATH_BUF.lock().unwrap();
        let bytes = cpath.as_bytes();
        let n = bytes.len().min(127);
        guard[..n].copy_from_slice(&bytes[..n]);
        guard[n] = 0;
        CRASH_MAPS_PATH.store(guard.as_ptr() as usize, Ordering::Relaxed);
    }
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
            PROBE_SIG,
        ] {
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
    }
}

/// 崩溃瞬间转存 /proc/self/maps(last-gasp:进程将死,读写全套豁免
/// 异步信号安全洁癖;有界 512KB,零分配——路径/缓冲全是装机预埋的静态)
unsafe fn dump_maps_for_autopsy() {
    unsafe {
        let path_ptr = CRASH_MAPS_PATH.load(Ordering::Relaxed);
        if path_ptr == 0 {
            return;
        }
        let out = libc::open(
            path_ptr as *const libc::c_char,
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o644,
        );
        if out < 0 {
            return;
        }
        let maps = libc::open(c"/proc/self/maps".as_ptr(), libc::O_RDONLY);
        if maps >= 0 {
            let mut buf = [0u8; 8192];
            let mut total = 0usize;
            loop {
                let r = libc::read(maps, buf.as_mut_ptr().cast(), buf.len());
                if r <= 0 || total > 512 * 1024 {
                    break;
                }
                let mut off = 0usize;
                while off < r as usize {
                    let w = libc::write(
                        out,
                        buf.as_ptr().add(off) as *const libc::c_void,
                        r as usize - off,
                    );
                    if w <= 0 {
                        break;
                    }
                    off += w as usize;
                    total += w as usize;
                }
            }
            libc::close(maps);
        }
        libc::close(out);
    }
}
