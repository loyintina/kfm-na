//! wire_render.rs — 会话 wire jsonl 尾部 → 可读对话文本（会话池查看器
//! 一期，只读，BAR-163）。
//!
//! A 档纯函数：输入 tail 文本与最大事件数，输出多行可读文本。
//! 折叠规则：tool_call 记挂账，tool_result 配对成一行
//! 「调 name(参数摘要) → 结果首行摘要（截 60 字符）」；usage 全段聚合
//! 成末尾一条账行；done 收尾行；坏行计数容错不炸。

use std::collections::HashMap;

use serde_json::Value;

/// 渲染 jsonl 尾部 max_events 个事件为可读对话文本
pub fn render_tail(jsonl: &str, max_events: usize) -> String {
    let lines: Vec<&str> = jsonl.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(max_events);
    let mut out: Vec<String> = Vec::new();
    let mut pending: HashMap<String, (String, String)> = HashMap::new();
    let mut bad = 0usize;
    let mut rounds = 0u64;
    let mut prompt_sum = 0u64;
    let mut completion_sum = 0u64;
    for line in &lines[start..] {
        let ev: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                bad += 1;
                continue;
            }
        };
        match ev.get("type").and_then(Value::as_str).unwrap_or("") {
            "user_msg" => {
                let content = ev.get("content").and_then(Value::as_str).unwrap_or("");
                out.push(format!("用户： {content}"));
            }
            "model_msg" => {
                let content = ev.get("content").and_then(Value::as_str).unwrap_or("");
                if !content.trim().is_empty() {
                    out.push(format!("agent： {content}"));
                }
            }
            "tool_call" => {
                let id = ev
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = ev
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string();
                let args = ev.get("arguments").and_then(Value::as_str).unwrap_or("");
                pending.insert(id, (name.clone(), arg_summary(&name, args)));
            }
            "tool_result" => {
                let id = ev.get("id").and_then(Value::as_str).unwrap_or("");
                let name = ev.get("name").and_then(Value::as_str).unwrap_or("?");
                let output = ev.get("output").and_then(Value::as_str).unwrap_or("");
                let summary = result_summary(output);
                match pending.remove(id) {
                    Some((pname, pargs)) => {
                        out.push(format!("调 {pname}({pargs}) → {summary}"));
                    }
                    None => {
                        out.push(format!("· {name} 结果（未配对）: {summary}"));
                    }
                }
            }
            "usage" => {
                rounds += 1;
                prompt_sum += ev.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
                completion_sum += ev
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
            "done" => {
                let reason = ev.get("reason").and_then(Value::as_str).unwrap_or("?");
                let n = ev.get("rounds").and_then(Value::as_u64).unwrap_or(0);
                out.push(format!("── 结束（{reason}，{n} 轮）"));
            }
            _ => {}
        }
    }
    for (_id, (name, args)) in pending {
        out.push(format!("调 {name}({args}) → （未见结果）"));
    }
    if bad > 0 {
        out.push(format!("（{bad} 行无法解析，已略过）"));
    }
    if rounds > 0 {
        out.push(format!(
            "账： {rounds}轮 · prompt {prompt_sum} · completion {completion_sum}"
        ));
    }
    if out.is_empty() {
        return "（空会话）".to_string();
    }
    out.join("\n")
}

/// tool_call 参数摘要：优先 path，其次 command；write_file 带内容字节数
fn arg_summary(name: &str, args: &str) -> String {
    let v: Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(_) => return clip(args, 40),
    };
    let pick = |key: &str| v.get(key).and_then(Value::as_str);
    if let Some(path) = pick("path") {
        if name == "write_file" {
            let bytes = pick("content").map(str::len).unwrap_or(0);
            return format!("{path}，{bytes} 字节");
        }
        return clip(path, 40);
    }
    if let Some(cmd) = pick("command") {
        return clip(cmd, 40);
    }
    clip(args, 40)
}

/// 结果首行摘要（截 60 字符）
fn result_summary(output: &str) -> String {
    let first = output.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    clip(first.trim(), 60)
}

/// 字符边界安全截断，超长补省略号
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}
