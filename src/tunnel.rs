//! tunnel.rs — L3 内置 ssh 正连隧道（2026-09-19 用户拍板，取代 russh 提案）
//!
//! 病灶：na 运行时数据通道（ws://127.0.0.1:9021）靠 Termux 的 ssh -L 吊着
//! ——Termux 被安卓省电杀、单连接扛所有流量，通道命根子不在自己手里。
//! 正身：na 用 L3 prefix 里的 openssh 自己 spawn `ssh -N -L` 后台进程 +
//! 看门狗（死了退避重拉）——打开 na 就能通，隧道归前台 app 持有。
//! 全链路已实证（2026-09-19）：沙箱 ssh 二进制 + 已部署密钥 → 服务器
//! sshd → kfmv4 通。
//!
//! 分层：上半 A 档纯逻辑（参数构造/退避表，tests/tunnel_spec.rs 钉死）；
//! 下半 B 档进程胶水（spawn/探活/看门狗单线程 1s 滴答）。
//!
//! 让位/接管语义：本地口已被占且能连通（Termux 隧道还在）→ 不抢，
//! ExternalUp 挂着 30s 复查；外部隧道一断，下一拍自持接管——用户
//! 停掉 Termux 隧道即无缝切换到 na 自持。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};

use crate::settings::{Backend, ServerEntry};

/// 转发目标口双锚（2026-09-20 na-server 立项）：kfmv4 ws 口 8021 /
/// na-server 口 9021。漂移 = 隧道通了但 ws 全断，考题钉死
pub const KFMV4_PORT: u16 = 8021;
pub const NA_SERVER_PORT: u16 = 9021;

/// 转发目标口 = f(后端)（A 档纯函数）：Kfmv4 → 8021（现状锚）；
/// NaServer → 9021（na-server 只绑回环，双端同口，na-server.md §二）
pub fn target_port(b: &Backend) -> u16 {
    match b {
        Backend::Kfmv4 => KFMV4_PORT,
        Backend::NaServer => NA_SERVER_PORT,
    }
}

/// ssh 缺件检查（A 档纯函数）：forward_args 与 na_server_sup::exec_args
/// 同一把尺——缺件判定写两处必漂移
pub fn check_ssh_fields(s: &ServerEntry) -> Result<(), String> {
    if s.ssh.host.is_empty() {
        return Err("ssh.host 空".into());
    }
    if s.ssh.user.is_empty() {
        return Err("ssh.user 空".into());
    }
    if s.ssh.key_path.is_empty() {
        return Err("ssh.keyPath 空".into());
    }
    if !s.ssh.password.is_empty() {
        return Err("v1 只走密钥登录（密码在 BatchMode 下必悬问假死）".into());
    }
    Ok(())
}

/// NA 内置 sshd 的手机本地监听口（与 files/usr/etc/ssh/kfm-sshd.conf
/// 的 `Port 8024 ListenAddress 127.0.0.1` 单一源——改口两边一起改）。
/// -R 反连的目的端：服务器 {remote_port} → 手机本口，直达 NA sshd
pub const NA_SSHD_PORT: u16 = 8024;

/// ssh 转发参数（A 档纯函数）。v1 只走密钥：密码登录在 BatchMode 下
/// 必悬问假死（无 askpass），显式拒；缺件（host/user/key 空）同拒。
/// -L 正连数据路 + -R 反连调试/推送路（2026-09-21 用户拍板「9022 我们
/// 自己的推送路径」：v1 -R 归 Termux 维护，Termux 休眠冻结 = 推送全瘫
/// 一整天实录；并进 na 自持隧道后看门狗双腿（BAR-117）维护——na 活着
/// 反连就在）。-R 显式绑 127.0.0.1：不新增公网暴露面（2026-09-01 红线）
pub fn forward_args(s: &ServerEntry) -> Result<Vec<String>, String> {
    check_ssh_fields(s)?;
    Ok(vec![
        "-N".into(),
        "-L".into(),
        format!(
            "{}:127.0.0.1:{}",
            s.tunnel.local_port,
            target_port(&s.backend)
        ),
        "-R".into(),
        format!(
            "127.0.0.1:{}:127.0.0.1:{}",
            s.tunnel.remote_port, NA_SSHD_PORT
        ),
        "-i".into(),
        s.ssh.key_path.clone(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-p".into(),
        s.ssh.port.to_string(),
        format!("{}@{}", s.ssh.user, s.ssh.host),
    ])
}

/// 看门狗退避表（A 档）：首死立即重拉（用户在场等不得），2/5/10s 渐进，
/// 30s 封顶——不指数爆炸，病态网络下每分钟至少敲一次门
pub fn backoff_secs(attempt: u32) -> u64 {
    match attempt {
        0 => 0,
        1 => 2,
        2 => 5,
        3 => 10,
        _ => 30,
    }
}

/// 稳定窗口（秒）：娃活过这么久才算「一次真连接」——短于此 = 抖动
pub const STABLE_SECS: u64 = 30;

/// 死娃后的重试计数裁决（A 档纯函数，2026-09-21 「反复连接反复断开」立案）：
/// 娃活得久（≥ STABLE_SECS）才算一次真连接 → 计数回 1（下次首死立即重拉，
/// 用户在场等不得）；**短命娃（含一 spawn 即死：撞口/拒连/认证失败）计数
/// 续涨** → 退避爬到 5/10/30s。原先「一 Up 就清零」在抖动网络下变成每 2s
/// 重拉一次 ssh 的热循环：手机无线电 + 服务器 sshd 一起挨打，而热循环
/// 本身又会诱发下一轮 255（现场实录：13:20-13:22 三分钟里 8 次 spawn/退出）
pub fn next_attempts(prev: u32, lived_secs: u64) -> u32 {
    if lived_secs >= STABLE_SECS {
        1
    } else {
        prev.saturating_add(1).max(1)
    }
}

/// 反连口释放脚本（A 档纯函数，2026-09-21 BAR-129 自愈）：**口即设备身份**
/// ——每设备 10 口段（手机 9022 / redroid 9122），所以「谁占着本设备口」
/// 必然是我们自己上一轮遗留的会话（手机退后台/灭屏/杀进程时，服务端 sshd
/// 要等 ClientAlive 90s 才收割，这段窗口里新 ssh 必撞口 → ExitOnForwardFailure
/// → 秒级 255 → 用户看到的「恢复后反复掉」）。故释放 = 把占口的那个
/// **sshd** 收掉：非 sshd 进程一律不动（防误伤真服务）
pub fn release_forward_script(remote_port: u16) -> String {
    format!(
        "P=$(ss -tlnpH \"sport = :{remote_port}\" 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u)
for p in $P; do
  if ps -p \"$p\" -o comm= 2>/dev/null | grep -q '^sshd'; then kill \"$p\" 2>/dev/null && echo \"released=$p\"; fi
done
[ -z \"$P\" ] && echo none"
    )
}

/// 反连口被占的判决（A 档纯函数）：ssh 死因里同时出现「远程转发绑定失败」
/// 与**本设备口号**才触发释放——认证失败/拒连/keepalive 超时都不许触发，
/// 否则可能把一条**活的**会话杀掉（口不匹配同理：别的设备的口不归我们管）
pub fn should_release_forward(stderr_tail: &str, remote_port: u16) -> bool {
    stderr_tail.contains("remote port forwarding failed")
        && stderr_tail.contains(&remote_port.to_string())
}

/// 状态词（A 档纯函数）：连接/服务卡状态行的唯一文案源。
/// Down{attempts:0} = 从没起来过（缺件/prefix 未装同相）→「未启动」；
/// Down{n>0} = 退避中，必须带次数（用户要知道还在敲第几次门）。
pub fn state_word(st: &TunnelState) -> String {
    match st {
        TunnelState::Up => "自持在线".into(),
        TunnelState::ExternalUp => "外部借用".into(),
        TunnelState::Starting => "连接中".into(),
        TunnelState::Down { attempts, .. } if *attempts == 0 => "未启动".into(),
        TunnelState::Down { attempts, .. } => format!("退避 ×{attempts}"),
    }
}

/// 传输可用相（A 档纯函数）：Up/ExternalUp 都是「本地口能走」——壳层
/// 会话只管 127.0.0.1:9021 通不通，不问是谁供的口。
pub fn usable(st: &TunnelState) -> bool {
    matches!(st, TunnelState::Up | TunnelState::ExternalUp)
}

/// 隧道可用沿踢壳层重孵的裁决（A 档纯函数，BAR-117）：上一拍不可用 →
/// 本拍可用 且 活跃会话死了 → 踢一脚。稳定在线（可用→可用）不踢
/// （每圈踢 = 重孵风暴）；会话活着不踢；可用→Down 不踢。
/// 病灶：重孵链是死亡事件驱动的，末次重孵撞 TCP refused（隧道未起）
/// 被 5s 时间闸压住后再无死亡事件 = 链断，隧道 Up 不回头踢壳层，
/// remote_dead 卡死到用户敲键。本函数是「隧道→壳层」的唯一联动门。
pub fn usable_edge_kick(prev_usable: bool, curr: &TunnelState, session_over: bool) -> bool {
    !prev_usable && usable(curr) && session_over
}

// ---- 数据面（UI 只读/按钮只写这两道门，绝不许碰锁内活物）----

/// 全局快照门：supervisor 启动时登记，插件卡经 snap() 读。
/// 拍板（2026-09-20）：避免穿 App plumbing，UI 直读全局。
static TUNNEL_SNAP: OnceLock<Arc<Mutex<TunnelSnap>>> = OnceLock::new();

/// 读隧道快照（没起看门狗 = None——L3 未装/无服务器条目同相）
pub fn snap() -> Option<Arc<Mutex<TunnelSnap>>> {
    TUNNEL_SNAP.get().cloned()
}

/// 控制命令（v1 只有重连：杀娃重置退避，下一拍立即重拉）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelCmd {
    Reconnect,
}

static TUNNEL_CMD: OnceLock<Sender<TunnelCmd>> = OnceLock::new();

/// 插件卡 [重连] 按钮的唯一入口；看门狗不在 = false（按钮侧可提示）
pub fn request_reconnect() -> bool {
    TUNNEL_CMD
        .get()
        .map(|tx| tx.send(TunnelCmd::Reconnect).is_ok())
        .unwrap_or(false)
}

// ---- B 档：进程胶水（spawn/探活/看门狗），判卷 = 真机实拍 + report 行 ----

/// 隧道状态（连接/服务插件卡与断线状态卡的唯一数据源）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    /// ssh 进程在，本地口探活未过
    Starting,
    /// 自持隧道在线（我们 spawn 的 ssh 供出本地口）
    Up,
    /// 借用外部隧道（本地口被占且可连通——Termux ssh -L 让位前的过渡态）
    ExternalUp,
    /// 死了/起不来，attempts 次失败后退避重拉中
    Down { attempts: u32, last_error: String },
}

/// 对外快照（UI 读这份，绝不许碰锁内活物）
#[derive(Debug, Clone)]
pub struct TunnelSnap {
    pub state: TunnelState,
    pub local_port: u16,
    /// user@host:port（卡上显示用）
    pub target: String,
    /// 代际戳：状态每变一次 +1——涂装 sig 的唯一代际源（漏维 = 鬼影，
    /// 解析槽烘焙 sig 带本维才在状态翻转时重烘）
    pub epoch: u64,
}

/// 本地口 TCP 探活（绑定在 = 转发通道在；端到端 ws 握手探活归插件卡阶段）
fn probe_port(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(300),
    )
    .is_ok()
}

/// 重拉等待（A 档纯函数，2026-09-22 BAR-132）：**释放成功 ≈ 口已腾** →
/// 只等 4s（让释放的 ssh 落地）再试，不背退避账——否则「上一轮残留占口」
/// 这种一秒就能解的病因，会被爬到 30s 的退避白白拖慢（实测：05:47 那次
/// 从断到恢复花了 29s，其中 30s 退避白等）。别的死因（网络/冻结）照退避
/// 表爬，防抖语义不变
pub fn retry_wait(attempts: u32, releasing: bool) -> u64 {
    if releasing { 4 } else { backoff_secs(attempts) }
}

/// 反连口释放（B 档胶水）：一次性 ssh 跑释放脚本，独立线程跑（看门狗
/// 1s 滴答不许被 ssh 握手拖住）。释放是快活，10s 超时即弃（下一轮再试）
fn kick_release_forward(prefix: &Path, server: &ServerEntry) {
    let prefix = prefix.to_path_buf();
    let server = server.clone();
    std::thread::spawn(move || {
        let Ok(args) = crate::na_server_sup::exec_args(&server) else {
            crate::report::report("tunnel", "反连口释放：ssh 参数缺件，放弃");
            return;
        };
        let mut cmd = std::process::Command::new(prefix.join("bin/ssh"));
        let child = cmd
            .args(&args)
            .env("PATH", prefix.join("bin"))
            .env("LD_LIBRARY_PATH", prefix.join("lib"))
            .env("PREFIX", &prefix)
            .env("TERMUX__PREFIX", &prefix)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            crate::report::report("tunnel", "反连口释放：ssh spawn 失败");
            return;
        };
        if let Some(mut sin) = child.stdin.take() {
            use std::io::Write as _;
            let _ = sin.write_all(release_forward_script(server.tunnel.remote_port).as_bytes());
            drop(sin); // 关 stdin = 脚本开跑
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(out)) => {
                let o = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
                crate::report::report(
                    "tunnel",
                    &format!(
                        "反连口释放：{}{}",
                        if o.is_empty() {
                            "已回收/无占用".to_string()
                        } else {
                            o
                        },
                        if e.is_empty() {
                            String::new()
                        } else {
                            format!("（err={e}）")
                        }
                    ),
                );
            }
            _ => crate::report::report("tunnel", "反连口释放：超时，下一轮再试"),
        }
    });
}

/// 起看门狗（一线程 1s 滴答）：探口 → 外部占则让位挂 ExternalUp；
/// 无娃则 spawn（Starting）；探活过且娃在 → Up；娃死 → Down 退避重拉。
/// 返回共享快照——UI/报表只读这份。**幂等**：重复调用（设置重载）返回
/// 已在跑的那份，不起第二条看门狗（两条抢一个口 = 互杀）。
pub fn start(prefix: PathBuf, server: ServerEntry) -> Arc<Mutex<TunnelSnap>> {
    if let Some(existing) = TUNNEL_SNAP.get() {
        crate::report::report("tunnel", "看门狗已在跑，忽略重复启动");
        return Arc::clone(existing);
    }
    let snap = Arc::new(Mutex::new(TunnelSnap {
        state: TunnelState::Down {
            attempts: 0,
            last_error: "未启动".into(),
        },
        local_port: server.tunnel.local_port,
        target: format!(
            "{}@{}:{}",
            server.ssh.user, server.ssh.host, server.ssh.port
        ),
        epoch: 0,
    }));
    TUNNEL_SNAP.set(Arc::clone(&snap)).ok();
    let Ok(args) = forward_args(&server) else {
        // 缺件：快照落字——连接/服务卡直接显示原因，不只靠报表
        let mut g = snap.lock().unwrap();
        g.state = TunnelState::Down {
            attempts: 0,
            last_error: "ssh 配置缺件".into(),
        };
        g.target = "—".into(); // 缺件时 user/host 是空串，"@:22" 上卡徒增噪音
        g.epoch += 1;
        drop(g);
        crate::report::report("tunnel", "ssh 配置缺件，隧道不启动（设置页补齐）");
        return snap;
    };
    crate::report::report(
        "tunnel",
        &format!(
            "起自持隧道看门狗：{}@{}:{} → 127.0.0.1:{}",
            server.ssh.user, server.ssh.host, server.ssh.port, server.tunnel.local_port
        ),
    );
    let (cmd_tx, cmd_rx) = channel::<TunnelCmd>();
    TUNNEL_CMD.set(cmd_tx).ok();
    let ssh_bin = prefix.join("bin/ssh");
    let snap_t = Arc::clone(&snap);
    std::thread::spawn(move || {
        let mut child: Option<std::process::Child> = None;
        let mut attempts: u32 = 0;
        let set = |st: TunnelState, snap_t: &Arc<Mutex<TunnelSnap>>| {
            let mut g = snap_t.lock().unwrap();
            if g.state != st {
                crate::report::report("tunnel", &format!("状态 {:?} → {:?}", g.state, st));
                g.state = st;
                g.epoch += 1;
            }
        };
        // 等一拍（可被取消）：命令到了 = true。重连语义 = 杀娃（有的话）+
        // 退避清零，下一拍立刻重新探口/spawn——不等退避不等 30s 复查
        let wait = |rx: &Receiver<TunnelCmd>, dur: std::time::Duration| -> bool {
            match rx.recv_timeout(dur) {
                Ok(TunnelCmd::Reconnect) => true,
                Err(_) => false,
            }
        };
        let reconnect = |child: &mut Option<std::process::Child>,
                         attempts: &mut u32,
                         snap_t: &Arc<Mutex<TunnelSnap>>| {
            crate::report::report("tunnel", "手动重连：杀娃重拉");
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait(); // 收尸——不留僵尸，口随进程 teardown 立刻放
            }
            *attempts = 0;
            set(
                TunnelState::Down {
                    attempts: 0,
                    last_error: "手动重连".into(),
                },
                snap_t,
            );
        };
        // 抖动诊断两笔账（看门狗线程私有）：spawned_at = 本次 ssh 起于何时
        // （活多久 = 真连接 vs 抖动）、ssh_err = 它的 stderr 尾环
        let mut spawned_at: Option<std::time::Instant> = None;
        let mut ssh_err: Option<Arc<Mutex<VecDeque<String>>>> = None;
        loop {
            let port = server.tunnel.local_port;
            let port_open = probe_port(port);
            let child_alive = child
                .as_mut()
                .map(|c| c.try_wait().ok().flatten().is_none())
                .unwrap_or(false);

            if port_open && !child_alive {
                // 外部隧道占着口——让位不抢，30s 一拍复查（它一断下一拍接管）
                set(TunnelState::ExternalUp, &snap_t);
                if wait(&cmd_rx, std::time::Duration::from_secs(30)) {
                    reconnect(&mut child, &mut attempts, &snap_t);
                }
                continue;
            }

            if child_alive {
                set(
                    if port_open {
                        TunnelState::Up
                    } else {
                        TunnelState::Starting
                    },
                    &snap_t,
                );
                if wait(&cmd_rx, std::time::Duration::from_secs(1)) {
                    reconnect(&mut child, &mut attempts, &snap_t);
                }
                continue;
            }

            // 娃不在（从没起/死了）：退了重来
            if let Some(mut c) = child.take() {
                let code = c.try_wait().ok().flatten();
                let lived = spawned_at
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(u64::MAX);
                spawned_at = None;
                attempts = next_attempts(attempts, lived);
                // 真因随报表落一行（stderr 尾——诊断「为什么断」的唯一证据）
                let tail = ssh_err
                    .as_ref()
                    .and_then(|r| {
                        r.lock()
                            .ok()
                            .map(|g| g.iter().cloned().collect::<Vec<_>>().join(" | "))
                    })
                    .unwrap_or_default();
                crate::report::report(
                    "tunnel",
                    &format!(
                        "ssh 进程退出（{code:?}，活 {lived}s），第 {attempts} 次退避重拉{}",
                        if tail.is_empty() {
                            String::new()
                        } else {
                            format!("；stderr: {tail}")
                        }
                    ),
                );
                ssh_err = None;
                // 反连口自愈（BAR-129）：撞口是「上一轮遗留会话还在
                // ClientAlive 收割窗里」的确定后果——不等 90s，直接请
                // 服务器把占本设备口的 sshd 收掉，下一轮即可绑上
                let releasing = if should_release_forward(&tail, server.tunnel.remote_port) {
                    crate::report::report(
                        "tunnel",
                        &format!(
                            "反连口 {} 被上一轮占着 → 请求服务器释放",
                            server.tunnel.remote_port
                        ),
                    );
                    kick_release_forward(&prefix, &server);
                    true
                } else {
                    false
                };
                let wait_s = retry_wait(attempts, releasing);
                if releasing {
                    attempts = 1; // 口腾了 = 从头来（下一轮 2s 级）
                }
                set(
                    TunnelState::Down {
                        attempts,
                        last_error: format!("ssh 退出 {code:?}"),
                    },
                    &snap_t,
                );
                if wait_s > 0 && wait(&cmd_rx, std::time::Duration::from_secs(wait_s)) {
                    reconnect(&mut child, &mut attempts, &snap_t);
                }
            }

            let spawn = std::process::Command::new(&ssh_bin)
                .args(&args)
                .env("PATH", prefix.join("bin"))
                .env("LD_LIBRARY_PATH", prefix.join("lib"))
                .env("PREFIX", &prefix)
                .env("TERMUX__PREFIX", &prefix)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                // stderr 必抓（2026-09-21 立案仪器）：原先 null，ssh 退出 255
                // 时只留一个 exit code——「为什么断」成了猜。抓进小环，
                // 退出时随报表落一行真因（认证/撞口/拒连/keepalive 超时）
                .stderr(std::process::Stdio::piped())
                .spawn();
            match spawn {
                Ok(mut c) => {
                    crate::report::report(
                        "tunnel",
                        &format!("ssh 正连隧道已 spawn → 127.0.0.1:{port}"),
                    );
                    spawned_at = Some(std::time::Instant::now());
                    // stderr 读线程（随管道关闭自灭）：只留末 4 行非空
                    if let Some(err) = c.stderr.take() {
                        let ring = Arc::new(Mutex::new(VecDeque::<String>::new()));
                        ssh_err = Some(Arc::clone(&ring));
                        std::thread::spawn(move || {
                            use std::io::BufRead;
                            let r = std::io::BufReader::new(err);
                            for line in r.lines().map_while(Result::ok) {
                                let t = line.trim().to_string();
                                if t.is_empty() {
                                    continue;
                                }
                                if let Ok(mut g) = ring.lock() {
                                    g.push_back(t);
                                    while g.len() > 4 {
                                        g.pop_front();
                                    }
                                }
                            }
                        });
                    }
                    child = Some(c);
                    set(TunnelState::Starting, &snap_t);
                }
                Err(e) => {
                    attempts += 1;
                    crate::report::report("tunnel", &format!("ssh spawn 失败: {e}"));
                    set(
                        TunnelState::Down {
                            attempts,
                            last_error: format!("spawn 失败: {e}"),
                        },
                        &snap_t,
                    );
                    if wait(
                        &cmd_rx,
                        std::time::Duration::from_secs(backoff_secs(attempts)),
                    ) {
                        reconnect(&mut child, &mut attempts, &snap_t);
                    }
                }
            }
            if wait(&cmd_rx, std::time::Duration::from_secs(1)) {
                reconnect(&mut child, &mut attempts, &snap_t);
            }
        }
    });
    snap
}
