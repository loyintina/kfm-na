//! crates/na-agent/tests/loop_spec.rs — BAR-161 钉③：loop 终止。
//!
//! 无 tool_calls 即停（stop）+ max_rounds 兜底停（防死循环）。
//! 答案区：crates/na-agent/src/agent.rs。考题不许改。

use std::collections::VecDeque;
use std::sync::Mutex;

use na_agent::agent::run_turn;
use na_agent::dialect::{ChatClient, ChatReply, FunctionCall, Message, ToolCall, ToolSpec};
use na_agent::host::FakeHost;
use na_agent::session::SessionWriter;

struct StubClient {
    replies: Mutex<VecDeque<ChatReply>>,
    pub calls: Mutex<usize>,
}

impl ChatClient for StubClient {
    fn chat(&self, _m: &str, _ms: &[Message], _t: &[ToolSpec]) -> Result<ChatReply, String> {
        *self.calls.lock().expect("锁") += 1;
        self.replies
            .lock()
            .expect("锁")
            .pop_front()
            .ok_or_else(|| "剧本耗尽".into())
    }
}

fn clock_call(id: &str) -> ChatReply {
    ChatReply {
        content: None,
        tool_calls: vec![ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "clock".into(),
                arguments: "{}".into(),
            },
        }],
        usage: None,
    }
}

fn done_event(host: &FakeHost) -> serde_json::Value {
    host.appended
        .lock()
        .expect("锁")
        .iter()
        .filter_map(|(_, l)| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["type"] == "done")
        .expect("done 事件在")
}

/// 无 tool_calls 即停：一轮出 content 就 done(stop)，绝不多打一轮 API
#[test]
fn spec_bar161_loop_无tool_calls即停() {
    let host = FakeHost::new("2026-09-26T00:00:00Z");
    let brain = StubClient {
        replies: Mutex::new(
            vec![ChatReply {
                content: Some("答完了".into()),
                tool_calls: vec![],
                usage: None,
            }]
            .into(),
        ),
        calls: Mutex::new(0),
    };
    let mut writer = SessionWriter::open(&host, "/s/0001-会话.jsonl");
    let mut messages = vec![Message::user("问")];
    let reply = run_turn(&host, &brain, &mut writer, "m", &mut messages, 8).expect("跑通");
    assert_eq!(reply, "答完了");
    assert_eq!(*brain.calls.lock().expect("锁"), 1, "恰一轮，不空转");
    let done = done_event(&host);
    assert_eq!(done["reason"], "stop");
    assert_eq!(done["rounds"], 1);
    assert_eq!(done["reply"], "答完了");
}

/// max_rounds 兜底：模型永出 tool_calls 也必须在上限停，
/// done(reason=max_rounds) 落账，轮数恰为上限
#[test]
fn spec_bar161_loop_max_rounds兜底停() {
    let host = FakeHost::new("2026-09-26T00:00:00Z");
    // 剧本给 10 份 tool_call 回复，上限 3——若兜底失效，剧本耗尽报错也是红
    let brain = StubClient {
        replies: Mutex::new(
            (0..10)
                .map(|i| clock_call(&format!("c{i}")))
                .collect::<Vec<_>>()
                .into(),
        ),
        calls: Mutex::new(0),
    };
    let mut writer = SessionWriter::open(&host, "/s/0001-会话.jsonl");
    let mut messages = vec![Message::user("问")];
    let reply = run_turn(&host, &brain, &mut writer, "m", &mut messages, 3).expect("兜底停");
    assert!(reply.contains("max_rounds"), "兜底说明: {reply}");
    assert_eq!(*brain.calls.lock().expect("锁"), 3, "恰打上限轮");
    let done = done_event(&host);
    assert_eq!(done["reason"], "max_rounds");
    assert_eq!(done["rounds"], 3);
    // 三轮各一对 tool_call/tool_result，事件链完整
    let types: Vec<String> = host
        .appended
        .lock()
        .expect("锁")
        .iter()
        .filter_map(|(_, l)| {
            serde_json::from_str::<serde_json::Value>(l)
                .ok()
                .and_then(|v| v["type"].as_str().map(str::to_string))
        })
        .collect();
    assert_eq!(
        types.iter().filter(|t| *t == "tool_call").count(),
        3,
        "三轮三调用: {types:?}"
    );
    assert_eq!(types.last().map(String::as_str), Some("done"));
}

/// DEFAULT_MAX_ROUNDS 契约：32（工单拍板值）
#[test]
fn spec_bar161_loop_默认上限32() {
    assert_eq!(na_agent::agent::DEFAULT_MAX_ROUNDS, 32);
}
