//! crates/na-agent/tests/dialect_spec.rs — BAR-161 钉④：方言解析。
//!
//! OpenAI 兼容响应的 content / tool_calls（含多调用）/ usage 解析，
//! 以及请求体形状（messages + tools + stream=false）。
//! 答案区：crates/na-agent/src/dialect.rs。考题不许改。

use na_agent::dialect::{Message, build_request, parse_response};
use na_agent::tools::tool_specs;

#[test]
fn spec_bar161_方言_content与usage() {
    let body = r#"{
        "choices": [{"message": {"role": "assistant", "content": "你好"}}],
        "usage": {"prompt_tokens": 101, "completion_tokens": 9, "total_tokens": 110}
    }"#;
    let r = parse_response(body).expect("解析");
    assert_eq!(r.content.as_deref(), Some("你好"));
    assert!(r.tool_calls.is_empty());
    let u = r.usage.expect("usage 在");
    assert_eq!(u.prompt_tokens, 101);
    assert_eq!(u.completion_tokens, 9);
    assert_eq!(u.total_tokens, 110);
}

#[test]
fn spec_bar161_方言_tool_calls多调用() {
    let body = r#"{
        "choices": [{"message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {"id": "call_a", "type": "function",
                 "function": {"name": "read_file", "arguments": "{\"path\":\"/a\"}"}},
                {"id": "call_b", "type": "function",
                 "function": {"name": "clock", "arguments": "{}"}}
            ]
        }}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 6, "total_tokens": 11}
    }"#;
    let r = parse_response(body).expect("解析");
    assert_eq!(r.tool_calls.len(), 2, "多调用全保留");
    assert_eq!(r.tool_calls[0].id, "call_a");
    assert_eq!(r.tool_calls[0].function.name, "read_file");
    assert_eq!(r.tool_calls[0].function.arguments, "{\"path\":\"/a\"}");
    assert_eq!(r.tool_calls[1].id, "call_b");
    assert_eq!(r.tool_calls[1].function.name, "clock");
    assert!(r.content.is_none(), "content null = None");
}

/// 缺 id 的兼容源：按序合成 call_N——回环只要求回填 id 与发出一致
#[test]
fn spec_bar161_方言_缺id按序合成() {
    let body = r#"{"choices": [{"message": {"tool_calls": [
        {"function": {"name": "clock", "arguments": "{}"}}
    ]}}]}"#;
    let r = parse_response(body).expect("解析");
    assert_eq!(r.tool_calls[0].id, "call_0");
}

#[test]
fn spec_bar161_方言_坏件判负() {
    assert!(parse_response("不是 json").is_err());
    assert!(parse_response("{}").is_err(), "缺 choices 判负");
    assert!(
        parse_response("{\"choices\": []}").is_err(),
        "空 choices 判负"
    );
    // 上游 error 字段：报错文本透出（不含 key——请求不在响应里）
    let err = parse_response("{\"error\": {\"message\": \"余额不足\"}}").unwrap_err();
    assert!(err.contains("余额不足"), "{err}");
}

#[test]
fn spec_bar161_方言_请求体形状() {
    let messages = vec![Message::user("问")];
    let body = build_request("glm-5.3-flash", &messages, &tool_specs());
    let v: serde_json::Value = serde_json::from_str(&body).expect("合法 JSON");
    assert_eq!(v["model"], "glm-5.3-flash");
    assert_eq!(v["stream"], false, "v1 非流式");
    assert_eq!(v["messages"][0]["role"], "user");
    assert_eq!(v["messages"][0]["content"], "问");
    let names: Vec<&str> = v["tools"]
        .as_array()
        .expect("tools 数组")
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["read_file", "write_file", "run_command", "clock"],
        "工具四件齐"
    );
}

/// 回填消息的序列化形状：assistant 带 tool_calls 时 content 键为 null、
/// tool 消息带 tool_call_id——OpenAI 兼容端点的硬要求（缺了 400）
#[test]
fn spec_bar161_方言_回填消息形状() {
    use na_agent::dialect::{FunctionCall, ToolCall};
    let m = Message::assistant(
        None,
        vec![ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "clock".into(),
                arguments: "{}".into(),
            },
        }],
    );
    let v = serde_json::to_value(&m).expect("序列化");
    assert_eq!(v["role"], "assistant");
    assert_eq!(v["tool_calls"][0]["id"], "c1");
    let t = Message::tool("c1", "结果".into());
    let v = serde_json::to_value(&t).expect("序列化");
    assert_eq!(v["role"], "tool");
    assert_eq!(v["tool_call_id"], "c1");
    assert_eq!(v["content"], "结果");
}
