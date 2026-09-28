//! BAR-171 钉组：attach 壳回收（killpg 整组灭）+ 日志面限流
//!
//! 病灶（2026-09-27/28 实录定罪）：ws 连接死只杀 `sh -c` 壳单 pid，
//! 孙进程（tmux 客户端）成孤儿继续附着 → 30 孤儿 attach + continuum
//! churn 拖垮 4 核机；fd 枯竭期 accept 无退避空转刷屏 99108 行/29GB。
//! 修复 = pty_sess kill 改 killpg（ESRCH 竞态窗回落单 pid）+
//! logcap 同款限流（前 3 直发 / 60s 窗 / 压制计数）。

use na_server::{logcap, pty_sess, state, wsterm};

/// 数进程组成员（/proc 直读，不借 pgrep——考题环境最小依赖）
fn group_members(pgid: i32) -> Vec<i32> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap() {
        let Ok(e) = e else { continue };
        let name = e.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        // /proc/<pid>/stat 字段 5 = pgrp；comm 可能含空格/括号，
        // 取最后一个 ')' 之后的字段序列才稳
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some(rp) = stat.rfind(')') else { continue };
        let fields: Vec<&str> = stat[rp + 1..].split_whitespace().collect();
        // ')' 后第 1 字段 = state，第 2 = ppid，第 3 = pgrp
        if fields.len() >= 3 && fields[2].parse::<i32>().ok() == Some(pgid) {
            out.push(pid);
        }
    }
    out
}

#[test]
fn spec_bar171_kill_整组灭_孙进程不留孤儿() {
    // `trap '' HUP` + `sleep 30 & wait`：壳（sh -c）+ 孙进程 sleep 同组。
    // **孙进程必须 SIGHUP 免疫**——判卷陷阱实录（BAR-171 变异①漏杀）：
    // 普通 sleep 不需要 killpg 也会死（壳是会话长，死了内核给前台组
    // 发 SIGHUP），单 pid 杀的变异照样全组灭 = 钉变瞎子。tmux 客户端
    // 的真实行为就是吃 SIGHUP 不死（detach 续活——孤儿附着的机制），
    // 所以模型孙进程用 trap '' HUP 复刻同款免疫。sleep 30 自带寿限——
    // 考题中途炸了也不留野进程
    let (sess, _reader) =
        pty_sess::spawn(None, Some("trap '' HUP; sleep 30 & wait"), 80, 24).expect("spawn 失败");
    std::thread::sleep(std::time::Duration::from_millis(600)); // 等 setsid + 孙进程起
    let pgid = sess.child.lock().unwrap().pgid();

    let before = group_members(pgid);
    assert!(
        before.len() >= 2,
        "杀前组里至少要有壳+孙两个成员，实际 {before:?}（pgid={pgid}）"
    );

    sess.child.lock().unwrap().kill().expect("kill 失败");
    // 收尸：kill 是即发的，但子壳变僵尸要等 waitpid（生产上这是
    // wsterm 等死线程的活，裸考 pty_sess 得自己收——僵尸也是
    // /proc 成员，不收 = 自己骗自己「幸存者还在」）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        {
            let mut c = sess.child.lock().unwrap();
            let _ = c.try_wait();
        }
        let left = group_members(pgid);
        if left.is_empty() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            // 带上幸存者身份再炸——复盘要看它是谁
            let who: Vec<String> = left
                .iter()
                .map(|p| {
                    let comm = std::fs::read_to_string(format!("/proc/{p}/comm"))
                        .map(|c| c.trim().to_string())
                        .unwrap_or_else(|_| "?".into());
                    let stat =
                        std::fs::read_to_string(format!("/proc/{p}/stat")).unwrap_or_default();
                    let state = stat.rfind(')').and_then(|rp| {
                        stat[rp + 1..].split_whitespace().next().map(str::to_string)
                    });
                    format!("{p}({comm}/{state:?})")
                })
                .collect();
            panic!(
                "kill 3s 后组里还有幸存者 {who:?}（child pid={pgid}）——孙进程成孤儿（BAR-171 病灶复发）"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn spec_bar171_logcap_should_emit_真值表() {
    // 前 3 次必发（第一现场可见性）
    assert!(logcap::should_emit(1, 0));
    assert!(logcap::should_emit(2, 0));
    assert!(logcap::should_emit(3, 0));
    // 第 4 次起：60s 窗内压制
    assert!(!logcap::should_emit(4, 0));
    assert!(!logcap::should_emit(4, 59));
    assert!(!logcap::should_emit(1000, 30));
    // 窗满出闸（汇总上场）
    assert!(logcap::should_emit(4, 60));
    assert!(logcap::should_emit(99108, 61));
}

#[test]
fn spec_bar171_logcap_throttled_前三直发_其后压制() {
    // 进程级共享表——key 带考题名防撞（并行考题互不染指）
    let key = "spec_bar171_throttled";
    assert!(logcap::throttled(key, "第 1 条"));
    assert!(logcap::throttled(key, "第 2 条"));
    assert!(logcap::throttled(key, "第 3 条"));
    assert!(!logcap::throttled(key, "第 4 条（应被压制）"));
    assert!(!logcap::throttled(key, "第 5 条（应被压制）"));
}

/// 接线守卫（源码级）：wsterm 的发送必须过停摆判死超时，pty kill
/// 必须是 killpg——两处拆一个，僵尸链就复活
#[test]
fn spec_bar171_停摆判死与killpg_接线守卫() {
    let wsterm =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/wsterm.rs")).unwrap();
    assert!(
        wsterm.contains("NA_SEND_STALL_SECS")
            && wsterm.contains("tokio::time::timeout(stall, fut)"),
        "wsterm 发送必须包停摆判死超时（NA_SEND_STALL_SECS + timeout）"
    );
    let pty =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/pty_sess.rs")).unwrap();
    assert!(
        pty.contains("Pid::from_raw(-self.pid.as_raw())"),
        "pty kill 必须杀整组（负 pid = killpg），只杀壳单 pid = BAR-171 孤儿复活"
    );
    let main =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")).unwrap();
    assert!(
        main.contains("Duration::from_millis(200)"),
        "accept 失败必须 200ms 退避——无退避 continue = fd 枯竭期空转刷屏"
    );
}

// ---- BAR-171 翻案·修复②：心跳租约（协议级 ping/pong 回执账）----

#[test]
fn spec_bar171_lease_expired_真值表() {
    use std::time::Duration;
    let lease = Duration::from_secs(90);
    assert!(!wsterm::lease_expired(Duration::from_secs(0), lease));
    assert!(!wsterm::lease_expired(Duration::from_secs(89), lease));
    assert!(
        wsterm::lease_expired(Duration::from_secs(90), lease),
        "到点即满"
    );
    assert!(wsterm::lease_expired(Duration::from_secs(3600), lease));
}

/// 哑客收尸 / 活客续命对考（B 档胶水全真链）：裸 TCP 手工 ws 握手后
/// 装死（不 pong）→ 租约期满必须被收；tungstenite 真客户端（自动
/// pong，conn.rs 心跳注释实锤）→ 两个租约期后必须还活着且照常收
/// 应用层 ping。env 旋钮拧到考题档：ping 1s / 租约 3s。
#[tokio::test]
async fn spec_bar171_心跳租约_哑客收尸_活客续命() {
    use futures_util::StreamExt as _;
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    unsafe {
        std::env::set_var("NA_PING_SECS", "1");
        std::env::set_var("NA_LEASE_SECS", "3");
    }

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let registry = std::sync::Arc::new(state::Registry::new());
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = listener.accept().await else {
                break;
            };
            let reg = std::sync::Arc::clone(&registry);
            tokio::spawn(wsterm::handle(s, reg));
        }
    });

    // —— 相一：哑客（手工握手后装死）租约期满必须被收 ——
    let mut raw = tokio::net::TcpStream::connect(addr).await.unwrap();
    raw.write_all(
        b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
    )
    .await
    .unwrap();
    // 读走 101 头（到 \r\n\r\n 为止）
    let mut buf = vec![0u8; 4096];
    let mut head = Vec::new();
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = tokio::time::timeout(Duration::from_secs(3), raw.read(&mut buf))
            .await
            .expect("握手响应 3s 没到")
            .unwrap();
        assert!(n > 0, "握手期连接就断了");
        head.extend_from_slice(&buf[..n]);
    }
    // 装死：只读不应（读到 ping 帧也绝不回 pong）——观察口 = 连接
    // 被服务器主动断开。**绝对期限兜底**（判卷陷阱第四例预备：每圈
    // 新建 timeout 会被 1s 一拍的 ping 帧喂得永远不超——变异“拆租约”
    // 时考题必须有死线，不许挂死）
    let t0 = Instant::now();
    let overall = t0 + Duration::from_secs(12);
    let mut closed = false;
    loop {
        let left = overall.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break; // 12s 都没被收 = 租约失效
        }
        match tokio::time::timeout(left, raw.read(&mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) => {
                closed = true;
                break;
            }
            Ok(Ok(_)) => continue, // ping/close 帧照吞，不回应
            Err(_) => break,
        }
    }
    let lived = t0.elapsed();
    assert!(closed, "哑客 12s 内必须被租约收尸（NA_LEASE_SECS=3）");
    assert!(
        lived >= Duration::from_secs(2),
        "哑客至少活满一个租约档才被收（防误杀即收），实际 {lived:?}"
    );

    // —— 相二：活客（tungstenite 自动 pong）两个租约期后还活着 ——
    let (mut ws, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}/"))
        .await
        .expect("活客握手失败");
    let mut app_pings = 0u32;
    let deadline = Instant::now() + Duration::from_secs(5); // > 1.5 个租约期
    while Instant::now() < deadline {
        let left = deadline - Instant::now();
        match tokio::time::timeout(left, ws.next()).await {
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))) => app_pings += 1,
            Ok(Some(Ok(_))) => {}
            Ok(None) | Ok(Some(Err(_))) | Err(_) => break,
        }
    }
    assert!(
        app_pings >= 3,
        "活客 5s 内应收 ≥3 个应用层 ping（1s 一拍）——连接被误杀或心跳没发，实际 {app_pings}"
    );
}

/// 接线守卫（源码级）：wsterm 必须有租约判死 + 协议级 ping——拆掉
/// 任一件，「安静窗永不停摆」盲区复活（BAR-171 翻案病灶）
#[test]
fn spec_bar171_心跳租约_接线守卫() {
    let wsterm_src =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/wsterm.rs")).unwrap();
    assert!(
        wsterm_src.contains("NA_LEASE_SECS")
            && wsterm_src.contains("lease_expired(last_rx.elapsed()"),
        "wsterm 必须有租约判死接线（NA_LEASE_SECS + lease_expired(last_rx...)）"
    );
    assert!(
        wsterm_src.contains("Message::Ping("),
        "必须发协议级 ws Ping——应用层 ServerMsg::Ping 没有回执，哑客永远抓不到"
    );
}
