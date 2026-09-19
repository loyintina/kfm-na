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
use std::sync::{Arc, Mutex};

use crate::settings::ServerEntry;

/// 转发目标口 = kfmv4 ws 口（与 conn::ConnConfig::default 的回环 8021 同锚；
/// 漂移 = 隧道通了但 ws 全断，考题 spec_转发参数_目标口单一源 钉死）
pub const TARGET_PORT: u16 = 8021;

/// ssh 转发参数（A 档纯函数）。v1 只走密钥：密码登录在 BatchMode 下
/// 必悬问假死（无 askpass），显式拒；缺件（host/user/key 空）同拒。
pub fn forward_args(s: &ServerEntry) -> Result<Vec<String>, String> {
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
    Ok(vec![
        "-N".into(),
        "-L".into(),
        format!("{}:127.0.0.1:{}", s.tunnel.local_port, TARGET_PORT),
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
/// 返回共享快照——UI/报表只读这份。
pub fn start(prefix: PathBuf, server: ServerEntry) -> Arc<Mutex<TunnelSnap>> {
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
    }));
    let Ok(args) = forward_args(&server) else {
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
    let ssh_bin = prefix.join("bin/ssh");
    let snap_t = Arc::clone(&snap);
    std::thread::spawn(move || {
        let mut child: Option<std::process::Child> = None;
        let mut attempts: u32 = 0;
        let mut external_since: Option<std::time::Instant> = None;
        let set = |st: TunnelState, snap_t: &Arc<Mutex<TunnelSnap>>| {
            let mut g = snap_t.lock().unwrap();
            if g.state != st {
                crate::report::report("tunnel", &format!("状态 {:?} → {:?}", g.state, st));
                g.state = st;
            }
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
                external_since.get_or_insert_with(std::time::Instant::now);
                std::thread::sleep(std::time::Duration::from_secs(30));
                continue;
            }
            external_since = None;

            if child_alive {
                set(
                    if port_open {
                        TunnelState::Up
                    } else {
                        TunnelState::Starting
                    },
                    &snap_t,
                );
                std::thread::sleep(std::time::Duration::from_secs(1));
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
                let wait = backoff_secs(attempts);
                set(
                    TunnelState::Down {
                        attempts,
                        last_error: format!("ssh 退出 {code:?}"),
                    },
                    &snap_t,
                );
                if wait > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(wait));
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
                    std::thread::sleep(std::time::Duration::from_secs(backoff_secs(attempts)));
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    snap
}
