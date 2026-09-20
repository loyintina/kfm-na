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
//! 短命 exec(local_exec):解析页本地相的执行腿(两轴契约第 6 步)——
//! 与 tmux_exec::exec 同一契约面,`sh -c` 跑命令收输出,不进会话体系。
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

/// Android 私有 prefix(bootstrap 安装目标;与 pick_home 同款硬编码,
/// JNI 推导在 bootstrap 壳侧——PTY 线程拿不到 AndroidApp 句柄,v1 从简)
#[cfg(target_os = "android")]
pub fn android_prefix() -> std::path::PathBuf {
    std::path::PathBuf::from("/data/data/dev.kfm.na/files/usr")
}

/// L3 挂勾(设计页 l3-bootstrap.md §5):bootstrap 装好的 prefix 在 →
/// shell 换 $PREFIX/bin/bash,env 补 PATH/LD_LIBRARY_PATH/PREFIX;
/// 不在 → 回落平台默认(env_extra 空,行为与 L3 前逐字节一致)
pub struct ShellPlan {
    pub shell: String,
    pub arg0: CString,
    pub env_extra: Vec<String>,
    /// 额外 argv(arg0 之后;空 = 交互 shell)。`-c 命令行` 走这里——
    /// ConnConfig.command 的命令行语义(ws 侧 = 服务端 sh -c,本地侧
    /// 对齐同一契约:platform shell -c)
    pub args: Vec<CString>,
}

pub fn shell_plan(prefix: &std::path::Path) -> ShellPlan {
    let bash = prefix.join("bin/bash");
    if bash.is_file() {
        ShellPlan {
            shell: bash.to_string_lossy().into_owned(),
            arg0: CString::new("bash").unwrap(),
            env_extra: vec![
                format!("PATH={}/bin:/system/bin:/system/xbin", prefix.display()),
                format!("LD_LIBRARY_PATH={}/lib", prefix.display()),
                format!("PREFIX={}", prefix.display()),
            ],
            args: vec![],
        }
    } else {
        ShellPlan {
            shell: default_shell().to_string(),
            arg0: CString::new("sh").unwrap(),
            env_extra: vec![],
            args: vec![],
        }
    }
}

/// 子进程最小环境(envp)。默认:Android 只给系统 toolbox 路径 + HOME(见
/// pick_home);host 给常见路径(考题要跑 stty),HOME 不设(考题不碰)。
/// env_extra 非空(L3 bash 方案)= 环境方案自带 PATH,平台默认 PATH 让位。
/// TERM 与 ws 会话同款,terminfo 由对端自行解决(L2 才带本地 terminfo)。
fn child_env(home: Option<&CString>, env_extra: &[String]) -> Vec<CString> {
    let mut env: Vec<CString> = if env_extra.is_empty() {
        if cfg!(target_os = "android") {
            [CString::new("PATH=/system/bin:/system/xbin").unwrap()].into()
        } else {
            [CString::new("PATH=/usr/bin:/bin:/usr/local/bin").unwrap()].into()
        }
    } else {
        env_extra
            .iter()
            .map(|e| CString::new(e.as_str()).unwrap())
            .collect()
    };
    env.push(CString::new("TERM=xterm-256color").unwrap());
    if let Some(h) = home {
        let mut line = b"HOME=".to_vec();
        line.extend_from_slice(h.as_bytes());
        env.push(CString::new(line).unwrap());
    }
    env
}

/// 本地 HOME 选址(2026-08-20 实拍「ls / 全墙」后的修补):共享存储的应用
/// 专属目录优先——免权限读写、文件管理器可见(用户要的「能看到的目录」);
/// 建不起来退化私有目录。返回 None = host(不设 HOME 不改 cwd)
fn pick_home() -> Option<CString> {
    if !cfg!(target_os = "android") {
        return None;
    }
    for cand in [
        "/storage/emulated/0/Android/data/dev.kfm.na/files",
        "/data/data/dev.kfm.na/files",
    ] {
        if std::fs::create_dir_all(cand).is_ok() {
            return CString::new(cand).ok();
        }
    }
    None
}

/// 本地 PTY transport:与 ws_spawner 同缝,ConnConfig.command = 命令行
/// (None = 交互 shell;Some = 平台 shell 方案 `-c` 跑命令行——与 ws 侧
/// 「服务端 sh -c」同一契约,2026-09-20 对齐:旧义「shell 路径覆盖」
/// 无调用方,且与 conn.rs 字段文档「开什么命令」相悖),url 字段本地
/// 路径忽略。
pub fn local_pty_spawner() -> Spawner {
    std::sync::Arc::new(|cfg: ConnConfig| {
        let (event_tx, event_rx) = std::sync::mpsc::channel::<SessionEvent>();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TermCmd>();
        let mut plan = shell_plan_for_platform();
        if let Some(cmdline) = cfg.command {
            // 命令行语义:环境照旧(L3 bash + PREFIX/PATH——tmux attach
            // 这类命令要解析得到 $PREFIX/bin),argv 补 -c 命令行
            plan.args = vec![
                CString::new("-c").unwrap(),
                CString::new(cmdline.as_str()).unwrap_or_else(|_| {
                    crate::report::report("term", "本地命令行含 NUL,按空命令处理");
                    CString::new("").unwrap()
                }),
            ];
        }
        std::thread::spawn(move || {
            if let Err(e) = drive_local(plan, cmd_rx, event_tx.clone()) {
                let _ = event_tx.send(SessionEvent::Failed { message: e });
            }
        });
        TermHandle {
            outbound: cmd_tx,
            events: event_rx,
        }
    })
}

/// 平台默认 shell 方案:Android 看 L3 prefix,host 直接默认(考题不碰)
fn shell_plan_for_platform() -> ShellPlan {
    #[cfg(target_os = "android")]
    {
        shell_plan(&android_prefix())
    }
    #[cfg(not(target_os = "android"))]
    {
        shell_plan(std::path::Path::new("/nonexistent"))
    }
}

/// fork 序列化锁(多线程进程 fd 表继承事故,local_pty_spec 并行实证):
/// 别的线程 openpty 出的 master/slave 会被我们的 fork 子进程继承且不关,
/// 对方 shell 退出后 master 永远等不到 EIO → Exited 丢失。对策两件套:
/// ①全部 fd 立刻 FD_CLOEXEC(子进程 exec 即清场);②openpty→cloexec→fork
/// 全程串行(杀掉 openpty 与 fcntl 之间的竞态窗)。本进程一切 fork 都走这里。
static FORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 驱动主体(writer 线程):openpty → fork → 读写循环。
fn drive_local(
    plan: ShellPlan,
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
    let path = CString::new(plan.shell.as_str())
        .map_err(|_| format!("shell 路径含 NUL: {}", plan.shell))?;
    let arg0 = plan.arg0;
    let args = plan.args;
    let home = pick_home(); // 含 create_dir_all(母进程侧,fork 前)
    if let Some(h) = &home {
        crate::report::report("term", &format!("本地 HOME = {}", h.to_string_lossy()));
    }
    let env = child_env(home.as_ref(), &plan.env_extra);
    let envp: Vec<&CString> = env.iter().collect();
    // argv = [arg0, args..., null](命令行语义时 args = [-c, 命令])
    let argv: Vec<*const libc::c_char> = std::iter::once(arg0.as_ptr())
        .chain(args.iter().map(|a| a.as_ptr()))
        .chain(std::iter::once(std::ptr::null()))
        .collect();

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
                // 落进自己家(async-signal-safe 名单内含 chdir);失败留 / 也能活
                if let Some(h) = &home {
                    libc::chdir(h.as_ptr());
                }
            }
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

/// 执行超时(与 tmux_exec 同款 10s:tmux 命令亚秒级,超时 = 病态兜底;
/// 文案同模——同一条契约面,报错措辞不随对象轴漂移)
const EXEC_TIMEOUT_S: u64 = 10;

/// 短命本地 PTY exec(解析页两轴契约第 6 步:本地相 exec 腿)——与
/// tmux_exec::exec 同一契约面:起一条短命本地 PTY 跑 `<shell> -c command`,
/// 收全部输出(stdout/stderr 同 PTY 合并,ws 路同形)直到子进程退出,
/// 结果进返回的 Receiver。命令串与 ws 路同款直用(tmux_ctl 尾带
/// `; exit`,`sh -c` 下无害)。环境 = 交互本地会同一份
/// (shell_plan_for_platform:L3 bash + PATH/LD_LIBRARY_PATH/PREFIX;
/// 无 L3 回落平台 sh)——$PREFIX/bin 的 tmux 解析与交互会话零漂移
/// (2026-09-20 真机探针:L3 装 tmux 后 new/ls/kill 全绿)。
pub fn local_exec(command: String) -> std::sync::mpsc::Receiver<Result<String, String>> {
    local_exec_with(command, EXEC_TIMEOUT_S)
}

/// 超时可注版(考题缝——10s 的考卷等不起;语义与 local_exec 全同)
pub fn local_exec_with(
    command: String,
    timeout_s: u64,
) -> std::sync::mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // 主循环死了发送失败:吞掉——执行线程绝不为上报陪葬(ws 线程同规)
        let _ = tx.send(run_exec(command, timeout_s));
    });
    rx
}

/// exec 驱动主体(执行线程):openpty → fork(shell -c) → poll 读环。
/// fork 安全纪律与 drive_local 同一份(串行锁 + CLOEXEC + 子进程只走
/// async-signal-safe);不同处:无 TermCmd 通道、无 SessionEvent——结果
/// 只有一次 send(Ok=全部输出/Err=启动失败或超时),超时杀子收尸。
fn run_exec(command: String, timeout_s: u64) -> Result<String, String> {
    use nix::unistd::{ForkResult, fork};

    let _fork_guard = FORK_LOCK.lock().map_err(|_| "fork 锁被毒化")?;
    let pty = nix::pty::openpty(None, None).map_err(|e| format!("openpty 失败: {e}"))?;
    let master_raw = pty.master.as_raw_fd();
    let slave_fd = pty.slave.as_raw_fd();
    for fd in [master_raw, slave_fd] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(format!(
                "fcntl CLOEXEC 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    // fork 前备齐 CString(零分配纪律同 drive_local);argv = [sh, -c, 命令]
    let plan = shell_plan_for_platform();
    let path = CString::new(plan.shell.as_str())
        .map_err(|_| format!("shell 路径含 NUL: {}", plan.shell))?;
    let arg0 = plan.arg0;
    let arg_dash_c = CString::new("-c").unwrap();
    let arg_cmd = CString::new(command.as_str()).map_err(|_| "命令串含 NUL")?;
    let home = pick_home(); // 含 create_dir_all(母进程侧,fork 前)
    let env = child_env(home.as_ref(), &plan.env_extra);
    let envp: Vec<&CString> = env.iter().collect();

    let child = match unsafe { fork() }.map_err(|e| format!("fork 失败: {e}"))? {
        ForkResult::Child => {
            unsafe {
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                if let Some(h) = &home {
                    libc::chdir(h.as_ptr());
                }
            }
            let argv = [
                arg0.as_ptr(),
                arg_dash_c.as_ptr(),
                arg_cmd.as_ptr(),
                std::ptr::null(),
            ];
            let mut envp_raw: Vec<*const libc::c_char> = envp.iter().map(|c| c.as_ptr()).collect();
            envp_raw.push(std::ptr::null());
            unsafe {
                libc::execve(path.as_ptr(), argv.as_ptr(), envp_raw.as_ptr());
                libc::_exit(127); // exec 失败才到这
            }
        }
        ForkResult::Parent { child } => child,
    };
    drop(pty.slave); // 父进程关 slave:子退出时 master 读端拿 EOF/EIO
    drop(_fork_guard);

    // 读环:poll 截拍(250ms) + 截止钟——裸阻塞 read 会把超时咬死
    // (子进程挂死时读环永远等不到 EIO)
    let mut file: std::fs::File = pty.master.into();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_s);
    let mut out = String::new();
    let mut buf = [0u8; 8192];
    let mut timed_out = false;
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            timed_out = true;
            break;
        }
        let remain = ((deadline - now).as_millis() as libc::c_int).clamp(1, 250);
        let mut pfd = libc::pollfd {
            fd: master_raw,
            events: libc::POLLIN,
            revents: 0,
        };
        let n = unsafe { libc::poll(&mut pfd, 1, remain) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
            let _ = nix::sys::wait::waitpid(child, None);
            return Err(format!("poll 失败: {err}"));
        }
        if n == 0 {
            continue; // 拍点到:回环看钟
        }
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.push_str(&String::from_utf8_lossy(&buf[..n])),
            // Linux 惯例:子退出后 slave 关闭,master 读 = EIO
            Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
            Err(e) => {
                let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
                let _ = nix::sys::wait::waitpid(child, None);
                return Err(format!("PTY 读失败: {e}"));
            }
        }
    }
    if timed_out {
        let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
    }
    // 收尸(两路汇合:正常读完 = 子已退出,waitpid 即返;超时 = 杀后收)
    let _ = nix::sys::wait::waitpid(child, None);
    if timed_out {
        Err(format!("执行超时（{timeout_s}s）"))
    } else {
        Ok(out)
    }
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
