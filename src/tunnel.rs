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
        // 断线检测三件套（2026-09-23 BAR-140，「孤岛也要秒回」加固）：
        // ConnectTimeout=5——断网期 spawn 不许挂 75s TCP SYN 重试，
        // 5s 速败让退避表接管；ServerAlive 5×2——NAT 吞 RST 的静默死
        // 检测从 45s 压到 10s（端到端探活还会更快杀，见 e2e_strike）
        "-o".into(),
        "ConnectTimeout=5".into(),
        "-o".into(),
        "ServerAliveInterval=5".into(),
        "-o".into(),
        "ServerAliveCountMax=2".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-p".into(),
        s.ssh.port.to_string(),
        format!("{}@{}", s.ssh.user, s.ssh.host),
    ])
}

/// 看门狗退避表（A 档）：首死立即重拉（用户在场等不得），2/5/10s 渐进，
/// 30s 封顶——不指数爆炸，病态网络下每分钟至少敲一次门。
/// 2026-09-23 加抖动（BAR-138）：IP 轮换风暴里多路重试同频撞口成簇
/// （auth.log 513 次 bind 冲突实录），退避相位必须打散。熵 = 系统时钟
/// 纳秒（boot_ms 在测试里 BOOT_T0 未种恒为 0——首版钉红实录），
/// 免 rand 依赖；抖动量 = 基准的一半以内，首死（0s）不抖。
pub fn backoff_secs(attempt: u32) -> u64 {
    let base = match attempt {
        0 => 0,
        1 => 2,
        2 => 5,
        3 => 10,
        _ => 30,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() ^ (d.subsec_nanos() as u64))
        .unwrap_or(0);
    let jitter = now % (base / 2 + 1);
    base + jitter
}

/// 稳定窗口（秒）：娃活过这么久才算「一次真连接」——短于此 = 抖动。
/// 2026-09-23 BAR-142 从 30 收到 8：IP 轮换风暴里娃常活 10~30s，
/// 30s 门槛把它们全判「抖动」→ 退避账一路爬到 5/10s（用户实测恢复
/// 10~20s 的大头）。活 8s 以上 = 真扛过流量，下次死按首死待（2s 级）；
/// spawn 即死的真抖动照旧爬账，防抖语义不破
pub const STABLE_SECS: u64 = 8;

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

/// 端口释放裁决（A 档纯函数，2026-09-22 BAR-133）：两种情形都该释放本设备
/// 反连口——①死因自报撞口（`should_release_forward`）②**死前转发已绑上**
/// （established）：那一刻服务器那条会话已无主（客户就是刚死的这个），它占
/// 着口只会让下一轮重拉白撞一次（实测这一跳值 5~6 秒：11s 恢复里的大头）。
/// 不 established 且非撞口死因（如认证失败）一律不动——不许误杀活会话
pub fn should_release_port(established: bool, stderr_tail: &str, remote_port: u16) -> bool {
    established || should_release_forward(stderr_tail, remote_port)
}

/// 端到端探活连败裁决（A 档纯函数，BAR-140）：本地口通 ≠ 隧道活——
/// NAT 吞 RST 时 ssh 僵尸仍举着本地监听，probe_port 全绿而数据已死，
/// 干等 ssh 自己的 keepalive 要 10s。穿透隧道打 na-server 健康面，
/// 连败满 strike 即杀娃重拉。活着 → 清零；杀 → (0, true) 调用方立即重拉
pub fn e2e_strike(prev: u32, alive: bool) -> (u32, bool) {
    if alive {
        (0, false)
    } else {
        let n = prev + 1;
        (n, n >= 2)
    }
}

// ---- QUIC 腿（设计 docs/active/quic隧道.md §二桥接模型，默认关） ----

/// 指纹 hex → 32 字节（A 档纯函数）：恰 64 字符全 hex 才收
pub fn parse_pin(h: &str) -> Option<[u8; 32]> {
    if h.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, c) in h.as_bytes().chunks_exact(2).enumerate() {
        let hi = (c[0] as char).to_digit(16)?;
        let lo = (c[1] as char).to_digit(16)?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

/// QUIC 腿齐件判定（A 档纯函数）：开关开 + 口非 0 + 服务器证（pin）
/// 与客户端证（psk）双双合法——缺任一件 = 腿不存在，静默走 ssh；
/// 两证齐全才准开腿（公网口前置条件，设计 §四）
pub fn quic_configured(s: &ServerEntry) -> bool {
    s.quic.enable
        && s.quic.port != 0
        && parse_pin(&s.quic.pin).is_some()
        && parse_pin(&s.quic.psk).is_some()
}

/// 连挂跳闸线（A 档）：QUIC 腿连续死满此次数降级 ssh 兜底。
/// 计数只随手动重连/回前台即审清零——用户在等 = 给 QUIC 再投一票
pub const QUIC_FAIL_TRIP: u32 = 3;

/// 数据路供应商（A 档纯函数）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    Quic,
    Ssh,
}

/// 腿裁决（A 档纯函数）：QUIC 优先，跳闸降级 ssh 兜底
pub fn leg_verdict(configured: bool, quic_fails: u32) -> Leg {
    if configured && quic_fails < QUIC_FAIL_TRIP {
        Leg::Quic
    } else {
        Leg::Ssh
    }
}

/// -R-only ssh 参数（A 档纯函数）：QUIC 腿供数据路时 ssh 只挂反连
/// 推送路（9022 不断）——摘除 -L 两段，本地口唯一属主是 QUIC 腿
pub fn reverse_only_args(s: &ServerEntry) -> Result<Vec<String>, String> {
    let mut a = forward_args(s)?;
    if let Some(i) = a.iter().position(|x| x == "-L") {
        a.drain(i..i + 2);
    }
    Ok(a)
}

/// 端到端探活（B 档）：穿透本地转发口打 na-server 健康面，认 HTTP 200。
/// 超时 1.2s——两次连败 ≈ 2~3s 定罪僵尸，比 keepalive 快一个量级
fn probe_e2e(port: u16) -> bool {
    use std::io::{Read as _, Write as _};
    let Ok(mut s) = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(500),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(1200)));
    let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(500)));
    let req = "GET /api/na/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    crate::report::http_status_is_200(&resp)
}

/// 状态词（A 档纯函数）：连接/服务卡状态行的唯一文案源。
/// Down{attempts:0} = 从没起来过（缺件/prefix 未装同相）→「未启动」；
/// Down{n>0} = 退避中，必须带次数（用户要知道还在敲第几次门）。
pub fn state_word(st: &TunnelState) -> String {
    match st {
        TunnelState::Up => "自持在线".into(),
        TunnelState::QuicUp => "自持 QUIC 在线".into(),
        TunnelState::ExternalUp => "外部借用".into(),
        TunnelState::Starting => "连接中".into(),
        TunnelState::Down { attempts, .. } if *attempts == 0 => "未启动".into(),
        TunnelState::Down { attempts, .. } => format!("退避 ×{attempts}"),
    }
}

/// 传输可用相（A 档纯函数）：Up/QuicUp/ExternalUp 都是「本地口能走」——
/// 壳层会话只管 127.0.0.1:9021 通不通，不问是谁供的口。
pub fn usable(st: &TunnelState) -> bool {
    matches!(
        st,
        TunnelState::Up | TunnelState::QuicUp | TunnelState::ExternalUp
    )
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
    /// 回前台/网络回即审（BAR-141）：用户在等了，检测判据从宽——
    /// 健康连接不碰，僵尸一拍定罪（不等连败×2），退避清零立即重拉
    ResumeKick,
}

static TUNNEL_CMD: OnceLock<Sender<TunnelCmd>> = OnceLock::new();

/// 插件卡 [重连] 按钮的唯一入口；看门狗不在 = false（按钮侧可提示）
pub fn request_reconnect() -> bool {
    TUNNEL_CMD
        .get()
        .map(|tx| tx.send(TunnelCmd::Reconnect).is_ok())
        .unwrap_or(false)
}

/// 回前台即审（BAR-141）唯一入口：resumed()/网络回都踢这里
pub fn request_resume_kick() -> bool {
    TUNNEL_CMD
        .get()
        .map(|tx| tx.send(TunnelCmd::ResumeKick).is_ok())
        .unwrap_or(false)
}

/// 回前台即审判决（A 档纯函数，BAR-141）：健康不碰（杀健康连接 =
/// 没事找事），僵尸一拍定罪，退避中立即重拉
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAction {
    /// 健康/正在连——什么都不做
    Ignore,
    /// 僵尸定罪——杀娃、释放、零退避重拉
    KillRespawn,
    /// 没在连（退避/未启动）——零退避立即重拉
    Respawn,
}

pub fn resume_verdict(child_alive: bool, port_open: bool, probe_ok: bool) -> ResumeAction {
    if !child_alive {
        return ResumeAction::Respawn;
    }
    if port_open && !probe_ok {
        return ResumeAction::KillRespawn;
    }
    ResumeAction::Ignore
}

// ---- B 档：进程胶水（spawn/探活/看门狗），判卷 = 真机实拍 + report 行 ----

/// 隧道状态（连接/服务插件卡与断线状态卡的唯一数据源）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    /// ssh 进程在，本地口探活未过
    Starting,
    /// 自持隧道在线（我们 spawn 的 ssh 供出本地口）
    Up,
    /// QUIC 腿在线（na-quic 桥供出本地口，ssh 只挂 -R 推送路伴生）
    QuicUp,
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

/// 释放后等待（A 档纯函数，BAR-142）：释放**同步确认完成** = 口已腾 →
/// 零等待直接 spawn（BAR-132 的固定 4s 盲等作废——同步后释放本身
/// 就是等待，盲等 4s 纯属白等）；释放失败（断网/超时）= 2s 后重试，
/// 不许盲 spawn 白撞一轮 255
pub fn release_wait(release_ok: bool) -> u64 {
    if release_ok { 0 } else { 2 }
}

/// 反连口释放——同步核（BAR-142）：调用方线程内联跑（看门狗在死亡/
/// 杀娃路径上，本就在等，阻塞它有界），budget 封顶。true = 释放确认
/// 完成（口已腾）；false = 失败/超时（调用方 release_wait 退避重试）
fn release_forward_sync(prefix: &Path, server: &ServerEntry, budget: std::time::Duration) -> bool {
    let Ok(args) = crate::na_server_sup::exec_args(server) else {
        crate::report::report("tunnel", "反连口释放：ssh 参数缺件，放弃");
        return false;
    };
    let mut cmd = std::process::Command::new(prefix.join("bin/ssh"));
    let child = cmd
        .args(&args)
        .env("PATH", prefix.join("bin"))
        .env("LD_LIBRARY_PATH", prefix.join("lib"))
        .env("PREFIX", prefix)
        .env("TERMUX__PREFIX", prefix)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        crate::report::report("tunnel", "反连口释放：ssh spawn 失败");
        return false;
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
    match rx.recv_timeout(budget) {
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
            true
        }
        _ => {
            crate::report::report("tunnel", "反连口释放：超时/失败，下一轮再试");
            false
        }
    }
}

/// QUIC 腿句柄（看门狗私有）：stop = 请它收（oneshot 进腿线程的 select），
/// dead = 腿的死信（run_client 返回/线程消失 = 腿死）——死亡检测事件驱动，
/// 这就是 QUIC 腿省掉探活三件套的原因（设计 docs/active/quic隧道.md §五）
struct QuicLeg {
    stop: tokio::sync::oneshot::Sender<()>,
    dead: Receiver<String>,
}

/// 起 QUIC 腿（核内线程，非外部进程）：自带 current_thread tokio runtime
/// 跑 na_quic::run_client——本机 127.0.0.1:{local_port} ←QUIC→ 服务器
/// UDP {quic.port} → 回联 {target_port}。齐件判定在上游（leg_verdict），
/// 本函数只吃已裁决的条目；None = 指纹不合法（上游已拦，此处兜底）
fn spawn_quic_leg(server: &ServerEntry) -> Option<QuicLeg> {
    use std::net::ToSocketAddrs as _;
    let pin = parse_pin(&server.quic.pin)?;
    let psk = parse_pin(&server.quic.psk)?;
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (dead_tx, dead) = channel::<String>();
    let host = server.ssh.host.clone();
    let qport = server.quic.port;
    let local = std::net::SocketAddr::from(([127, 0, 0, 1], server.tunnel.local_port));
    let target = target_port(&server.backend);
    std::thread::spawn(move || {
        let say = |m: String| {
            let _ = dead_tx.send(m);
        };
        let Some(addr) = format!("{host}:{qport}")
            .to_socket_addrs()
            .ok()
            .and_then(|mut i| i.next())
        else {
            say("DNS 解析失败".into());
            return;
        };
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                say(format!("runtime 起不来: {e}"));
                return;
            }
        };
        rt.block_on(async move {
            tokio::select! {
                r = na_quic::run_client(
                    addr,
                    "kfm-na",
                    local,
                    target,
                    na_quic::client_config(pin),
                    Some(psk),
                ) => {
                    say(match r {
                        Ok(()) => "腿正常退出".into(),
                        Err(e) => format!("腿死: {e}"),
                    });
                }
                _ = stop_rx => say("看门狗请收".into()),
            }
        });
    });
    Some(QuicLeg { stop, dead })
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
        // QUIC 腿（默认关，设置页 servers.json "quic" 段开启）：腿在 = 它供
        // 数据路，ssh 只挂 -R 推送路伴生；腿死连挂 QUIC_FAIL_TRIP 次跳闸，
        // 降级 ssh 兜底，手动重连/回前台即审清零再给 QUIC 一票
        let mut quic: Option<QuicLeg> = None;
        let mut quic_fails: u32 = 0;
        let set = |st: TunnelState, snap_t: &Arc<Mutex<TunnelSnap>>| {
            let mut g = snap_t.lock().unwrap();
            if g.state != st {
                crate::report::report("tunnel", &format!("状态 {:?} → {:?}", g.state, st));
                g.state = st;
                g.epoch += 1;
            }
        };
        // 等一拍（可被取消）：命令到了 = Some。重连语义 = 杀娃（有的话）+
        // 退避清零，下一拍立刻重新探口/spawn——不等退避不等 30s 复查
        let wait = |rx: &Receiver<TunnelCmd>, dur: std::time::Duration| -> Option<TunnelCmd> {
            rx.recv_timeout(dur).ok()
        };
        let reconnect = |child: &mut Option<std::process::Child>,
                         quic: &mut Option<QuicLeg>,
                         attempts: &mut u32,
                         quic_fails: &mut u32,
                         snap_t: &Arc<Mutex<TunnelSnap>>| {
            crate::report::report("tunnel", "手动重连：杀娃收腿重拉");
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait(); // 收尸——不留僵尸，口随进程 teardown 立刻放
            }
            if let Some(q) = quic.take() {
                let _ = q.stop.send(());
            }
            *attempts = 0;
            *quic_fails = 0; // 用户在等 = 给 QUIC 再投一票
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
        // 死前转发是否绑上过（口开过 = 服务器那条 -R 会话存在过；它一死即无主）
        let mut was_up = false;
        // 端到端探活连败计数（BAR-140，看门狗线程私有）
        let mut e2e_miss: u32 = 0;
        // 杀娃重拉（BAR-140/141 共用）：杀娃收尸、退避清零、死前绑过顺路
        // 请服务器收尸（免下一 spawn 白撞 255）；状态落 Down，调用方 continue
        let kill_zombie =
            |child: &mut Option<std::process::Child>,
             attempts: &mut u32,
             e2e_miss: &mut u32,
             was_up: bool,
             why: &str,
             prefix: &Path,
             server: &ServerEntry,
             snap_t: &Arc<Mutex<TunnelSnap>>,
             set: &dyn Fn(TunnelState, &Arc<Mutex<TunnelSnap>>)| {
                crate::report::report("tunnel", why);
                if let Some(mut c) = child.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                *e2e_miss = 0;
                *attempts = 0;
                if was_up {
                    release_forward_sync(prefix, server, std::time::Duration::from_secs(2));
                }
                set(
                    TunnelState::Down {
                        attempts: 0,
                        last_error: why.to_string(),
                    },
                    snap_t,
                );
            };
        let mut ssh_err: Option<Arc<Mutex<VecDeque<String>>>> = None;
        loop {
            let port = server.tunnel.local_port;
            let port_open = probe_port(port);
            let child_alive = child
                .as_mut()
                .map(|c| c.try_wait().ok().flatten().is_none())
                .unwrap_or(false);

            // QUIC 腿死信审理（事件驱动死亡检测）：腿死 → 记一笔跳闸账，
            // ssh 伴生一并收（数据路换供应商，ssh 角色随之换），不睡——
            // 落到重生段按 leg_verdict 立即重拉（兜底 ssh 零等待接上）
            let mut quic_msg: Option<String> = None;
            if let Some(q) = &quic {
                match q.dead.try_recv() {
                    Ok(m) => quic_msg = Some(m),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        quic_msg = Some("腿线程消失".into())
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
            if let Some(msg) = quic_msg {
                quic = None;
                quic_fails += 1;
                crate::report::report(
                    "tunnel",
                    &format!("QUIC 腿死（{msg}），第 {quic_fails}/{QUIC_FAIL_TRIP} 次"),
                );
                if let Some(mut c) = child.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                set(
                    TunnelState::Down {
                        attempts: quic_fails,
                        last_error: format!("QUIC 腿死: {msg}"),
                    },
                    &snap_t,
                );
            }

            if port_open && !child_alive && quic.is_none() {
                // 外部隧道占着口——让位不抢，30s 一拍复查（它一断下一拍接管）
                set(TunnelState::ExternalUp, &snap_t);
                if wait(&cmd_rx, std::time::Duration::from_secs(30)).is_some() {
                    reconnect(
                        &mut child,
                        &mut quic,
                        &mut attempts,
                        &mut quic_fails,
                        &snap_t,
                    );
                }
                continue;
            }

            if quic.is_some() {
                was_up = port_open;
                // 伴生 -R ssh 死亡审理：死了就地收，段尾重生（数据路不归它，
                // 状态不动——QUIC 腿在，卡上就不许为推送路抖动翻状态）
                if let Some(c) = child.as_mut()
                    && let Some(code) = c.try_wait().ok().flatten()
                {
                    crate::report::report(
                        "tunnel",
                        &format!("伴生 -R ssh 退出（{code:?}），段尾重拉"),
                    );
                    child = None;
                }
                // 端到端探活（BAR-140 同款判据打 QUIC 桥）：连败×2 收腿
                if port_open {
                    let (n, kill) = e2e_strike(e2e_miss, probe_e2e(port));
                    e2e_miss = n;
                    if kill {
                        crate::report::report(
                            "tunnel",
                            "端到端探活连败×2：QUIC 僵尸定罪，收腿立即重拉",
                        );
                        if let Some(q) = quic.take() {
                            let _ = q.stop.send(());
                        }
                        quic_fails += 1;
                        e2e_miss = 0;
                        if let Some(mut c) = child.take() {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        set(
                            TunnelState::Down {
                                attempts: quic_fails,
                                last_error: "QUIC 僵尸".into(),
                            },
                            &snap_t,
                        );
                        continue;
                    }
                }
                set(
                    if port_open {
                        TunnelState::QuicUp
                    } else {
                        TunnelState::Starting
                    },
                    &snap_t,
                );
                match wait(&cmd_rx, std::time::Duration::from_secs(1)) {
                    Some(TunnelCmd::Reconnect) => reconnect(
                        &mut child,
                        &mut quic,
                        &mut attempts,
                        &mut quic_fails,
                        &snap_t,
                    ),
                    // 回前台即审（BAR-141）：僵尸一拍定罪（不等连败×2）
                    Some(TunnelCmd::ResumeKick) => {
                        let probe_ok = !port_open || probe_e2e(port);
                        if port_open && !probe_ok {
                            crate::report::report(
                                "tunnel",
                                "回前台即审：QUIC 僵尸一拍定罪，收腿立即重拉",
                            );
                            if let Some(q) = quic.take() {
                                let _ = q.stop.send(());
                            }
                            quic_fails += 1;
                            if let Some(mut c) = child.take() {
                                let _ = c.kill();
                                let _ = c.wait();
                            }
                            set(
                                TunnelState::Down {
                                    attempts: quic_fails,
                                    last_error: "QUIC 僵尸（回前台即审）".into(),
                                },
                                &snap_t,
                            );
                        }
                    }
                    None => {}
                }
                if child.is_some() || quic.is_none() {
                    continue; // 伴生活着呢（或腿刚被收走落 Down）——下一拍再说
                }
                // 伴生死了：落到重生段补一条 -R-only ssh
            } else if child_alive {
                was_up = port_open;
                // 端到端探活（BAR-140）：本地口通 ≠ 隧道活——NAT 吞 RST 时
                // ssh 僵尸举着本地监听，数据面已死。连败×2 即杀娃立即重拉
                // （不等 ssh keepalive 10s，不等退避）；死前绑过 → 顺路请
                // 服务器收尸，免下一 spawn 白撞一次 255
                if port_open {
                    let (n, kill) = e2e_strike(e2e_miss, probe_e2e(port));
                    e2e_miss = n;
                    if kill {
                        kill_zombie(
                            &mut child,
                            &mut attempts,
                            &mut e2e_miss,
                            was_up,
                            "端到端探活连败×2：僵尸隧道定罪，杀娃立即重拉",
                            &prefix,
                            &server,
                            &snap_t,
                            &set,
                        );
                        continue;
                    }
                }
                set(
                    if port_open {
                        TunnelState::Up
                    } else {
                        TunnelState::Starting
                    },
                    &snap_t,
                );
                match wait(&cmd_rx, std::time::Duration::from_secs(1)) {
                    Some(TunnelCmd::Reconnect) => reconnect(
                        &mut child,
                        &mut quic,
                        &mut attempts,
                        &mut quic_fails,
                        &snap_t,
                    ),
                    // 回前台即审（BAR-141）：用户在等——健康不碰，僵尸一拍
                    // 定罪（不等连败×2），零退避立即重拉
                    Some(TunnelCmd::ResumeKick) => {
                        let probe_ok = !port_open || probe_e2e(port);
                        if resume_verdict(true, port_open, probe_ok) == ResumeAction::KillRespawn {
                            kill_zombie(
                                &mut child,
                                &mut attempts,
                                &mut e2e_miss,
                                was_up,
                                "回前台即审：僵尸隧道一拍定罪，杀娃立即重拉",
                                &prefix,
                                &server,
                                &snap_t,
                                &set,
                            );
                        }
                    }
                    None => {}
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
                let established = was_up;
                was_up = false;
                let port_hit = should_release_forward(&tail, server.tunnel.remote_port);
                let releasing = should_release_port(established, &tail, server.tunnel.remote_port);
                // BAR-142：释放改同步确认——确认完成（口已腾）零等待直接
                // spawn；失败（断网/超时）2s 重试，不许盲 spawn 白撞一轮
                let wait_s = if releasing {
                    crate::report::report(
                        "tunnel",
                        &format!(
                            "反连口 {} 释放（{}）→ 同步收紧",
                            server.tunnel.remote_port,
                            if port_hit {
                                "撞口后"
                            } else {
                                "预防式：死前已绑过，残留会话无主"
                            }
                        ),
                    );
                    let ok =
                        release_forward_sync(&prefix, &server, std::time::Duration::from_secs(3));
                    if ok {
                        attempts = 1; // 口腾了 = 从头来（下一轮 2s 级）
                    }
                    release_wait(ok)
                } else {
                    backoff_secs(attempts)
                };
                set(
                    TunnelState::Down {
                        attempts,
                        last_error: format!("ssh 退出 {code:?}"),
                    },
                    &snap_t,
                );
                if wait_s > 0 {
                    match wait(&cmd_rx, std::time::Duration::from_secs(wait_s)) {
                        Some(TunnelCmd::Reconnect) => reconnect(
                            &mut child,
                            &mut quic,
                            &mut attempts,
                            &mut quic_fails,
                            &snap_t,
                        ),
                        // 回前台即审（BAR-141）：退避中的用户在等——清零立即
                        // spawn；QUIC 跳闸账同清（用户在等 = 再给 QUIC 一票）
                        Some(TunnelCmd::ResumeKick) => {
                            attempts = 0;
                            quic_fails = 0;
                        }
                        None => {}
                    }
                }
            }

            // 数据路裁决（QUIC 优先，跳闸降级 ssh 兜底）：腿不在且配置齐件
            // 未跳闸 → 起 QUIC 腿；ssh 随之降级为 -R-only 伴生（推送路不断）
            if quic.is_none() && leg_verdict(quic_configured(&server), quic_fails) == Leg::Quic {
                match spawn_quic_leg(&server) {
                    Some(q) => {
                        crate::report::report(
                            "tunnel",
                            &format!(
                                "QUIC 腿已 spawn → 127.0.0.1:{port}（UDP {}:{}）",
                                server.ssh.host, server.quic.port
                            ),
                        );
                        quic = Some(q);
                        e2e_miss = 0;
                        set(TunnelState::Starting, &snap_t);
                    }
                    None => {
                        quic_fails += 1;
                        crate::report::report(
                            "tunnel",
                            &format!("QUIC 腿 spawn 缺件（指纹不合法），第 {quic_fails} 次"),
                        );
                    }
                }
            }
            // ssh 参数：腿在 = -R-only 伴生；腿不在 = 全量正连（-L + -R）
            let ssh_args = if quic.is_some() {
                match reverse_only_args(&server) {
                    Ok(a) => a,
                    Err(e) => {
                        crate::report::report("tunnel", &format!("伴生 ssh 参数缺件: {e}"));
                        let _ = wait(&cmd_rx, std::time::Duration::from_secs(1));
                        continue;
                    }
                }
            } else {
                args.clone()
            };

            let spawn = std::process::Command::new(&ssh_bin)
                .args(&ssh_args)
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
                        &if quic.is_some() {
                            format!(
                                "伴生 -R ssh 已 spawn（推送路 → {}）",
                                server.tunnel.remote_port
                            )
                        } else {
                            format!("ssh 正连隧道已 spawn → 127.0.0.1:{port}")
                        },
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
                    // 伴生重生不碰状态——QUIC 腿分支自管 QuicUp/Starting
                    if quic.is_none() {
                        set(TunnelState::Starting, &snap_t);
                    }
                }
                Err(e) => {
                    attempts += 1;
                    crate::report::report("tunnel", &format!("ssh spawn 失败: {e}"));
                    // 伴生 spawn 失败不翻数据路状态（QUIC 腿在，口还通）
                    if quic.is_none() {
                        set(
                            TunnelState::Down {
                                attempts,
                                last_error: format!("spawn 失败: {e}"),
                            },
                            &snap_t,
                        );
                    }
                    match wait(
                        &cmd_rx,
                        std::time::Duration::from_secs(backoff_secs(attempts)),
                    ) {
                        Some(TunnelCmd::Reconnect) => reconnect(
                            &mut child,
                            &mut quic,
                            &mut attempts,
                            &mut quic_fails,
                            &snap_t,
                        ),
                        Some(TunnelCmd::ResumeKick) => {
                            attempts = 0;
                            quic_fails = 0;
                        }
                        None => {}
                    }
                }
            }
            if let Some(cmd) = wait(&cmd_rx, std::time::Duration::from_secs(1)) {
                match cmd {
                    TunnelCmd::Reconnect => reconnect(
                        &mut child,
                        &mut quic,
                        &mut attempts,
                        &mut quic_fails,
                        &snap_t,
                    ),
                    TunnelCmd::ResumeKick => {
                        attempts = 0;
                        quic_fails = 0;
                    }
                }
            }
        }
    });
    snap
}
