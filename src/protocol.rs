//! protocol.rs — terminal-pty ws 协议编解码（A 档考题 tests/protocol_spec.rs 的答案区）
//!
//! 协议真相源：kfmv4/src/server/ws-server.ts handleMessage。
//! 封套：{type, payload, timestamp}。
//!
//! 纪律：本文件是「答案」，只允许为通过考题而写；考题（tests/protocol_spec.rs）不许动。

use serde_json::{Value, json};

/// 客户端 → 服务端消息
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMsg {
    Open {
        cwd: Option<String>,
        command: Option<String>,
        tag: Option<String>,
    },
    Input {
        session_id: String,
        input: String,
    },
    Resize {
        session_id: String,
        cols: u32,
        rows: u32,
    },
    Close {
        session_id: String,
    },
}

/// 服务端 → 客户端消息
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMsg {
    Opened {
        session_id: String,
        tag: Option<String>,
    },
    Output {
        session_id: String,
        data: String,
    },
    Exit {
        session_id: String,
        code: i32,
    },
    Error {
        message: String,
    },
    /// 服务端 30s 心跳的应用层 ping（payload null）
    Ping,
    /// 本客户端不关心的类型（ack/snapshot/tmux-*…），前向兼容收纳
    Unknown {
        type_name: String,
    },
}

/// 协议错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub message: String,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "协议错误: {}", self.message)
    }
}

impl std::error::Error for ProtocolError {}

fn err(msg: impl Into<String>) -> ProtocolError {
    ProtocolError {
        message: msg.into(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1) // 系统时钟异常时兜底（考题要求 timestamp > 0）
}

/// 编码客户端消息为封套 JSON 串
pub fn encode_client(msg: &ClientMsg) -> String {
    let (ty, payload) = match msg {
        ClientMsg::Open { cwd, command, tag } => {
            // 全 None → 空对象（serde skip_serializing_if 语义，手写不引 derive）
            let mut p = serde_json::Map::new();
            if let Some(v) = cwd {
                p.insert("cwd".into(), json!(v));
            }
            if let Some(v) = command {
                p.insert("command".into(), json!(v));
            }
            if let Some(v) = tag {
                p.insert("tag".into(), json!(v));
            }
            ("terminal-open", Value::Object(p))
        }
        ClientMsg::Input { session_id, input } => (
            "terminal-input",
            json!({"sessionId": session_id, "input": input}),
        ),
        ClientMsg::Resize {
            session_id,
            cols,
            rows,
        } => (
            "terminal-resize",
            json!({"sessionId": session_id, "cols": cols, "rows": rows}),
        ),
        ClientMsg::Close { session_id } => ("terminal-close", json!({"sessionId": session_id})),
    };
    json!({"type": ty, "payload": payload, "timestamp": now_ms()}).to_string()
}

/// 取 payload 上的必填字符串字段
fn req_str<'v>(p: &'v Value, ty: &str, key: &str) -> Result<&'v str, ProtocolError> {
    p.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| err(format!("{ty} 缺字段 {key}")))
}

/// 解码服务端封套 JSON 串
pub fn decode_server(raw: &str) -> Result<ServerMsg, ProtocolError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| err(format!("非合法 JSON: {e}")))?;
    err("缺 type 字段");
    let ty = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| err("缺 type 字段"))?;
    let p = v.get("payload").cloned().unwrap_or(Value::Null);
    match ty {
        "terminal-opened" => Ok(ServerMsg::Opened {
            session_id: req_str(&p, ty, "sessionId")?.to_string(),
            // tag 缺省或显式 null 都为 None（ws-server.ts:182 tag 可能 undefined）
            tag: p.get("tag").and_then(Value::as_str).map(str::to_string),
        }),
        "terminal-output" => Ok(ServerMsg::Output {
            session_id: req_str(&p, ty, "sessionId")?.to_string(),
            data: req_str(&p, ty, "data")?.to_string(),
        }),
        "terminal-exit" => Ok(ServerMsg::Exit {
            session_id: req_str(&p, ty, "sessionId")?.to_string(),
            code: p
                .get("code")
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
                .ok_or_else(|| err("exit 缺字段 code 或类型非数字"))?,
        }),
        "error" => Ok(ServerMsg::Error {
            message: req_str(&p, ty, "message")?.to_string(),
        }),
        "ping" => Ok(ServerMsg::Ping),
        other => Ok(ServerMsg::Unknown {
            type_name: other.to_string(),
        }),
    }
}
