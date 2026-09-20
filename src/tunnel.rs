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

use std::path::PathBuf;
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

/// ssh 转发参数（A 档纯函数）。v1 只走密钥：密码登录在 BatchMode 下
/// 必悬问假死（无 askpass），显式拒；缺件（host/user/key 空）同拒。
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
                attempts += 1;
                crate::report::report(
                    "tunnel",
                    &format!("ssh 进程退出（{code:?}），第 {attempts} 次退避重拉"),
                );
                let wait_s = backoff_secs(attempts);
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
                .stderr(std::process::Stdio::null())
                .spawn();
            match spawn {
                Ok(c) => {
                    crate::report::report(
                        "tunnel",
                        &format!("ssh 正连隧道已 spawn → 127.0.0.1:{port}"),
                    );
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
