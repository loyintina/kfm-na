//! tests/brain_ep_spec.rs — A 档考题：脑插座 BrainEndpoint + echo-brain 夹具
//!
//! 契约真相源：docs/active/ai-presence.md §四A trait 草案 / §五 状态机。
//! echo-brain = 考题夹具（回放上游 fixture 翻译出的事件流，零网络），
//! 供期 0③ 对话页断网开发 + 协议层断网回归基准。
//!
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::brain::{ChatEvent, events_from_upstream_sse};
use kfm_na::brain_ep::{BrainEndpoint, ChatStartReq, EchoBrain};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const KIMI: &str = include_str!("fixtures/ai-chat/upstream-kimi-k2.7-highspeed-20260830.sse");
const GLM: &str = include_str!("fixtures/ai-chat/upstream-glm-5.3-flash-20260830.sse");

fn req() -> ChatStartReq {
    ChatStartReq {
        session_id: "考题会话-中文".to_string(), // 中文 session id 合法（四C 白名单含 \p{L}）
        messages: vec![("user".to_string(), "你好".to_string())],
        model: "echo-model".to_string(),
        provider: "echo".to_string(),
        tools: vec![],
    }
}

/// 收流直到 Done/Error 或超时（防实现死循环把考题挂死）。
fn collect(rx: &Receiver<ChatEvent>, budget: Duration) -> Vec<ChatEvent> {
    let deadline = Instant::now() + budget;
    let mut out = Vec::new();
    loop {
        let remain = deadline.saturating_duration_since(Instant::now());
        assert!(
            remain > Duration::ZERO,
            "收流超时：实现可能死循环或漏发终结事件"
        );
        match rx.recv_timeout(remain) {
            Ok(ev) => {
                let terminal = matches!(ev, ChatEvent::Done | ChatEvent::Error { .. });
                out.push(ev);
                if terminal {
                    return out;
                }
            }
            Err(_) => return out, // 发送端断开 = 流自然结束
        }
    }
}

// ========== 1. 全程回放：与翻译器直出逐事件一致 ==========

#[test]
fn kimi_fixture_full_replay_matches_translator() {
    let expect = events_from_upstream_sse(KIMI);
    assert!(expect.len() > 40, "夹具本身应有大几十事件， sanity check");
    let brain = EchoBrain::from_upstream_sse(KIMI, Duration::ZERO);
    let (_h, rx) = brain.start(req());
    let got = collect(&rx, Duration::from_secs(5));
    assert_eq!(got, expect, "echo 回放必须与翻译器直出逐事件一致");
    assert_eq!(got.last(), Some(&ChatEvent::Done));
}

#[test]
fn glm_fixture_replay_event_counts_match_anatomy() {
    // glm 41 帧逐帧解剖（brain_spec 已钉）：reasoning×37 + content×2 + stop + [DONE]
    let brain = EchoBrain::from_upstream_sse(GLM, Duration::ZERO);
    let (_h, rx) = brain.start(req());
    let got = collect(&rx, Duration::from_secs(5));
    let count = |f: fn(&ChatEvent) -> bool| got.iter().filter(|e| f(e)).count();
    assert_eq!(count(|e| matches!(e, ChatEvent::MessageStart)), 1);
    assert_eq!(count(|e| matches!(e, ChatEvent::ThinkingDelta { .. })), 37);
    assert_eq!(count(|e| matches!(e, ChatEvent::TextDelta { .. })), 2);
    assert_eq!(
        count(|e| matches!(e, ChatEvent::ContentBlockStop { .. })),
        1
    );
    assert_eq!(count(|e| matches!(e, ChatEvent::MessageStop)), 1);
    assert_eq!(count(|e| matches!(e, ChatEvent::Done)), 1);
}

// ========== 2. 取消语义 ==========

#[test]
fn cancel_mid_run_truncates_with_cancelled_error() {
    let total = events_from_upstream_sse(KIMI).len();
    let brain = EchoBrain::from_upstream_sse(KIMI, Duration::from_millis(20));
    let (h, rx) = brain.start(req());
    let first = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("首事件应到达");
    assert!(matches!(first, ChatEvent::MessageStart));
    assert!(brain.cancel(&h), "进行中的 run 取消应返回 true");
    let rest = collect(&rx, Duration::from_secs(5));
    // 截断：收到的事件总数必须远小于全程
    assert!(
        1 + rest.len() < total / 2,
        "取消后仍收到 {} 事件（全程 {}），取消检查疑似失效",
        1 + rest.len(),
        total
    );
    // 收尾事件 = 人话「已取消」（四C 错误语义：用户取消 → error '已取消'）
    match rest.last() {
        Some(ChatEvent::Error { content }) => assert!(content.contains("已取消")),
        other => panic!("取消后收尾应为 Error(已取消)，实际 {other:?}"),
    }
    // run 终态可见（观测轨：UI 据此灭灯）
    let deadline = Instant::now() + Duration::from_secs(2);
    while !h.is_done() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.is_done(), "取消后 run 应进入 done 态");
    assert!(h.is_cancelled());
}

#[test]
fn cancel_after_finish_returns_false() {
    let brain = EchoBrain::from_upstream_sse(GLM, Duration::ZERO);
    let (h, rx) = brain.start(req());
    let _ = collect(&rx, Duration::from_secs(5));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !h.is_done() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.is_done());
    assert!(
        !brain.cancel(&h),
        "已终结的 run 取消应返回 false（kfmv4 ok:false 语义）"
    );
}

// ========== 3. attach 游标回放（重连接口，echo 版 = 历史后缀回放） ==========

#[test]
fn attach_replays_suffix_from_cursor() {
    let all = events_from_upstream_sse(GLM);
    let brain = EchoBrain::from_upstream_sse(GLM, Duration::ZERO);
    let (h, _rx) = brain.start(req());
    let rx2 = brain.attach(&h, 3).expect("echo 必须支持 attach");
    let got = collect(&rx2, Duration::from_secs(5));
    assert_eq!(
        got,
        all[3..].to_vec(),
        "attach(from=3) 必须回放事件 3 起的后缀"
    );
}

#[test]
fn attach_beyond_end_yields_empty_stream() {
    let all = events_from_upstream_sse(GLM);
    let brain = EchoBrain::from_upstream_sse(GLM, Duration::ZERO);
    let (h, _rx) = brain.start(req());
    let rx2 = brain
        .attach(&h, all.len() as u64)
        .expect("越界游标仍应给流（kfmv4：只发 __end__ 不 404）");
    let got = collect(&rx2, Duration::from_secs(2));
    assert!(got.is_empty(), "游标越界应得空流，实际 {} 事件", got.len());
}
