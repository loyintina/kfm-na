//! BAR-171 钉组：attach 壳回收（killpg 整组灭）+ 日志面限流
//!
//! 病灶（2026-09-27/28 实录定罪）：ws 连接死只杀 `sh -c` 壳单 pid，
//! 孙进程（tmux 客户端）成孤儿继续附着 → 30 孤儿 attach + continuum
//! churn 拖垮 4 核机；fd 枯竭期 accept 无退避空转刷屏 99108 行/29GB。
//! 修复 = pty_sess kill 改 killpg（ESRCH 竞态窗回落单 pid）+
//! logcap 同款限流（前 3 直发 / 60s 窗 / 压制计数）。

use na_server::{logcap, pty_sess};

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
