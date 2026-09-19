//! 隧道考题（A 档）：ssh 转发参数构造 + 退避表——纯逻辑先行钉死，
//! 进程胶水（B 档）在 src/tunnel.rs 下半。
//!
//! 变异抽检：①目标端口写错（9021→8021 拼反）必须咬；
//! ②BatchMode 漏了必须咬（无 askpass 时密码悬问 = 隧道假死）。

use kfm_na::settings::{ServerEntry, SshFields, TunnelPorts};
use kfm_na::tunnel::{
    TARGET_PORT, TunnelState, backoff_secs, forward_args, state_word, usable, usable_edge_kick,
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
        a.iter().any(|x| x.starts_with("ServerAliveInterval=")),
        "保活探测"
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
    // 变异锚：TARGET_PORT 必须与 kfmv4 ws 端口同锚（ConnConfig::default 的 8021）
    assert_eq!(TARGET_PORT, 8021, "转发目标口 = kfmv4 ws 口，漂移即全线断");
    let a = args_of(&srv("h", "u", "/k"));
    assert!(a.join(" ").contains(&format!(":{TARGET_PORT}")));
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
    assert_eq!(backoff_secs(0), 0, "首死立即重试（用户在场等不得）");
    assert_eq!(backoff_secs(1), 2);
    assert_eq!(backoff_secs(2), 5);
    assert_eq!(backoff_secs(3), 10);
    assert_eq!(backoff_secs(4), 30);
    assert_eq!(backoff_secs(99), 30, "封顶 30s，不指数爆炸");
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
