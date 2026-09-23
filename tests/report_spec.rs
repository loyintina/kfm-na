//! 报表考题（A 档）：设备/实例标识（BAR-134）——field-reports.log 是多设备
//! 混流，行尾 [arch/pid] 是判读第一分道闸。纯逻辑钉死格式，变异抽检：
//! 摘掉 stamp 必须咬。

use kfm_na::report::stamp_msg;

#[test]
fn spec_bar134_报表行_挂设备实例标识() {
    // 1. 原消息完整保留在前缀
    let out = stamp_msg("隧道 Up");
    assert!(out.starts_with("隧道 Up "), "原消息必须在前: {out}");
    // 2. 行尾是 [arch/pid] 形态：方括号 + 斜杠 + 数字 pid
    let tail = out.strip_prefix("隧道 Up ").unwrap();
    assert!(
        tail.starts_with('[') && tail.ends_with(']'),
        "标识必须方括号包裹: {tail}"
    );
    let inner = &tail[1..tail.len() - 1];
    let (arch, pid) = inner.split_once('/').expect("标识必须 arch/pid 两段");
    assert!(!arch.is_empty(), "arch 不许为空");
    assert!(
        !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()),
        "pid 必须是数字: {pid}"
    );
    // 3. pid 必须是本进程真 pid（多实例分道靠它）
    assert_eq!(pid, std::process::id().to_string());
    // 4. arch 必须等于编译目标（redroid/手机同 arch 时靠 pid 分）
    assert_eq!(arch, std::env::consts::ARCH);
    // 5. 空消息也挂（心跳类短行不豁免）
    let out2 = stamp_msg("");
    assert!(out2.starts_with('['), "空消息标识直接打头: {out2}");
}

#[test]
fn spec_bar134_标识_转义后仍存活() {
    // 落盘链：msg 先进 escape_json 再进 JSON——标识是纯 ASCII，必须原样穿过
    let msg = stamp_msg("quote\"and\\slash");
    let body = format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        kfm_na::report::escape_json("stage"),
        kfm_na::report::escape_json(&msg)
    );
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let got = v["msg"].as_str().unwrap();
    assert!(
        got.starts_with("quote\"and\\slash ["),
        "转义不得伤消息体: {got}"
    );
    assert!(got.ends_with(']'), "标识必须在行尾: {got}");
}
