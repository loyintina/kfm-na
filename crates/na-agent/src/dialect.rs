//! dialect.rs — OpenAI 兼容方言（bigmodel-coding glm / deepseek /
//! kimi-code k3 同吃这份——BAR-164 起 kimi always-thinking 响应形状
//! （content 空 + reasoning_content + usage.completion_tokens_details）
//! 与 reasoning+tool_calls 共存都在本册容忍面内）。纯 JSON 进出
//! （A 档），传输在 httpc.rs。
//!
//! 请求：POST {base_url}/chat/completions，非流式（v1 判卷要的是
//! content/tool_calls/usage 三段全量，不需要 SSE 增量），messages + tools。
//! 响应：choices[0].message.{content, tool_calls[]} + usage。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String {
    "function".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    /// OpenAI 兼容线协议：arguments 是 JSON 文本（不是对象）
    pub arguments: String,
}

/// 会话消息（回放重建与请求体共用同一类型 = 单源）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: String,
    /// assistant 带 tool_calls 时 content 可为 null
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// role=tool 时回指
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: &str) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn system(content: &str) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content,
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn tool(call_id: &str, content: String) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }
}

/// 一轮 API 的产物（方言解析的出口）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChatReply {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// 工具规格（发给模型的 schema 面）。
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: ToolFn,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFn {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
}

/// 构造请求体（A 档纯函数）。stream 恒 false——v1 非流式。
/// max_tokens：Some 入体 / None 键不出现（api_key 老路逐字节不变；
/// kimi-code thinking 模型必须给足——BAR-164，providers 裁决喂入）
pub fn build_request(
    model: &str,
    messages: &[Message],
    tools: &[ToolSpec],
    max_tokens: Option<u32>,
) -> String {
    let mut v = serde_json::json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "stream": false,
    });
    if let Some(mt) = max_tokens {
        v["max_tokens"] = serde_json::json!(mt);
    }
    v.to_string()
}

/// 解析响应体（A 档纯函数）。只认 choices[0]；缺 choices/坏 JSON = Err。
pub fn parse_response(body: &str) -> Result<ChatReply, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("响应非合法 JSON: {e}"))?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知上游错误");
        return Err(format!("上游报错: {msg}"));
    }
    let msg = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .ok_or_else(|| "响应缺 choices[0].message".to_string())?;
    let content = msg
        .get("content")
        .and_then(|c| c.as_str())
        .map(str::to_string);
    let mut tool_calls = Vec::new();
    if let Some(arr) = msg.get("tool_calls").and_then(|t| t.as_array()) {
        for (i, tc) in arr.iter().enumerate() {
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .ok_or_else(|| format!("tool_calls[{i}] 缺 function.name"))?;
            let arguments = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            tool_calls.push(ToolCall {
                // 缺 id 时按序合成——回环只要求「回填的 id 与发出的一致」
                id: tc
                    .get("id")
                    .and_then(|x| x.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_{i}")),
                kind: tc
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("function")
                    .to_string(),
                function: FunctionCall {
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                },
            });
        }
    }
    let usage = v.get("usage").map(|u| Usage {
        prompt_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        completion_tokens: u
            .get("completion_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
        total_tokens: u.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
    });
    Ok(ChatReply {
        content,
        tool_calls,
        usage,
    })
}

/// ChatClient：provider 调用面 trait——agent 循环只见它（考题注 StubClient
/// 证明循环与网络无关；真机 = httpc::OpenAiClient）。
pub trait ChatClient {
    fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
    ) -> Result<ChatReply, String>;
}
