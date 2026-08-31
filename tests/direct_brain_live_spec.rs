//! tests/direct_brain_live_spec.rs — direct-api-brain B 档钉（live，双线）
//!
//! 默认 #[ignore]——真打 Kimi/智谱上游，烧真 token（用户 2026-08-31 明示预算充足）。
//! 手动跑：cargo test --test direct_brain_live_spec -- --ignored
//!
//! 配置源：/root/.kfmv4/providers.json + .env（开发期借用 kfmv4 的卡；
//! 上机后换 na 私有目录同款文件）。
//!
//! 判卷（双线同尺）：start → 收流 → Done 到达 / 零 Error 事件 /
//! 累积正文含暗号 NA-LIVE-OK（reasoning 归位路径也认了——归位后正文非空即算）。

use kfm_na::brain::{ChatEvent, RunAccumulator};
use kfm_na::brain_ep::{BrainEndpoint, ChatStartReq};
use kfm_na::direct_brain::DirectApiBrain;
use std::time::{Duration, Instant};

fn brain() -> DirectApiBrain {
    let providers = std::fs::read_to_string("/root/.kfmv4/providers.json")
        .expect("读 kfmv4 providers.json 失败");
    let dotenv = std::fs::read_to_string("/root/.kfmv4/.env").expect("读 kfmv4 .env 失败");
    DirectApiBrain::from_files(&providers, &dotenv).expect("装配失败")
}

fn run_once(provider: &str, model: &str) {
    let brain = brain();
    let req = ChatStartReq {
        session_id: "na-live-test".to_string(),
        messages: vec![(
            "user".to_string(),
            "请只回复这串字符，别的什么都不要说：NA-LIVE-OK".to_string(),
        )],
        model: model.to_string(),
        provider: provider.to_string(),
        tools: vec![],
    };
    let (_h, rx) = brain.start(req);
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut acc = RunAccumulator::new();
    let mut saw_done = false;
    let mut errors = Vec::new();
    let mut n_events = 0usize;
    while Instant::now() < deadline && !saw_done {
        let remain = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remain) {
            Ok(ev) => {
                n_events += 1;
                if let ChatEvent::Error { content } = &ev {
                    errors.push(content.clone());
                }
                if matches!(ev, ChatEvent::Done) {
                    saw_done = true;
                }
                acc.apply(&ev);
            }
            Err(_) => break,
        }
    }
    let text = acc.final_text();
    eprintln!(
        "[live] {provider}/{model}: events={n_events} done={saw_done} \
         relocated={} text={:?} errors={errors:?}",
        acc.relocated(),
        text
    );
    assert!(saw_done, "{provider}/{model}: 120s 内未见 Done");
    assert!(
        errors.is_empty(),
        "{provider}/{model}: 出现 Error 事件: {errors:?}"
    );
    assert!(
        text.contains("NA-LIVE-OK"),
        "{provider}/{model}: 正文应含暗号，实际 {text:?}"
    );
}

#[test]
#[ignore]
fn live_kimi_highspeed_一轮真对话() {
    run_once("Kimi", "kimi-for-coding-highspeed");
}

#[test]
#[ignore]
fn live_glm_flash_一轮真对话() {
    run_once("智谱", "glm-5.3-flash");
}

#[test]
#[ignore]
fn live_坏provider_人话error事件() {
    let brain = brain();
    let req = ChatStartReq {
        session_id: "na-live-test".to_string(),
        messages: vec![("user".to_string(), "hi".to_string())],
        model: "x".to_string(),
        provider: "不存在的卡".to_string(),
        tools: vec![],
    };
    let (_h, rx) = brain.start(req);
    let ev = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("应立即收到事件");
    match ev {
        ChatEvent::Error { content } => assert!(content.contains("provider 不存在")),
        other => panic!("坏 provider 应立即 Error 事件，实际 {other:?}"),
    }
}
