//! 隧道考题（A 档）：ssh 转发参数构造 + 退避表——纯逻辑先行钉死，
//! 进程胶水（B 档）在 src/tunnel.rs 下半。
//!
//! 变异抽检：①目标端口写错（9021→8021 拼反）必须咬；
//! ②BatchMode 漏了必须咬（无 askpass 时密码悬问 = 隧道假死）；
//! ③-R 反连裸口打头（公网暴露面）/ 整段摘除必须咬。

use kfm_na::settings::{Backend, ServerEntry, SshFields, TunnelPorts};
use kfm_na::tunnel::{
    KFMV4_PORT, NA_SERVER_PORT, TunnelState, backoff_secs, forward_args, state_word, target_port,
    usable, usable_edge_kick,
};

fn srv(host: &str, user: &str, key: &str) -> ServerEntry {
    ServerEntry {
        id: "main".into(),
        name: "主服".into(),
        ssh: SshFields {
            host: host.into(),
            port: 22,
            user: user.into(),
            key_path: key.into(),
            password: String::new(),
        },
        tunnel: TunnelPorts {
            local_port: 9021,
            remote_port: 9022,
        },
        ws_url: String::new(),
        command: None,
        hotkey: None,
        backend: Backend::Kfmv4,
    }
}

fn args_of(s: &ServerEntry) -> Vec<String> {
    forward_args(s).expect("合法条目必须出参")
}

#[test]
fn spec_转发参数_本地段() {
    let a = args_of(&srv("8.145.46.182", "root", "/k/id_ed25519"));
    let joined = a.join(" ");
    assert!(
        joined.contains("-L 9021:127.0.0.1:8021"),
        "本地转发三元组：本地口→回环→kfmv4 口，实际 {joined}"
    );
    assert!(a.iter().any(|x| x == "-N"), "不执行远程命令 -N");
    assert!(
        a.iter().any(|x| x == "BatchMode=yes"),
        "无 askpass 时必须禁交互（密码悬问 = 假死）"
    );
    assert!(
        a.iter().any(|x| x == "ExitOnForwardFailure=yes"),
        "绑定失败必须死掉让看门狗知情，不许静默裸奔"
    );
    assert!(
        a.iter().any(|x| x == "ServerAliveInterval=5"),
        "保活探测 5s 一拍（BAR-140：静默死检测 45s→10s）"
    );
    assert!(
        a.iter().any(|x| x == "ServerAliveCountMax=2"),
        "两拍不应即判死"
    );
    assert!(
        a.iter().any(|x| x == "ConnectTimeout=5"),
        "断网期 spawn 5s 速败（BAR-140：不许挂 75s SYN 重试）"
    );
    assert!(a.last().unwrap() == "root@8.145.46.182", "目的端收尾");
    assert!(
        a.windows(2).any(|w| w[0] == "-p" && w[1] == "22"),
        "sshd 端口"
    );
    assert!(
        a.windows(2)
            .any(|w| w[0] == "-i" && w[1] == "/k/id_ed25519"),
        "密钥路径"
    );
}

#[test]
fn spec_转发参数_目标口单一源() {
    // 变异锚：双口钉死（kfmv4 ws = 8021；na-server = 9021 双端同口）
    assert_eq!(KFMV4_PORT, 8021, "kfmv4 ws 口，漂移即全线断");
    assert_eq!(NA_SERVER_PORT, 9021, "na-server 口（na-server.md §二）");
    assert_eq!(target_port(&Backend::Kfmv4), KFMV4_PORT);
    assert_eq!(target_port(&Backend::NaServer), NA_SERVER_PORT);
    // Kfmv4 后端 = 现状锚（8021）
    let a = args_of(&srv("h", "u", "/k"));
    assert!(a.join(" ").contains("-L 9021:127.0.0.1:8021"));
    // NaServer 后端 = 隧道指 na-server（9021→9021）
    let mut s = srv("h", "u", "/k");
    s.backend = Backend::NaServer;
    let a = args_of(&s);
    assert!(
        a.join(" ").contains("-L 9021:127.0.0.1:9021"),
        "NaServer 后端隧道必须指 9021，实际 {}",
        a.join(" ")
    );
}

#[test]
fn spec_转发参数_缺件全拒() {
    assert!(forward_args(&srv("", "root", "/k")).is_err(), "空 host");
    assert!(forward_args(&srv("h", "", "/k")).is_err(), "空 user");
    assert!(forward_args(&srv("h", "root", "")).is_err(), "空 key");
    let mut s = srv("h", "root", "/k");
    s.ssh.password = "secret".into();
    assert!(
        forward_args(&s).is_err(),
        "v1 只走密钥：密码登录 BatchMode 必悬问，显式拒"
    );
}

#[test]
fn spec_退避表() {
    // BAR-138 起带抖动（相位打散防撞口簇）：断言从精确等值改为区间
    // [base, base+base/2]，base 表本身不变
    assert_eq!(backoff_secs(0), 0, "首死立即重试（用户在场等不得），不抖");
    for (n, base) in [(1u32, 2u64), (2, 5), (3, 10), (4, 30), (99, 30)] {
        let v = backoff_secs(n);
        assert!(
            (base..=base + base / 2).contains(&v),
            "attempt {n}: {v} 不在 [{base}, {}] 抖动区间",
            base + base / 2
        );
    }
}

#[test]
fn spec_转发参数_反连段() {
    // 2026-09-21 用户拍板「9022 我们自己的推送路径」：-R 反连并进
    // na 自持隧道（v1 归 Termux 维护，Termux 休眠冻结 = 推送全瘫一整天）。
    let a = args_of(&srv("h", "u", "/k"));
    let joined = a.join(" ");
    assert!(
        joined.contains("-R 127.0.0.1:9022:127.0.0.1:8024"),
        "反连三元组：服务器 9022 → 手机 NA sshd 8024，实际 {joined}"
    );
    assert!(
        !joined.contains("-R 9022:"),
        "-R 裸口打头 = 绑公网 0.0.0.0，暴露面回潮（2026-09-01 红线），实际 {joined}"
    );
    // 远端口跟配置走（换服务器/改口时设置页可配）
    let mut s = srv("h", "u", "/k");
    s.tunnel.remote_port = 9122;
    assert!(
        args_of(&s)
            .join(" ")
            .contains("-R 127.0.0.1:9122:127.0.0.1:8024"),
        "remote_port 必须可配"
    );
}

#[test]
fn spec_自定义本地口() {
    let mut s = srv("h", "u", "/k");
    s.tunnel.local_port = 9121;
    assert!(args_of(&s).join(" ").contains("-L 9121:127.0.0.1:8021"));
}

#[test]
fn spec_状态词_四态五相() {
    // 连接/服务卡状态行的唯一文案源——词变了考题必须跟着改
    assert_eq!(state_word(&TunnelState::Up), "自持在线");
    assert_eq!(state_word(&TunnelState::ExternalUp), "外部借用");
    assert_eq!(state_word(&TunnelState::Starting), "连接中");
    assert_eq!(
        state_word(&TunnelState::Down {
            attempts: 0,
            last_error: "未启动".into()
        }),
        "未启动",
        "attempts=0 的 Down = 从没起来过（缺件/prefix 未装同相）"
    );
    assert_eq!(
        state_word(&TunnelState::Down {
            attempts: 3,
            last_error: "ssh 退出".into()
        }),
        "退避 ×3",
        "退避中必须带次数——用户要知道它还在敲第几次门"
    );
}

/// BAR-117：隧道可用沿踢壳层重孵的裁决（2026-09-20 接管终判现场定罪：
/// 传输 Up 了壳层 remote_dead 卡死——重孵链死亡事件驱动，末次重孵撞
/// TCP refused 被 5s 闸压住后再无死亡事件 = 链断）。
#[test]
fn spec_bar117_隧道up沿_踢活跃死会话重孵() {
    let down = TunnelState::Down {
        attempts: 1,
        last_error: "ssh 退出".into(),
    };
    // 不可用 → Up 且活跃会话死了 = 踢（接管终判的原场景）
    assert!(usable_edge_kick(false, &TunnelState::Up, true));
    // 不可用 → ExternalUp 同样踢（外部借用期会话也走同一本地口）
    assert!(usable_edge_kick(false, &TunnelState::ExternalUp, true));
    // 稳定在线不踢（Up→Up 每圈踢 = 重孵风暴）
    assert!(!usable_edge_kick(true, &TunnelState::Up, true));
    assert!(!usable_edge_kick(true, &TunnelState::ExternalUp, true));
    // 会话活着不踢（好端端的会话不许被隧道事件顶掉）
    assert!(!usable_edge_kick(false, &TunnelState::Up, false));
    // 可用 → Down 不踢（断线沿另有死亡事件驱动）
    assert!(!usable_edge_kick(true, &down, true));
    // Down→Down 不踢
    assert!(!usable_edge_kick(false, &down, true));
    // usable 相表：Up/ExternalUp 可用，Starting/Down 不可用
    assert!(usable(&TunnelState::Up));
    assert!(usable(&TunnelState::ExternalUp));
    assert!(!usable(&TunnelState::Starting));
    assert!(!usable(&down));
}

#[test]
fn spec_抖动_短命退避不归零() {
    // 2026-09-21「反复连接反复断开」立案：原先一 Up 就 attempts=0，抖动
    // 网络下退化成每 2s 重拉 ssh 的热循环（手机无线电 + 服务器 sshd 同挨）
    use kfm_na::tunnel::{STABLE_SECS, next_attempts};
    // 真连接（活够稳定窗口）→ 回 1（首死立即重拉，用户在场等不得）
    assert_eq!(next_attempts(7, STABLE_SECS), 1);
    assert_eq!(next_attempts(1, 600), 1);
    // 短命娃（一 spawn 即死/秒级死）→ 计数续涨（退避爬 5/10/30s）
    assert_eq!(next_attempts(0, 0), 1);
    assert_eq!(next_attempts(1, 1), 2);
    assert_eq!(next_attempts(2, STABLE_SECS - 1), 3);
    assert_eq!(next_attempts(9, 3), 10);
    // 退避表随计数爬到封顶（不指数爆炸；BAR-138 起带抖动，断言区间）
    use kfm_na::tunnel::backoff_secs;
    for (n, base) in [(1u32, 2u64), (2, 5), (3, 10), (4, 30), (99, 30)] {
        let v = backoff_secs(n);
        assert!((base..=base + base / 2).contains(&v), "attempt {n}: {v}");
    }
}

#[test]
fn spec_反连口_释放判决与脚本() {
    // BAR-129：恢复后反复掉 = 上一轮会话还占着本设备反连口（ClientAlive
    // 90s 收割窗），不等它，直接释放。判决必须严：只有「远程转发绑定失败
    // + 本设备口号」才动手——别的死因（认证/拒连/keepalive）触发释放会
    // 把一条活会话杀掉
    use kfm_na::tunnel::{release_forward_script, should_release_forward};
    let real = "Warning: remote port forwarding failed for listen port 9022";
    assert!(should_release_forward(real, 9022), "撞本设备口 = 释放");
    assert!(
        !should_release_forward(real, 9122),
        "别的设备的口（redroid 9122）不归我们管"
    );
    assert!(
        !should_release_forward("Permission denied (publickey).", 9022),
        "认证失败不许触发（会杀掉活会话）"
    );
    assert!(!should_release_forward("Connection refused", 9022));
    assert!(!should_release_forward(
        "Timeout, server not responding.",
        9022
    ));
    assert!(!should_release_forward("", 9022));
    // 脚本：按本设备口过滤，且只杀 sshd（非 sshd 一律不动）
    let s = release_forward_script(9022);
    assert!(s.contains("sport = :9022"), "按口过滤：{s}");
    assert!(
        s.contains("comm=") && s.contains("^sshd"),
        "只杀 sshd 防误伤"
    );
    assert!(s.contains("kill"), "有杀动作");
    assert!(s.contains("none"), "无占用要显形（判卷可读）");
    let s2 = release_forward_script(9122);
    assert!(s2.contains("sport = :9122"), "口参数化（每设备段）");
}

#[test]
fn spec_bar132_释放成功即免退避() {
    // 2026-09-22 BAR-132：释放成功 ≈ 口已腾 → 只等 4s 再试，不背爬升的
    // 退避账（实测那次从断到恢复 29s，其中 30s 退避白等）；别的死因照退避
    use kfm_na::tunnel::retry_wait;
    assert_eq!(retry_wait(4, true), 4, "释放路不背退避账");
    assert_eq!(retry_wait(99, true), 4);
    for a in 1..=6u32 {
        // 2026-09-23 BAR-138：backoff_secs 带抖动（基准一半以内），
        // 退避区间钉 [base, base+base/2]，不再钉等值
        let base = match a {
            1 => 2,
            2 => 5,
            3 => 10,
            _ => 30,
        };
        let w = retry_wait(a, false);
        assert!(
            (base..=base + base / 2).contains(&w),
            "a={a}: 退避 {w} 越出 [{base}, {}]",
            base + base / 2
        );
    }
}

#[test]
fn spec_bar133_死前绑过就预防式释放() {
    // 2026-09-22 BAR-133：移动网换 IP 掐死隧道后，重拉必撞上一轮残留反连口
    // （实测那一跳值 5~6 秒，11s 恢复里的大头）。裁决两种情形都释放，别的
    // 死因（认证失败等）一律不动——不许误杀活会话
    use kfm_na::tunnel::should_release_port;
    // ①死前绑过（建立的会话此刻已无主）→ 释放，哪怕死因无关
    assert!(should_release_port(
        true,
        "Connection to x closed by remote host.",
        9022
    ));
    assert!(should_release_port(true, "", 9022));
    // ②没绑过但死因自报撞口 → 释放（原来那条路保留）
    assert!(should_release_port(
        false,
        "Error: remote port forwarding failed for listen port 9022",
        9022
    ));
    // ③没绑过 + 别的死因（认证/拒连）→ 不动（防误杀）
    assert!(!should_release_port(
        false,
        "Permission denied (publickey).",
        9022
    ));
    assert!(!should_release_port(false, "Connection refused", 9022));
    assert!(!should_release_port(false, "", 9022));
    // ④他设备口号不归我们管（撞口判据里已含口匹配）
    assert!(!should_release_port(
        false,
        "Error: remote port forwarding failed for listen port 9122",
        9022
    ));
}

#[test]
fn spec_bar138_退避抖动_相位打散() {
    // BAR-138：抖动必须真实存在（退避退化成定值 = 撞口簇回魂）。
    // base=2 抖 0/1：连采必见 {2,3} 两值（系统时钟纳秒进位翻转）
    let mut seen = std::collections::HashSet::new();
    for _ in 0..40 {
        seen.insert(backoff_secs(1));
        if seen.len() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        seen.contains(&2) && seen.contains(&3),
        "抖动未生效：连采只见 {seen:?}"
    );
}

#[test]
fn spec_bar140_端到端探活连败_即杀即拉() {
    // BAR-140：本地口通 ≠ 隧道活（NAT 吞 RST，ssh 僵尸举监听）。
    // 连败×2 才杀（单发抖动不冤杀）；活一拍清零；杀后计数归零（立即重拉）
    use kfm_na::tunnel::e2e_strike;
    assert_eq!(e2e_strike(0, true), (0, false), "活：清零不杀");
    assert_eq!(e2e_strike(1, true), (0, false), "活：清掉前科");
    assert_eq!(e2e_strike(0, false), (1, false), "首败：记账不杀");
    assert_eq!(e2e_strike(1, false), (2, true), "连败×2：定罪杀");
    // 杀完归零由调用方负责，裁决函数自身不许返回负计数
    assert_eq!(e2e_strike(5, true), (0, false), "再多前科也清零");
}
