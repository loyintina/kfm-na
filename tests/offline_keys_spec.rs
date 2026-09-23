//! 断线输入暂存考题（A 档，BAR-135 用户拍板「断联影响最小化」）：
//! 远程死会话 + 隧道不可用期的击键进队，Opened 回冲——不盲孵必死
//! 连接、不丢输入、不重复。
//! 变异抽检：①容量闸摘除（不丢最旧）必须咬；②drain 不保序/不清零
//! 必须咬；③should_hold 判据放宽（如本地也收）必须咬。

use kfm_na::offline_keys::{CAP_BYTES, OfflineKeys, should_hold};

#[test]
fn spec_bar135_暂存_收冲保序() {
    let mut q = OfflineKeys::new();
    assert!(q.is_empty());
    q.push("ls".to_string());
    q.push("\r".to_string());
    q.push("echo hi".to_string());
    assert_eq!(q.pending_bytes(), 2 + 1 + 7);
    assert!(!q.is_empty());
    let got = q.drain();
    assert_eq!(got, vec!["ls", "\r", "echo hi"]); // 保序 = 击键序即回冲序
    assert!(q.is_empty());
    assert_eq!(q.pending_bytes(), 0); // 冲完清零（状态行字节数跟着灭）
}

#[test]
fn spec_bar135_暂存_容量闸丢最旧() {
    let mut q = OfflineKeys::new();
    let big = "x".repeat(CAP_BYTES); // 一条顶满
    q.push(big.clone());
    q.push("tail".to_string()); // 这条进来，最旧的那条必须让位
    assert_eq!(q.dropped(), 1, "超容量必须丢最旧且记账");
    assert!(q.pending_bytes() <= CAP_BYTES);
    let got = q.drain();
    assert_eq!(got, vec!["tail"]); // 留新不留旧
    // 丢账不随 drain 清零（它是账不是态）
    assert_eq!(q.dropped(), 1);
}

#[test]
fn spec_bar135_收编判定_只收远程死会话隧道断() {
    // 真值表：session_over × is_remote × tunnel_usable
    assert!(should_hold(true, true, false)); // 收编：远程死会话隧道断
    assert!(!should_hold(true, true, true)); // 隧道通 = 走 kick_reconnect 原路
    assert!(!should_hold(true, false, false)); // 本地死会话与隧道无关
    assert!(!should_hold(false, true, false)); // 活会话不收
    assert!(!should_hold(false, false, false));
    assert!(!should_hold(false, true, true));
    assert!(!should_hold(true, false, true));
    assert!(!should_hold(false, false, true));
}
