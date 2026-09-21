//! tests/na_server_sup_spec.rs — A 档考题：主体拉起链纯逻辑
//! （ensure 脚本/exec 参数/verdict 解析/状态词，src/na_server_sup.rs）
//!
//! 纪律：本文件是考题，生成器不许改；答案只允许碰 src/na_server_sup.rs。

use kfm_na::na_server_sup::{
    MARK_ALIVE, MARK_FAIL, MARK_SPAWNED, SupState, Verdict, ensure_script, exec_args, state_word,
    verdict_of,
};
use kfm_na::settings::{Backend, ServerEntry, SshFields, TunnelPorts};

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
        backend: Backend::NaServer,
    }
}

// ---- ensure 脚本：三段序钉死 ----

#[test]
fn spec_脚本_先探活() {
    let s = ensure_script();
    let probe = s.find("curl -s -m 2").expect("有探活");
    let alive = s.find(MARK_ALIVE).expect("有 ALIVE 标记");
    let build = s.find("cargo build").expect("有建造段");
    assert!(
        probe < build && alive < build,
        "探活+接管必须在建造之前——重启别人的进程 = 破坏 ExternalUp 语义"
    );
}

#[test]
fn spec_脚本_建造在拉起前() {
    let s = ensure_script();
    let build = s
        .find("cargo build --release -p na-server")
        .expect("有建造");
    let spawn = s.find("setsid nohup").expect("有拉起");
    assert!(build < spawn, "缺二进制先建再拉");
}

#[test]
fn spec_脚本_源码新即重建() {
    // 契约变更不许默默不生效（2026-09-21 负载判色：na-server 加 cores 键，
    // 老二进制在跑 = 新客户端永远收不到核数、负载轨白等一个口径）
    let s = ensure_script();
    assert!(
        s.contains("find crates/na-server/src crates/na-sys/src"),
        "判据必须覆盖 na-server 与 na-sys 两侧 src（体征契约在 na-sys 里）"
    );
    assert!(
        s.contains("-newer target/release/na-server"),
        "判据 = 源码比二进制新"
    );
    assert!(
        s.contains("[ ! -x target/release/na-server ] || [ -n \"$STALE\" ]"),
        "缺二进制与源码更新两条都要触发重建"
    );
    // 重建归重建，**绝不重启在跑的老进程**（别人会话挂它上面）
    let probe = s.find("curl -s -m 2").unwrap();
    let build = s.find("cargo build").unwrap();
    assert!(probe < build, "探活仍在重建之前（接管优先，不掐别人会话）");
    let spawn = s.find("setsid nohup").unwrap();
    let build2 = s.rfind("cargo build").unwrap();
    assert!(build2 < spawn, "重建后才能拉起（新娃用新二进制）");
}

#[test]
fn spec_脚本_拉起必须彻底_detach() {
    let s = ensure_script();
    assert!(
        s.contains("setsid nohup"),
        "setsid+nohup 双保险（ssh 断连不死）"
    );
    assert!(s.contains("</dev/null"), "stdin 脱钩");
    assert!(s.contains("&\n"), "后台化");
    // 裸 nohup & 随 shell 死的教训（redroid 接力实录）——setsid 必须在
    let line = s.lines().find(|l| l.contains("nohup")).unwrap();
    assert!(line.contains("setsid"), "拉起行必须带 setsid: {line}");
}

#[test]
fn spec_脚本_端口与绑定钉死() {
    let s = ensure_script();
    assert!(s.contains("NA_BIND=127.0.0.1:9021"), "只绑回环 9021");
    assert!(
        s.contains("http://127.0.0.1:9021/api/na/health"),
        "health 探活打回环"
    );
}

#[test]
fn spec_脚本_拉起后复检() {
    let s = ensure_script();
    let spawn = s.find("setsid nohup").unwrap();
    let recheck = s.rfind("curl -s -m 2").unwrap();
    let spawned = s.find(MARK_SPAWNED).unwrap();
    assert!(
        spawn < recheck && spawn < spawned,
        "拉起后必须复检才许报 SPAWNED——拉出死娃报假绿 = C 静默死点"
    );
}

#[test]
fn spec_脚本_失败路径有标记() {
    let s = ensure_script();
    // 每段失败都必须落 MARK_FAIL（cd 失败/建造失败/复检失败）
    assert!(
        s.matches(MARK_FAIL).count() >= 3,
        "失败标记至少三处（cd/建造/复检），实际 {}",
        s.matches(MARK_FAIL).count()
    );
}

// ---- exec 参数 ----

#[test]
fn spec_exec参数_不是隧道() {
    let a = exec_args(&srv("h", "u", "/k")).expect("合法条目出参");
    assert!(!a.iter().any(|x| x == "-N"), "exec 不要 -N（那是隧道的）");
    assert!(!a.iter().any(|x| x == "-L"), "exec 不要 -L");
    let tail: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        tail[tail.len() - 2..],
        ["bash", "-s"],
        "脚本走 stdin：bash -s 收尾"
    );
}

#[test]
fn spec_exec参数_安全与超时() {
    let a = exec_args(&srv("8.145.46.182", "root", "/k/id")).expect("出参");
    assert!(a.iter().any(|x| x == "BatchMode=yes"), "禁交互");
    assert!(
        a.iter().any(|x| x.starts_with("ConnectTimeout=")),
        "病态网络总超时（批模式 ssh 自身没有）"
    );
    assert!(
        a.windows(2).any(|w| w[0] == "-i" && w[1] == "/k/id"),
        "密钥路径"
    );
    assert!(a.iter().any(|x| x == "root@8.145.46.182"), "目的端");
}

#[test]
fn spec_exec参数_缺件全拒() {
    assert!(exec_args(&srv("", "root", "/k")).is_err(), "空 host");
    assert!(exec_args(&srv("h", "", "/k")).is_err(), "空 user");
    assert!(exec_args(&srv("h", "root", "")).is_err(), "空 key");
    let mut s = srv("h", "root", "/k");
    s.ssh.password = "secret".into();
    assert!(exec_args(&s).is_err(), "密码登录显式拒（与隧道同尺）");
}

// ---- verdict 解析 ----

#[test]
fn spec_verdict_三标记() {
    assert_eq!(verdict_of(MARK_ALIVE), Verdict::Alive);
    assert_eq!(verdict_of(MARK_SPAWNED), Verdict::Spawned);
    assert!(matches!(verdict_of(MARK_FAIL), Verdict::Failed(_)));
}

#[test]
fn spec_verdict_取最后标记() {
    // cargo 输出可能含任何字——以收尾标记为准
    let out = format!("noise\n{MARK_ALIVE}\nmore noise\n");
    assert_eq!(verdict_of(&out), Verdict::Alive);
    let out2 = format!("{MARK_FAIL}\n{MARK_SPAWNED}\n");
    assert_eq!(
        verdict_of(&out2),
        Verdict::Spawned,
        "最后标记赢（前面的 FAIL 是中途回声）"
    );
}

#[test]
fn spec_verdict_无标记是失败() {
    assert!(matches!(verdict_of(""), Verdict::Failed(_)));
    assert!(matches!(verdict_of("random garbage"), Verdict::Failed(_)));
}

// ---- 状态词 ----

#[test]
fn spec_状态词_五态() {
    assert_eq!(state_word(&SupState::Up), "自持在线");
    assert_eq!(state_word(&SupState::ExternalUp), "外部借用");
    assert_eq!(state_word(&SupState::Checking), "确认中");
    assert_eq!(state_word(&SupState::TunnelDown), "待隧道");
    assert_eq!(
        state_word(&SupState::Down {
            attempts: 0,
            last_error: "未启动".into()
        }),
        "未启动"
    );
    assert_eq!(
        state_word(&SupState::Down {
            attempts: 2,
            last_error: "x".into()
        }),
        "退避 ×2"
    );
}

#[test]
fn spec_状态发布_在途相不跳出卡面() {
    // 用户「我就算在 na 客户端里，也会看到它反复地重连」定案：ensure 探针
    // 每 15s 一次的**在途相**不许进快照——否则服务卡状态词每 15s 翻一次
    // 「自持在线 ↔ 确认中」，被读成反复重连（实测：同一秒里多次迁移）
    use kfm_na::na_server_sup::{SupState, publish_state, state_word};
    let up = SupState::ExternalUp;
    assert_eq!(
        publish_state(&up, SupState::Checking),
        up,
        "在途相不发布（保持上一个结果相）"
    );
    assert_eq!(
        publish_state(&SupState::Checking, SupState::Up),
        SupState::Up,
        "结果相照发"
    );
    assert_eq!(
        publish_state(&SupState::Checking, SupState::Checking),
        SupState::Checking,
        "首探前保持确认中（那是真话）"
    );
    let down = SupState::Down {
        attempts: 2,
        last_error: "x".into(),
    };
    assert_eq!(
        publish_state(&down, SupState::Checking),
        down,
        "失败相不被在途相盖掉"
    );
    assert_eq!(
        publish_state(&up, SupState::TunnelDown),
        SupState::TunnelDown
    );
    assert_eq!(state_word(&SupState::Checking), "确认中");
}
