//! crates/na-agent/tests/session_spec.rs — BAR-161 钉⑤：会话追加。
//!
//! append-only jsonl 逐行合法 JSON、seq/ts 每行在、usage 从第一轮就记、
//! NNNN 序号递增、续写 seq 不断、回放重建 messages。
//! 答案区：crates/na-agent/src/session.rs。考题不许改。

use na_agent::dialect::Usage;
use na_agent::host::{FakeHost, Host, StdHost};
use na_agent::session::{
    SessionWriter, latest_session, next_session_seq, replay_messages, session_path, valid_line_name,
};

fn usage(round_tokens: u64) -> Usage {
    Usage {
        prompt_tokens: round_tokens,
        completion_tokens: 1,
        total_tokens: round_tokens + 1,
    }
}

/// 逐行合法 JSON + seq 单调 + ts 每行在 + usage 第一轮就记
#[test]
fn spec_bar161_会话_逐行合法usage从第一轮记() {
    let host = FakeHost::new("2026-09-26T00:00:00Z");
    let path = session_path("/root/.kfm/session/demo", 1);
    assert_eq!(path, "/root/.kfm/session/demo/0001-会话.jsonl");
    let mut w = SessionWriter::open(&host, &path);
    w.user_msg("任务").expect("写");
    w.usage(1, usage(10)).expect("写"); // 第一轮 API 返回即记
    w.model_msg(1, Some("好"), &[]).expect("写");
    w.done(1, "stop", "好").expect("写");

    let appended = host.appended.lock().expect("锁");
    assert_eq!(appended.len(), 4);
    let mut last_seq = 0;
    for (_, line) in appended.iter() {
        let v: serde_json::Value = serde_json::from_str(line).expect("逐行合法 JSON");
        let seq = v["seq"].as_u64().expect("seq 在");
        assert!(seq > last_seq, "seq 单调: {seq} <= {last_seq}");
        last_seq = seq;
        assert_eq!(v["ts"], "2026-09-26T00:00:00Z", "ts 每行在");
        assert!(v["type"].is_string());
    }
    // usage 从第一轮就记：第 2 行（紧随 user_msg）即是，round=1
    let v: serde_json::Value = serde_json::from_str(&appended[1].1).expect("JSON");
    assert_eq!(v["type"], "usage");
    assert_eq!(v["round"], 1);
    assert_eq!(v["prompt_tokens"], 10);
    assert_eq!(v["total_tokens"], 11);
}

/// NNNN 序号递增（真目录扫档实证）+ 最新会话定位
#[test]
#[allow(non_snake_case)]
fn spec_bar161_会话_NNNN递增() {
    let tmp = tempfile::tempdir().expect("临时目录");
    let dir = tmp.path().join("demo");
    let dir = dir.to_string_lossy().into_owned();
    let host = StdHost::new(tmp.path().to_path_buf());
    assert_eq!(next_session_seq(&host, &dir).expect("空目录"), 1);
    host.append_line(&session_path(&dir, 1), "{}")
        .expect("写 1");
    host.append_line(&session_path(&dir, 2), "{}")
        .expect("写 2");
    assert_eq!(next_session_seq(&host, &dir).expect("递增"), 3);
    assert_eq!(
        latest_session(&host, &dir).expect("定位"),
        Some(session_path(&dir, 2))
    );
    // 杂件不干扰序号（line.toml 与随手文件不算会话）
    host.append_line(&format!("{dir}/line.toml"), "x")
        .expect("写杂");
    host.append_line(&format!("{dir}/0003笔记.txt"), "x")
        .expect("写杂");
    assert_eq!(next_session_seq(&host, &dir).expect("杂件不扰"), 3);
}

/// 续写 seq 不断：重开既有文件接着数（append-only 不覆写）
#[test]
fn spec_bar161_会话_续写seq不断() {
    let host = FakeHost::new("2026-09-26T00:00:00Z");
    let path = "/s/demo/0001-会话.jsonl";
    {
        let mut w = SessionWriter::open(&host, path);
        w.user_msg("第一条").expect("写");
        w.done(1, "stop", "完").expect("写");
    }
    // 重开（FakeHost 的 append 账即文件本体——open 读不到 appended，
    // 故用真临时目录实证 seq 续账）
    let tmp = tempfile::tempdir().expect("临时目录");
    let real = StdHost::new(tmp.path().to_path_buf());
    let p = tmp.path().join("0001-会话.jsonl");
    let p = p.to_string_lossy().into_owned();
    {
        let mut w = SessionWriter::open(&real, &p);
        w.user_msg("一").expect("写");
        w.user_msg("二").expect("写");
    }
    {
        let mut w = SessionWriter::open(&real, &p);
        w.user_msg("三").expect("续写");
    }
    let text = std::fs::read_to_string(&p).expect("文件在");
    let seqs: Vec<u64> = text
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).expect("JSON")["seq"]
                .as_u64()
                .expect("seq")
        })
        .collect();
    assert_eq!(seqs, [1, 2, 3], "续写 seq 接着数不覆写");
}

/// 回放：jsonl → messages 重建上下文（user/assistant/tool 三类，
/// usage/done 是账不进上下文）
#[test]
fn spec_bar161_会话_回放重建() {
    let host = FakeHost::new("2026-09-26T00:00:00Z");
    let path = "/s/demo/0001-会话.jsonl";
    let mut w = SessionWriter::open(&host, path);
    w.user_msg("读个文件").expect("写");
    w.usage(1, usage(5)).expect("写");
    let calls = vec![na_agent::dialect::ToolCall {
        id: "c1".into(),
        kind: "function".into(),
        function: na_agent::dialect::FunctionCall {
            name: "read_file".into(),
            arguments: "{\"path\":\"/a\"}".into(),
        },
    }];
    w.model_msg(1, None, &calls).expect("写");
    w.tool_call(1, "c1", "read_file", "{\"path\":\"/a\"}")
        .expect("写");
    w.tool_result(1, "c1", "read_file", "内容").expect("写");
    w.usage(2, usage(9)).expect("写");
    w.model_msg(2, Some("读完了"), &[]).expect("写");
    w.done(2, "stop", "读完了").expect("写");

    // FakeHost 的 appended 账落成 read 面：拼回文件本体再回放
    let text: String = host
        .appended
        .lock()
        .expect("锁")
        .iter()
        .map(|(_, l)| format!("{l}\n"))
        .collect();
    let host2 = FakeHost::new("2026-09-26T00:00:00Z").with_file(path, &text);
    let msgs = replay_messages(&host2, path).expect("回放");
    assert_eq!(msgs.len(), 4, "user+assistant+tool+assistant");
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].tool_calls.as_ref().expect("tool_calls")[0].id, "c1");
    assert_eq!(msgs[2].role, "tool");
    assert_eq!(msgs[2].tool_call_id.as_deref(), Some("c1"));
    assert_eq!(msgs[2].content.as_deref(), Some("内容"));
    assert_eq!(msgs[3].content.as_deref(), Some("读完了"));

    // 不存在的路径 = 空史（新会话第一课）
    assert!(
        replay_messages(&host2, "/s/无/9999-会话.jsonl")
            .expect("空史")
            .is_empty()
    );
}

/// 线名闸：路径注入/穿越/中文一律拒（花名册 = 路径组件，出事即越权）
#[test]
fn spec_bar161_会话_线名闸() {
    assert!(valid_line_name("demo"));
    assert!(valid_line_name("a-b_c9"));
    assert!(!valid_line_name(""));
    assert!(!valid_line_name("../etc"));
    assert!(!valid_line_name("a/b"));
    assert!(!valid_line_name("信箱"));
    assert!(!valid_line_name("a.b"));
    assert!(!valid_line_name(&"x".repeat(65)));
}
