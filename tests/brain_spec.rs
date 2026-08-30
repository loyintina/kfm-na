//! tests/brain_spec.rs — A 档考题：AI 脑协议层（src/brain.rs）
//!
//! 契约真相源：docs/active/ai-presence.md §四A/四B。
//! 判卷基准：tests/fixtures/ai-chat/ 双路上游真流（2026-08-30 活探针）——
//! kimi 45 帧（role×1 + reasoning×38 + 空delta×1 + content×2 + stop×1 +
//! usage-only×1 + [DONE]），glm 41 帧（role+reasoning×37 + role+content×2 +
//! stop+usage×1 + [DONE]），逐帧解剖已复核。
//!
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::brain::{
    ChatEvent, OpenAiTranslator, RunAccumulator, SseParser, build_chat_request,
    error_event_from_http,
};

const KIMI: &str = include_str!("fixtures/ai-chat/upstream-kimi-k2.7-highspeed-20260830.sse");
const GLM: &str = include_str!("fixtures/ai-chat/upstream-glm-5.3-flash-20260830.sse");

// ========== 工具：fixture → 帧 / 事件 ==========

fn frames_of(raw: &str) -> Vec<String> {
    let mut p = SseParser::new();
    p.feed(raw.as_bytes());
    p.drain_frames()
}

fn events_of(raw: &str) -> Vec<ChatEvent> {
    let mut p = SseParser::new();
    p.feed(raw.as_bytes());
    let mut t = OpenAiTranslator::new();
    let mut out = Vec::new();
    for f in p.drain_frames() {
        out.extend(t.translate_payload(&f));
    }
    out
}

/// 各变体计数：(msg_start, block_start, text_d, think_d, stop, msg_stop, done, error)
fn event_counts(evs: &[ChatEvent]) -> (usize, usize, usize, usize, usize, usize, usize, usize) {
    let mut c = (0, 0, 0, 0, 0, 0, 0, 0);
    for e in evs {
        match e {
            ChatEvent::MessageStart => c.0 += 1,
            ChatEvent::ContentBlockStart { .. } => c.1 += 1,
            ChatEvent::TextDelta { .. } => c.2 += 1,
            ChatEvent::ThinkingDelta { .. } => c.3 += 1,
            ChatEvent::ContentBlockStop { .. } => c.4 += 1,
            ChatEvent::MessageStop => c.5 += 1,
            ChatEvent::Done => c.6 += 1,
            ChatEvent::Error { .. } => c.7 += 1,
            _ => {}
        }
    }
    c
}

fn text_of(evs: &[ChatEvent]) -> String {
    evs.iter()
        .filter_map(|e| match e {
            ChatEvent::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

// ========== SSE 帧解析器 ==========

#[test]
fn spec_sse_单帧整喂() {
    let mut p = SseParser::new();
    p.feed(b"data: {\"a\":1}\n\n");
    assert_eq!(p.drain_frames(), vec!["{\"a\":1}".to_string()]);
}

#[test]
fn spec_sse_多帧粘连保序() {
    let mut p = SseParser::new();
    p.feed(b"data: one\n\ndata: two\n\ndata: three\n\n");
    assert_eq!(p.drain_frames(), vec!["one", "two", "three"]);
}

#[test]
fn spec_sse_半帧暂存不吐() {
    let mut p = SseParser::new();
    p.feed(b"data: hel");
    assert_eq!(p.next_frame(), None, "半帧不许吐出");
    p.feed(b"lo\n\n");
    assert_eq!(p.drain_frames(), vec!["hello"]);
}

#[test]
fn spec_sse_逐字节碎喂与整喂等价() {
    let whole = frames_of(KIMI);
    let mut p = SseParser::new();
    let mut chopped = Vec::new();
    for &b in KIMI.as_bytes() {
        p.feed(&[b]);
        chopped.extend(p.drain_frames());
    }
    assert_eq!(whole.len(), 45, "kimi fixture 帧数钉死（含 [DONE]）");
    assert_eq!(chopped, whole, "逐字节碎喂必须与整喂逐帧一致");
}

#[test]
fn spec_sse_注释行与crlf容忍() {
    let mut p = SseParser::new();
    p.feed(b": keep-alive comment\r\ndata: x\r\n\r\n");
    assert_eq!(p.drain_frames(), vec!["x"]);
}

#[test]
fn spec_sse_多行data按规范拼接() {
    let mut p = SseParser::new();
    p.feed(b"data: a\ndata: b\n\n");
    assert_eq!(p.drain_frames(), vec!["a\nb"]);
}

// ========== 上游翻译器（fixture 当标准答案） ==========

#[test]
fn spec_翻译_kimi真流_事件序列钉死() {
    let evs = events_of(KIMI);
    // 45 事件 = start(2) + thinking×38 + text×2 + stop(2) + done(1)
    assert_eq!(evs.len(), 45, "事件总数钉死");
    assert_eq!(event_counts(&evs), (1, 1, 2, 38, 1, 1, 1, 0));
    assert_eq!(evs[0], ChatEvent::MessageStart);
    assert_eq!(
        evs[1],
        ChatEvent::ContentBlockStart {
            index: 0,
            tool_use: None
        }
    );
    assert_eq!(evs[44], ChatEvent::Done, "末事件必须是 Done");
    assert_eq!(text_of(&evs), "PONG");
    // stop 与 message_stop 必须相邻收尾（done 之前）
    assert_eq!(evs[42], ChatEvent::ContentBlockStop { index: 0 });
    assert_eq!(evs[43], ChatEvent::MessageStop);
}

#[test]
fn spec_翻译_glm真流_事件序列钉死() {
    let evs = events_of(GLM);
    // 44 事件 = start(2) + thinking×37 + text×2 + stop(2) + done(1)
    assert_eq!(evs.len(), 44, "事件总数钉死");
    assert_eq!(event_counts(&evs), (1, 1, 2, 37, 1, 1, 1, 0));
    assert_eq!(evs[0], ChatEvent::MessageStart);
    assert_eq!(evs[43], ChatEvent::Done);
    assert_eq!(text_of(&evs), "PONG");
}

#[test]
fn spec_翻译_逐字节碎喂与整喂等价() {
    let whole = events_of(KIMI);
    let mut p = SseParser::new();
    let mut t = OpenAiTranslator::new();
    let mut chopped = Vec::new();
    for &b in KIMI.as_bytes() {
        p.feed(&[b]);
        for f in p.drain_frames() {
            chopped.extend(t.translate_payload(&f));
        }
    }
    assert_eq!(chopped, whole, "碎喂事件序列必须与整喂一致");
}

#[test]
fn spec_翻译_静默帧零事件() {
    // kimi 方言三类静默帧：role-only / 空 delta / usage-only（choices:[]）
    let mut t = OpenAiTranslator::new();
    assert!(t
        .translate_payload(r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}"#)
        .is_empty());
    assert!(
        t.translate_payload(r#"{"choices":[{"index":0,"delta":{},"finish_reason":null}]}"#)
            .is_empty()
    );
    assert!(
        t.translate_payload(r#"{"choices":[],"usage":{"total_tokens":58}}"#)
            .is_empty()
    );
    // 且静默帧不得触发懒启动
    assert!(
        t.translate_payload(r#"{"choices":[{"index":0,"delta":{},"finish_reason":null}]}"#)
            .is_empty()
    );
}

#[test]
fn spec_翻译_未知字段容忍() {
    let mut t = OpenAiTranslator::new();
    let evs = t.translate_payload(
        r#"{"id":"x","system_fingerprint":"fp","future_field":{"nested":[1,2]},"choices":[{"index":0,"delta":{"content":"hi","mystery":true},"finish_reason":null}]}"#,
    );
    assert_eq!(
        evs,
        vec![
            ChatEvent::MessageStart,
            ChatEvent::ContentBlockStart {
                index: 0,
                tool_use: None
            },
            ChatEvent::TextDelta {
                index: 0,
                text: "hi".into()
            },
        ]
    );
}

#[test]
fn spec_翻译_tool_calls帧期0容忍不炸() {
    let mut t = OpenAiTranslator::new();
    // 期 2 才翻译 tool_use；期 0 判据 = 不 panic、不产生畸形事件
    let evs = t.translate_payload(
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]},"finish_reason":null}]}"#,
    );
    assert!(
        evs.is_empty(),
        "tool_calls 帧期 0 静默（期 2 转正时改判据）"
    );
}

#[test]
fn spec_翻译_坏json帧产error不panic() {
    let mut t = OpenAiTranslator::new();
    let evs = t.translate_payload("{not json at all");
    assert_eq!(evs.len(), 1);
    match &evs[0] {
        ChatEvent::Error { content } => assert!(content.contains("解析失败")),
        other => panic!("坏帧必须产 Error 事件，实得 {other:?}"),
    }
}

// ========== 归位（R3 / kfmv4 陷阱 10） ==========

#[test]
fn spec_归位_纯思考归正文() {
    let mut acc = RunAccumulator::new();
    for e in events_of(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"答案是\"}}]}\n\n\
         data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"四十二\"}}]}\n\n\
         data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
         data: [DONE]\n\n",
    )
    .iter()
    {
        acc.apply(e);
    }
    assert_eq!(acc.final_text(), "答案是四十二", "纯思考必须归位为正文");
    assert_eq!(acc.thinking(), "", "归位后思考清空");
    assert!(acc.relocated(), "归位信号必须置位（观测用）");
}

#[test]
fn spec_归位_有正文不归位() {
    let mut acc = RunAccumulator::new();
    for e in events_of(KIMI).iter() {
        acc.apply(e);
    }
    assert_eq!(acc.final_text(), "PONG");
    assert!(!acc.thinking().is_empty(), "思考区原样保留");
    assert!(!acc.relocated());
}

// ========== 上游 HTTP 错误体（双路 401 实录形状） ==========

#[test]
fn spec_错误体_两家401各按其形() {
    let kimi = error_event_from_http(
        401,
        r#"{"error":{"message":"The API Key appears to be invalid or may have expired.","type":"invalid_authentication_error"}}"#,
    );
    let glm = error_event_from_http(
        401,
        r#"{"error":{"code":"401","message":"令牌已过期或验证不正确"}}"#,
    );
    match kimi {
        ChatEvent::Error { content } => {
            assert!(content.starts_with("API 请求失败: 401 — "));
            assert!(content.contains("API Key"));
        }
        other => panic!("必须是 Error 事件: {other:?}"),
    }
    match glm {
        ChatEvent::Error { content } => {
            assert!(content.starts_with("API 请求失败: 401 — "));
            assert!(content.contains("令牌已过期"));
        }
        other => panic!("必须是 Error 事件: {other:?}"),
    }
}

#[test]
fn spec_错误体_非json原样截断300() {
    let big = "x".repeat(500);
    let ev = error_event_from_http(502, &big);
    match ev {
        ChatEvent::Error { content } => {
            assert!(content.starts_with("API 请求失败: 502 — "));
            assert_eq!(
                content.chars().count(),
                "API 请求失败: 502 — ".chars().count() + 300
            );
        }
        other => panic!("必须是 Error 事件: {other:?}"),
    }
}

// ========== 请求体构造 ==========

#[test]
fn spec_请求体_形状钉死() {
    let s = build_chat_request("glm-5.3-flash", &[("user".to_string(), "你好".to_string())]);
    let v: serde_json::Value = serde_json::from_str(&s).expect("产物必须合法 JSON");
    assert_eq!(v["model"], "glm-5.3-flash");
    assert_eq!(v["stream"], true);
    assert_eq!(v["stream_options"]["include_usage"], true);
    assert_eq!(v["messages"][0]["role"], "user");
    assert_eq!(v["messages"][0]["content"], "你好");
    assert_eq!(v["messages"].as_array().unwrap().len(), 1);
}
