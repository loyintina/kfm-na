//! crates/na-agent/tests/host_spec.rs — BAR-161 钉①：host 抽象。
//!
//! FakeHost 注入（内存 fs + 剧本命令 + 固定钟）跑完整 agent 循环：
//! fs/exec/clock 全走 trait，核心逻辑 host 无关由此钉死。
//! 答案区：crates/na-agent/src/{agent,host,tools,session}.rs。考题不许改。

use std::collections::VecDeque;
use std::sync::Mutex;

use na_agent::agent::run_turn;
use na_agent::dialect::{ChatClient, ChatReply, FunctionCall, Message, ToolCall, Usage};
use na_agent::host::{CmdOut, FakeHost};
use na_agent::session::SessionWriter;

/// 剧本脑：按序吐 reply，吐完报错（证明循环精确消费）
struct StubClient {
    replies: Mutex<VecDeque<ChatReply>>,
    pub calls: Mutex<usize>,
}

impl StubClient {
    fn scripted(replies: Vec<ChatReply>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(0),
        }
    }
}

impl ChatClient for StubClient {
    fn chat(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[na_agent::dialect::ToolSpec],
    ) -> Result<ChatReply, String> {
        *self.calls.lock().expect("锁") += 1;
        self.replies
            .lock()
            .expect("锁")
            .pop_front()
            .ok_or_else(|| "剧本耗尽".to_string())
    }
}

fn tc(id: &str, name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        kind: "function".into(),
        function: FunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

fn usage() -> Option<Usage> {
    Some(Usage {
        prompt_tokens: 11,
        completion_tokens: 7,
        total_tokens: 18,
    })
}

/// 全走 trait：读文件走 fake fs、命令走剧本、钟走固定值——
/// 任何一件漏到真宿主（std::fs/真 sh）都会当场空账本/错内容而红。
#[test]
fn spec_bar161_host抽象_fakehost全走trait() {
    let host = FakeHost::new("2026-09-26T00:00:00Z")
        .with_file("/虚拟/账本.txt", "BAR-161 假账内容")
        .with_command(
            "echo 假命令",
            CmdOut {
                exit_code: 0,
                stdout: "假输出\n".into(),
                stderr: String::new(),
            },
        );
    let brain = StubClient::scripted(vec![
        ChatReply {
            content: None,
            tool_calls: vec![tc("c1", "read_file", "{\"path\":\"/虚拟/账本.txt\"}")],
            usage: usage(),
        },
        ChatReply {
            content: None,
            tool_calls: vec![tc("c2", "run_command", "{\"command\":\"echo 假命令\"}")],
            usage: usage(),
        },
        ChatReply {
            content: None,
            tool_calls: vec![tc("c3", "clock", "{}")],
            usage: usage(),
        },
        ChatReply {
            content: Some("读完跑完，收工".into()),
            tool_calls: vec![],
            usage: usage(),
        },
    ]);
    let mut writer = SessionWriter::open(&host, "/会话根/demo/0001-会话.jsonl");
    let mut messages = vec![Message::user("读账跑命令报时")];
    let reply = run_turn(&host, &brain, &mut writer, "假模型", &mut messages, 8).expect("循环跑通");
    assert_eq!(reply, "读完跑完，收工");
    assert_eq!(*brain.calls.lock().expect("锁"), 4, "四轮精确消费");

    // wire 全在 fake 账里（append-only 账 = 唯一写入面）
    let appended = host.appended.lock().expect("锁");
    assert_eq!(
        appended.len(),
        15,
        "3×(usage+model+tool_call+tool_result) + 末轮 usage+model_msg+done"
    );
    let first_path = &appended[0].0;
    assert!(
        appended.iter().all(|(p, _)| p == first_path),
        "全事件同一份会话文件"
    );
    assert_eq!(first_path, "/会话根/demo/0001-会话.jsonl");

    // 命令走的是 FakeHost（真 sh 里「echo 假命令」stdout 不会是「假输出」）
    assert_eq!(host.ran.lock().expect("锁").as_slice(), ["echo 假命令"]);

    // 事件链类型序（usage 每轮都在 model_msg 前——API 返回即记账）
    let types: Vec<String> = appended
        .iter()
        .map(|(_, l)| {
            let v: serde_json::Value = serde_json::from_str(l).expect("每行合法 JSON");
            v["type"].as_str().expect("type 在").to_string()
        })
        .collect();
    assert_eq!(
        types,
        vec![
            "usage",
            "model_msg",
            "tool_call",
            "tool_result",
            "usage",
            "model_msg",
            "tool_call",
            "tool_result",
            "usage",
            "model_msg",
            "tool_call",
            "tool_result",
            "usage",
            "model_msg",
            "done",
        ]
    );
    // tool_result 的内容证明 read_file 吃的是 fake fs 不是真盘
    let v: serde_json::Value = serde_json::from_str(&appended[3].1).expect("合法 JSON");
    assert_eq!(v["output"], "BAR-161 假账内容");
    // clock 走固定钟
    let v: serde_json::Value = serde_json::from_str(&appended[11].1).expect("合法 JSON");
    assert_eq!(v["output"], "2026-09-26T00:00:00Z");
}

/// run_command 的结果三件套（exit_code/stdout/stderr）全量回填给模型——
/// 回填消息可直接续进 messages（回放链的一环）。
#[test]
fn spec_bar161_host抽象_命令三件套回填() {
    let host = FakeHost::new("2026-09-26T00:00:00Z").with_command(
        "false",
        CmdOut {
            exit_code: 1,
            stdout: "o".into(),
            stderr: "e".into(),
        },
    );
    let brain = StubClient::scripted(vec![
        ChatReply {
            content: None,
            tool_calls: vec![tc("c1", "run_command", "{\"command\":\"false\"}")],
            usage: None,
        },
        ChatReply {
            content: Some("好".into()),
            tool_calls: vec![],
            usage: None,
        },
    ]);
    let mut writer = SessionWriter::open(&host, "/s/0001-会话.jsonl");
    let mut messages = vec![Message::user("跑")];
    run_turn(&host, &brain, &mut writer, "m", &mut messages, 4).expect("跑通");
    let tool_msg = messages
        .iter()
        .find(|m| m.role == "tool")
        .expect("工具回填消息在");
    let v: serde_json::Value =
        serde_json::from_str(tool_msg.content.as_deref().expect("content 在")).expect("JSON");
    assert_eq!(v["exit_code"], 1);
    assert_eq!(v["stdout"], "o");
    assert_eq!(v["stderr"], "e");
    assert_eq!(tool_msg.tool_call_id.as_deref(), Some("c1"));
}
