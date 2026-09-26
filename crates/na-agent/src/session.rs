//! session.rs — 会话存储（用户模型，不许走样）：
//! <session_root>/<线名>/<NNNN-会话>.jsonl，append-only 一事件一行，
//! 每行带 ts 与 seq；usage 账从第一轮 API 起逐轮落（未来 context 占用
//! 与交接协议的预埋）。线文件夹即花名册——不搞注册表。
//!
//! 全部经 Host 读写（核心平台无关铁律）；NNNN 序号 = 扫目录现有
//! NNNN-*.jsonl 取 max+1。

use crate::dialect::{Message, ToolCall, Usage};
use crate::host::Host;

/// 事件类型词表（一事件一行，type 字段取值）。
pub mod ev {
    pub const USER_MSG: &str = "user_msg";
    pub const MODEL_MSG: &str = "model_msg";
    pub const TOOL_CALL: &str = "tool_call";
    pub const TOOL_RESULT: &str = "tool_result";
    pub const USAGE: &str = "usage";
    pub const DONE: &str = "done";
}

pub fn line_dir(root: &str, line: &str) -> String {
    format!("{root}/{line}")
}

/// 线名合法性（路径组件即花名册，注入/穿越一律拒）：ASCII [A-Za-z0-9_-]+
pub fn valid_line_name(line: &str) -> bool {
    !line.is_empty()
        && line.len() <= 64
        && line
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 花名册 = 列目录（信箱是信件面不是 agent 线，除外）。
/// Host 面没有 list_dirs——用 read 线目录试探会制造幽灵线，故 lines
/// 枚举在 daemon 壳（na-agentd）用 std::fs 直接列 session_root。
pub const MAILBOX_DIR: &str = "信箱";

/// 扫线目录找下一个会话序号（NNNN-*.jsonl 取 max+1；无 = 1）。
pub fn next_session_seq(host: &dyn Host, dir: &str) -> Result<u32, String> {
    let mut max = 0u32;
    for name in host.list_files(dir)? {
        if let Some(n) = parse_seq(&name) {
            max = max.max(n);
        }
    }
    Ok(max + 1)
}

fn parse_seq(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".jsonl")?;
    let (num, _) = stem.split_once('-')?;
    if num.len() == 4 && num.bytes().all(|b| b.is_ascii_digit()) {
        num.parse().ok()
    } else {
        None
    }
}

pub fn session_path(dir: &str, seq: u32) -> String {
    format!("{dir}/{seq:04}-会话.jsonl")
}

/// 最新会话（NNNN 最大者）的路径；无线/无会话 = None。
pub fn latest_session(host: &dyn Host, dir: &str) -> Result<Option<String>, String> {
    let mut best: Option<(u32, String)> = None;
    for name in host.list_files(dir)? {
        if let Some(n) = parse_seq(&name)
            && best.as_ref().is_none_or(|(m, _)| n > *m)
        {
            best = Some((n, format!("{dir}/{name}")));
        }
    }
    Ok(best.map(|(_, p)| p))
}

/// append-only 会话写入器：seq 单调，ts 取自 Host 钟。
pub struct SessionWriter<'h, H: Host> {
    host: &'h H,
    path: String,
    seq: u64,
}

impl<'h, H: Host> SessionWriter<'h, H> {
    /// 续写既有文件时 seq 从已有行数起（append-only 不覆写）。
    pub fn open(host: &'h H, path: &str) -> Self {
        let seq = host
            .read_file(path)
            .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count() as u64)
            .unwrap_or(0);
        Self {
            host,
            path: path.to_string(),
            seq,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn emit(&mut self, event: serde_json::Value) -> Result<(), String> {
        self.seq += 1;
        // seq/ts 在前，事件体在后（serde_json Map 保插入序）
        let mut merged = serde_json::Map::new();
        merged.insert("seq".into(), serde_json::json!(self.seq));
        merged.insert("ts".into(), serde_json::json!(self.host.now_rfc3339()));
        match event {
            serde_json::Value::Object(m) => merged.extend(m),
            _ => unreachable!("事件恒为对象"),
        }
        self.host
            .append_line(&self.path, &serde_json::Value::Object(merged).to_string())
    }

    pub fn user_msg(&mut self, content: &str) -> Result<(), String> {
        self.emit(serde_json::json!({"type": ev::USER_MSG, "content": content}))
    }

    pub fn model_msg(
        &mut self,
        round: u32,
        content: Option<&str>,
        tool_calls: &[ToolCall],
    ) -> Result<(), String> {
        self.emit(serde_json::json!({
            "type": ev::MODEL_MSG,
            "round": round,
            "content": content,
            "tool_calls": tool_calls,
        }))
    }

    pub fn tool_call(
        &mut self,
        round: u32,
        id: &str,
        name: &str,
        arguments: &str,
    ) -> Result<(), String> {
        self.emit(serde_json::json!({
            "type": ev::TOOL_CALL,
            "round": round,
            "id": id,
            "name": name,
            "arguments": arguments,
        }))
    }

    pub fn tool_result(
        &mut self,
        round: u32,
        id: &str,
        name: &str,
        output: &str,
    ) -> Result<(), String> {
        self.emit(serde_json::json!({
            "type": ev::TOOL_RESULT,
            "round": round,
            "id": id,
            "name": name,
            "output": output,
        }))
    }

    /// usage 账：每轮 API 返回即落（第一轮也不许漏——预埋 context 占用账）
    pub fn usage(&mut self, round: u32, u: Usage) -> Result<(), String> {
        self.emit(serde_json::json!({
            "type": ev::USAGE,
            "round": round,
            "prompt_tokens": u.prompt_tokens,
            "completion_tokens": u.completion_tokens,
            "total_tokens": u.total_tokens,
        }))
    }

    pub fn done(&mut self, rounds: u32, reason: &str, reply: &str) -> Result<(), String> {
        self.emit(serde_json::json!({
            "type": ev::DONE,
            "rounds": rounds,
            "reason": reason,
            "reply": reply,
        }))
    }
}

/// 回放：会话 jsonl → messages（跨 send 续聊的上下文重建）。
/// 只消费 user_msg / model_msg / tool_result；usage/done 是账不是上下文。
pub fn replay_messages(host: &dyn Host, path: &str) -> Result<Vec<Message>, String> {
    let text = match host.read_file(path) {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()), // 不存在 = 空史（新会话）
    };
    let mut out = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("{path} 第 {} 行非合法 JSON: {e}", ln + 1))?;
        match v.get("type").and_then(|t| t.as_str()) {
            Some(ev::USER_MSG) => {
                let c = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
                out.push(Message::user(c));
            }
            Some(ev::MODEL_MSG) => {
                let content = v
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(str::to_string);
                let tool_calls: Vec<ToolCall> = v
                    .get("tool_calls")
                    .and_then(|t| serde_json::from_value(t.clone()).ok())
                    .unwrap_or_default();
                out.push(Message::assistant(content, tool_calls));
            }
            Some(ev::TOOL_RESULT) => {
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let output = v.get("output").and_then(|x| x.as_str()).unwrap_or("");
                out.push(Message::tool(id, output.to_string()));
            }
            _ => {} // usage / done / 未知类型：账，不进上下文
        }
    }
    Ok(out)
}
