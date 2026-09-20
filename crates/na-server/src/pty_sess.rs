//! pty_sess.rs — PTY 会话壳（nix 直造，B 档胶水）
//!
//! 曾用 portable-pty：它的 serial → termios 0.2 依赖链没有 android cfg，
//! 手机端 chain 整编不过（Termux 实证 E0432/E0433，`os::target` 空）。
//! 换 nix 直造——主 crate local_pty.rs 同款路径（posix_openpt/fork/
//! waitpid），Android 与 host Linux 同一份代码，二期双端同构的前置。
//!
//! 语义对齐 kfmv4 terminal-pty.ts:35-71：
//! - shell = $SHELL，缺省 /bin/sh（kfmv4 缺省 zsh——那是桌面假设；
//!   服务器最小公分母是 sh，且 na 的远程会话历来由 command 显式给出）
//! - command 有 → `shell -c command`；无 → 交互 shell
//! - 默认 80x24，cwd = 指定 || $HOME || /，TERM=xterm-256color，env 全继承
//!
//! fork 安全（local_pty.rs 同款纪律）：fd 全 FD_CLOEXEC + openpty→cloexec→
//! fork 全程串行（FORK_LOCK）——多线程进程里别的线程开的 fd 会被 fork
//! 子进程继承不关，对方 shell 退出后 master 永远等不到 EIO；fork-exec
//! 之间只用 async-signal-safe 调用，零分配。

use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::sync::{Arc, Mutex};

pub struct PtySession {
    /// 主端本体（resize 用）；读/写各持 dup，互不影响关闭时机
    master: File,
    pub writer: File,
    /// Arc<Mutex> 共享：等死线程 try_wait 轮询与 kill 共存（短锁不互阻塞）
    pub child: Arc<Mutex<ChildProc>>,
    pub cmd_label: String,
}

impl PtySession {
    /// 终端重排：TIOCSWINSZ（portable-pty 时代 master.resize 同义）
    pub fn resize(&self, cols: u32, rows: u32) -> Result<(), String> {
        let ws = libc::winsize {
            ws_row: u16::try_from(rows).unwrap_or(24),
            ws_col: u16::try_from(cols).unwrap_or(80),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) } != 0 {
            return Err(format!(
                "TIOCSWINSZ 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

/// 子进程柄：try_wait（WNOHANG 非阻塞）与 kill 共存
pub struct ChildProc {
    pid: nix::unistd::Pid,
    reaped: bool,
}

impl ChildProc {
    /// Ok(Some(code)) 已退（信号杀/重复收尸报 -1）/ Ok(None) 还活着
    pub fn try_wait(&mut self) -> Result<Option<i32>, String> {
        use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
        if self.reaped {
            return Ok(Some(-1));
        }
        match waitpid(self.pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => Ok(None),
            Ok(WaitStatus::Exited(_, code)) => {
                self.reaped = true;
                Ok(Some(code))
            }
            Ok(_) => {
                self.reaped = true;
                Ok(Some(-1))
            }
            Err(nix::errno::Errno::ECHILD) => {
                self.reaped = true;
                Ok(Some(-1))
            }
            Err(e) => Err(format!("waitpid 失败: {e}")),
        }
    }

    pub fn kill(&mut self) -> Result<(), String> {
        nix::sys::signal::kill(self.pid, nix::sys::signal::Signal::SIGKILL)
            .map_err(|e| format!("kill 失败: {e}"))
    }
}

/// fork 序列化锁（多线程进程 fd 表继承事故，local_pty.rs 同款）
static FORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 启动一条 PTY 会话；返回（会话柄， 读取端）。
/// 读取端独立返回：读循环要交给专用线程，会话柄留给写/resize/kill。
pub fn spawn(
    cwd: Option<&str>,
    command: Option<&str>,
    cols: u32,
    rows: u32,
) -> Result<(PtySession, Box<dyn Read + Send>), String> {
    use nix::unistd::{ForkResult, fork};

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let dir = cwd
        .map(str::to_string)
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| "/".into());

    let _fork_guard = FORK_LOCK.lock().map_err(|_| "fork 锁被毒化")?;

    let pty = nix::pty::openpty(None, None).map_err(|e| format!("openpty 失败: {e}"))?;
    let master_raw = pty.master.as_raw_fd();
    let slave_raw = pty.slave.as_raw_fd();
    // FD_CLOEXEC：exec 时清场——别的 fork 子进程再也握不住我们的 slave
    for fd in [master_raw, slave_raw] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(format!(
                "fcntl CLOEXEC 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    // 初始窗口尺寸
    {
        let ws = libc::winsize {
            ws_row: u16::try_from(rows).unwrap_or(24),
            ws_col: u16::try_from(cols).unwrap_or(80),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(master_raw, libc::TIOCSWINSZ, &ws);
        }
    }

    // fork 前备齐全部 CString（fork-exec 之间零分配纪律）
    let (argv, cmd_label) = match command {
        Some(c) => (
            vec![shell.clone(), "-c".into(), c.to_string()],
            c.to_string(),
        ),
        None => (vec![shell.clone()], shell.clone()),
    };
    let path = CString::new(shell.as_str()).map_err(|_| "shell 路径含 NUL")?;
    let argv_c: Vec<CString> = argv
        .iter()
        .map(|a| CString::new(a.as_str()).expect("argv 不含 NUL"))
        .collect();
    let dir_c = CString::new(dir.as_str()).map_err(|_| "cwd 含 NUL")?;
    // env 全继承，TERM 覆盖（kfmv4 同款 xterm-256color）
    let mut env: Vec<CString> = std::env::vars()
        .filter(|(k, _)| k != "TERM")
        .map(|(k, v)| CString::new(format!("{k}={v}")).expect("env 不含 NUL"))
        .collect();
    env.push(CString::new("TERM=xterm-256color").unwrap());

    let child = match unsafe { fork() }.map_err(|e| format!("fork 失败: {e}"))? {
        ForkResult::Child => {
            // 子进程：slave 变控制终端 + 挂 0/1/2 → exec（只走
            // async-signal-safe 调用；失败 _exit 不 return）
            unsafe {
                libc::setsid();
                libc::ioctl(slave_raw, libc::TIOCSCTTY, 0);
                libc::dup2(slave_raw, 0);
                libc::dup2(slave_raw, 1);
                libc::dup2(slave_raw, 2);
                libc::chdir(dir_c.as_ptr());
            }
            let mut argv_raw: Vec<*const libc::c_char> =
                argv_c.iter().map(|c| c.as_ptr()).collect();
            argv_raw.push(std::ptr::null());
            let mut envp_raw: Vec<*const libc::c_char> = env.iter().map(|c| c.as_ptr()).collect();
            envp_raw.push(std::ptr::null());
            unsafe {
                libc::execve(path.as_ptr(), argv_raw.as_ptr(), envp_raw.as_ptr());
                libc::_exit(127); // exec 失败才到这
            }
        }
        ForkResult::Parent { child } => child,
    };
    drop(pty.slave); // 父进程关掉 slave：子退出时 master 读端拿 EOF/EIO
    drop(_fork_guard); // 串行只保 openpty→cloexec→fork 一段

    // fd 所有权切分：master 本体留会话（resize），reader/writer 各持 dup
    let master_file: File = pty.master.into();
    let reader_file = master_file
        .try_clone()
        .map_err(|e| format!("clone reader 失败: {e}"))?;
    let writer_fd = unsafe { libc::dup(master_raw) };
    if writer_fd < 0 {
        return Err(format!(
            "dup master 失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    unsafe {
        libc::fcntl(writer_fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let writer_file = unsafe { File::from_raw_fd(writer_fd) };

    Ok((
        PtySession {
            master: master_file,
            writer: writer_file,
            child: Arc::new(Mutex::new(ChildProc {
                pid: child,
                reaped: false,
            })),
            cmd_label,
        },
        Box::new(reader_file),
    ))
}
