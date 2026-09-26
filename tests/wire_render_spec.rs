//! wire_render 考题（A 档，BAR-163 会话池一期）：wire jsonl 尾部 → 可读
//! 对话文本。钉死五类事件形态、tool 配对折叠、usage 聚合账行、尾部 N
//! 截断、坏行容错。
//!
//! 变异抽检：①摘 usage 聚合（账行消失 = 用户看不到这轮烧了多少钱）
//! 必须咬；②tool_result 不配对（一次调用摊两行）必须咬；③坏行直接
//! panic 必须咬。

use kfm_na::wire_render::render_tail;

fn ev(s: &str) -> String {
    s.to_string()
}

/// 一段典型 wire 流：用户问 → 账 → agent 说 → 调工具 → 回 → 账 → 完
fn sample() -> String {
    [
        ev(r#"{"type":"user_msg","content":"读一下 bugs.md","seq":1}"#),
        ev(r#"{"type":"usage","round":1,"prompt_tokens":540,"completion_tokens":130}"#),
        ev(r#"{"type":"model_msg","content":"先读文件。","round":1}"#),
        ev(r#"{"type":"tool_call","id":"c1","name":"read_file","arguments":"{\"path\":\"/root/bugs.md\"}","round":1}"#),
        ev(r##"{"type":"tool_result","id":"c1","name":"read_file","output":"# bugs.md 标题\n第二行","round":1}"##),
        ev(r#"{"type":"usage","round":2,"prompt_tokens":800,"completion_tokens":70}"#),
        ev(r#"{"type":"done","reason":"end_turn","rounds":2}"#),
    ]
    .join("\n")
}

#[test]
fn spec_bar163_渲染_五类事件各就其位() {
    let text = render_tail(&sample(), 100);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "用户： 读一下 bugs.md");
    assert_eq!(lines[1], "agent： 先读文件。");
    assert_eq!(lines[2], "调 read_file(/root/bugs.md) → # bugs.md 标题");
    assert_eq!(lines[3], "── 结束（end_turn，2 轮）");
    assert!(lines[4].starts_with("账： "), "账行收尾: {text}");
}

#[test]
fn spec_bar163_渲染_tool折叠_参数摘要与结果截断() {
    // write_file 带字节数；command 参数走 command 键；长结果截 60 字符
    let long = "x".repeat(80);
    let jsonl = [
        ev(r#"{"type":"tool_call","id":"a","name":"write_file","arguments":"{\"path\":\"/tmp/x.md\",\"content\":\"hello\"}"}"#),
        ev(r#"{"type":"tool_result","id":"a","name":"write_file","output":"ok"}"#),
        ev(r#"{"type":"tool_call","id":"b","name":"run_command","arguments":"{\"command\":\"ls -la /root\"}"}"#),
        format!(r#"{{"type":"tool_result","id":"b","name":"run_command","output":"{long}"}}"#),
    ]
    .join("\n");
    let text = render_tail(&jsonl, 100);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "调 write_file(/tmp/x.md，5 字节) → ok");
    let expect_summary = "x".repeat(60);
    assert_eq!(
        lines[1],
        format!("调 run_command(ls -la /root) → {expect_summary}…")
    );
}

#[test]
fn spec_bar163_渲染_usage聚合成一条账行() {
    let text = render_tail(&sample(), 100);
    let bill: Vec<&str> = text.lines().filter(|l| l.starts_with("账： ")).collect();
    assert_eq!(bill.len(), 1, "usage 事件只出一条聚合账行: {text}");
    assert_eq!(bill[0], "账： 2轮 · prompt 1340 · completion 200");
}

#[test]
fn spec_bar163_渲染_尾部n截断() {
    // max_events=2 只取 done 与倒数第二条 usage → 无用户行
    let text = render_tail(&sample(), 2);
    assert!(!text.contains("用户："), "只留尾部 2 事件: {text}");
    assert!(text.contains("── 结束（end_turn，2 轮）"));
    assert_eq!(text.lines().filter(|l| l.starts_with("账： ")).count(), 1);
}

#[test]
fn spec_bar163_渲染_坏行容错与未配对兜底() {
    let jsonl = [
        ev(r#"{"type":"user_msg","content":"hi"}"#),
        ev("这不是 json"),
        ev(r#"{"type":"tool_result","id":"ghost","name":"read_file","output":"孤儿结果"}"#),
        ev(r#"{"type":"tool_call","id":"c9","name":"read_file","arguments":"{\"path\":\"/x\"}"}"#),
    ]
    .join("\n");
    let text = render_tail(&jsonl, 100);
    assert!(text.contains("用户： hi"));
    assert!(text.contains("· read_file 结果（未配对）: 孤儿结果"));
    assert!(text.contains("调 read_file(/x) → （未见结果）"));
    assert!(text.contains("（1 行无法解析，已略过）"));
}

#[test]
fn spec_bar163_渲染_空输入给占位() {
    assert_eq!(render_tail("", 100), "（空会话）");
    assert_eq!(render_tail("\n\n", 100), "（空会话）");
}
