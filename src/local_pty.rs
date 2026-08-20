//! local_pty.rs — 本地 PTY transport(L1,多端分层设计页 §3:第一次抽层)
//!
//! 设计页:`/root/kfmv4/experiments/dsh-na/na/multi-end-layering.md`(v0 送审,
//! 用户终审拍板先行,评审裁决到达后对账)。契约考题:tests/local_pty_spec.rs。
//!
//! 职责:与 ws transport 同缝(`Spawner`),把 ConnConfig 翻译成一条本地 PTY
//! 会话——Android 上 exec `/system/bin/sh`(mksh + toybox),host 上 `/bin/sh`。
//! 秒开的原理:零网络,冷进程首连 ~2.1s 唤醒成本(BAR-022/023 归因)不在这条
//! 路径上;ws 远程会话后台接,Ctrl-] 切换(android_app 双会话槽)。
//!
//! 线程模型(与 ws 驱动同构):
//! - writer 线程:收 TermCmd——Input 写 master / Resize ioctl TIOCSWINSZ /
//!   Close 杀子进程;
//! - reader 线程:阻塞读 master → SessionEvent::Output;EIO/EOF(子进程退出)
//!   → waitpid 收尸 → SessionEvent::Exited。
//!
//! fork 安全:fork 前备齐全部 CString(路径/argv/envp),fork-exec 之间只用
//! async-signal-safe 调用(setsid/ioctl/dup2/close/execve),零分配。
//!
//! 平台注记:bionic 没有 openpty(2)(libutil 遗产),nix::pty::openpty 走
//! posix_openpt/grantpt/unlockpt/ptsname——Android 与 host Linux 同一份代码。

use std::ffi::CString;
use std::io::Read;
use std::os::unix::io::{AsRawFd, FromRawFd};

use crate::conn::{ConnConfig, Spawner, TermCmd, TermHandle};
use crate::session::SessionEvent;

/// 默认 shell:Android = /system/bin/sh(mksh);host = /bin/sh(考题用)
pub fn default_shell() -> &'static str {
    if cfg!(target_os = "android") {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    }
}

/// 子进程最小环境(envp)。Android 只给系统 toolbox 路径 + 私有目录 HOME;
/// host 给常见路径(考题要跑 stty),HOME 由考题自己注入或不设。
/// TERM 与 ws 会话同款,terminfo 由对端自行解决(L2 才带本地 terminfo)。
fn child_env() -> Vec<CString> {
    if cfg!(target_os = "android") {
        [
            CString::new("PATH=/system/bin:/system/xbin").unwrap(),
            CString::new("TERM=xterm-256color").unwrap(),
            CString::new("HOME=/data/data/dev.kfm.na/files").unwrap(),
        ]
        .into()
    } else {
        [
            CString::new("PATH=/usr/bin:/bin:/usr/local/bin").unwrap(),
            CString::new("TERM=xterm-256color").unwrap(),
        ]
        .into()
    }
}

/// 本地 PTY transport:与 ws_spawner 同缝,ConnConfig.command = shell 路径
/// 覆盖(None = 平台默认),url 字段本地路径忽略。
pub fn local_pty_spawner() -> Spawner {
    std::sync::Arc::new(|cfg: ConnConfig| {
        let (event_tx, event_rx) = std::sync::mpsc::channel::<SessionEvent>();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TermCmd>();
        let shell = cfg.command.unwrap_or_else(|| default_shell().to_string());
        std::thread::spawn(move || {
            if let Err(e) = drive_local(shell, cmd_rx, event_tx.clone()) {
                let _ = event_tx.send(SessionEvent::Failed { message: e });
            }
        });
        TermHandle {
            outbound: cmd_tx,
            events: event_rx,
        }
    })
}

/// fork 序列化锁(多线程进程 fd 表继承事故,local_pty_spec 并行实证):
/// 别的线程 openpty 出的 master/slave 会被我们的 fork 子进程继承且不关,
/// 对方 shell 退出后 master 永远等不到 EIO → Exited 丢失。对策两件套:
/// ①全部 fd 立刻 FD_CLOEXEC(子进程 exec 即清场);②openpty→cloexec→fork
/// 全程串行(杀掉 openpty 与 fcntl 之间的竞态窗)。本进程一切 fork 都走这里。
static FORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 驱动主体(writer 线程):openpty → fork → 读写循环。
fn drive_local(
    shell: String,
    cmd_rx: std::sync::mpsc::Receiver<TermCmd>,
    event_tx: std::sync::mpsc::Sender<SessionEvent>,
) -> Result<(), String> {
    use nix::unistd::{ForkResult, fork};

    let _fork_guard = FORK_LOCK.lock().map_err(|_| "fork 锁被毒化")?;

    let pty = nix::pty::openpty(None, None).map_err(|e| format!("openpty 失败: {e}"))?;
    let master_raw = pty.master.as_raw_fd();
    let slave_fd = pty.slave.as_raw_fd();
    // FD_CLOEXEC:exec 时清场——别的 fork 子进程再也握不住我们的 slave
    for fd in [master_raw, slave_fd] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(format!(
                "fcntl CLOEXEC 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    // fork 前备齐 CString(fork-exec 之间零分配纪律)
    let path = CString::new(shell.as_str()).map_err(|_| format!("shell 路径含 NUL: {shell}"))?;
    let arg0 = CString::new("sh").unwrap();
    let env = child_env();
    let envp: Vec<&CString> = env.iter().collect();

    let child = match unsafe { fork() }.map_err(|e| format!("fork 失败: {e}"))? {
        ForkResult::Child => {
            // 子进程:slave 变控制终端 + 挂 0/1/2 → exec shell(只走
            // async-signal-safe 调用;任何失败 _exit 不 return——回 Rust
            // 世界会双重持有父进程状态)
            unsafe {
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
            }
            let argv = [arg0.as_ptr(), std::ptr::null()];
            let mut envp_raw: Vec<*const libc::c_char> = envp.iter().map(|c| c.as_ptr()).collect();
            envp_raw.push(std::ptr::null());
            unsafe {
                libc::execve(path.as_ptr(), argv.as_ptr(), envp_raw.as_ptr());
                libc::_exit(127); // exec 失败才到这
            }
        }
        ForkResult::Parent { child } => child,
    };
    drop(pty.slave); // 父进程关掉 slave:子退出时 master 读端拿 EOF/EIO
    drop(_fork_guard); // fork 窗口关:串行只保 openpty→cloexec→fork 一段

    // fd 所有权切分(IO Safety:一个 fd 只能有一个 owner)——reader 拿 master
    // 本体(它活到收尸后),writer dup 一份自用(也 CLOEXEC:别进未来子进程)
    let reader_file: std::fs::File = pty.master.into();
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
    let mut writer_file = unsafe { std::fs::File::from_raw_fd(writer_fd) };

    // 接通即报(本地无握手):session_id 固定 "local"
    let _ = event_tx.send(SessionEvent::Opened {
        session_id: "local".into(),
    });

    // reader 线程:master → Output;EOF/EIO → 收尸 → Exited
    let reader_tx = event_tx.clone();
    std::thread::spawn(move || {
        let mut master = reader_file;
        let mut buf = [0u8; 8192];
        loop {
            match master.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if reader_tx.send(SessionEvent::Output { data }).is_err() {
                        return; // 主循环死了:不为上报陪葬(同 ws 纪律)
                    }
                }
                Err(e) => {
                    // Linux 惯例:子退出后 slave 关闭,master 读 = EIO
                    if e.raw_os_error() != Some(libc::EIO) {
                        let _ = reader_tx.send(SessionEvent::Failed {
                            message: format!("PTY 读失败: {e}"),
                        });
                        return;
                    }
                    break;
                }
            }
        }
        let code = match nix::sys::wait::waitpid(child, None) {
            Ok(nix::sys::wait::WaitStatus::Exited(_, c)) => c,
            _ => -1, // 信号杀/收尸失败都报 -1(事件面不细分,v1)
        };
        let _ = reader_tx.send(SessionEvent::Exited { code });
    });

    // writer 循环(本线程):TermCmd → master 的 dup 副本
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            TermCmd::Input(s) => {
                use std::io::Write;
                if writer_file.write_all(s.as_bytes()).is_err() {
                    break; // 子进程已死,读写两断
                }
            }
            TermCmd::Resize { cols, rows } => {
                let ws = libc::winsize {
                    ws_row: rows as u16,
                    ws_col: cols as u16,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                unsafe {
                    libc::ioctl(writer_fd, libc::TIOCSWINSZ, &ws);
                }
            }
            TermCmd::Close => {
                let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
                break;
            }
        }
    }
    Ok(())
}

/// 本地会话工厂服务(newtype 服务键,与 ws 的 `dyn TermFactory` 键区分开——
/// 基座单一来源纪律下同键二次 provide = AlreadyProvided,双工厂并存走双键)。
/// 形状与 WsTermFactory 一致:默认配置 + transport 缝(考题注假 transport)。
pub struct LocalPtyFactory {
    default: ConnConfig,
    spawner: Spawner,
}

impl LocalPtyFactory {
    pub fn new(default: ConnConfig, spawner: Spawner) -> Self {
        LocalPtyFactory { default, spawner }
    }
}

impl crate::conn::TermFactory for LocalPtyFactory {
    fn default_config(&self) -> ConnConfig {
        self.default.clone()
    }
    fn spawn(&self, config: &ConnConfig) -> TermHandle {
        (self.spawner)(config.clone())
    }
}
