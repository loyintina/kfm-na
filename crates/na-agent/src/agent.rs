//! agent.rs — agent 核心循环（平台无关，A 档）：
//! model 出 tool_calls → 经 Host 执行 → 结果回填 → 续跑，至 stop
//! （无 tool_calls 即停）；max_rounds 兜底防死循环。
//!
//! 循环只见 Host / ChatClient 两个 trait——host 无关与网络无关由
//! tests/host_spec.rs / loop_spec.rs 的 FakeHost + StubClient 钉死。

use crate::dialect::{ChatClient, Message};
use crate::host::Host;
use crate::session::SessionWriter;
use crate::tools;

pub const DEFAULT_MAX_ROUNDS: u32 = 32;

/// 跑一轮完整对话到 stop。messages 会被就地续长（调用方拿来回放/持久）。
/// 返回最终文本（stop 时模型的 content；max_rounds 兜底时为说明文本）。
pub fn run_turn<H: Host>(
    host: &H,
    client: &dyn ChatClient,
    session: &mut SessionWriter<'_, H>,
    model: &str,
    messages: &mut Vec<Message>,
    max_rounds: u32,
) -> Result<String, String> {
    let specs = tools::tool_specs();
    let mut round = 0u32;
    loop {
        round += 1;
        if round > max_rounds {
            let note = format!("已达 max_rounds={max_rounds} 兜底，强制停");
            session.done(max_rounds, "max_rounds", &note)?;
            return Ok(note);
        }
        let reply = client.chat(model, messages, &specs)?;
        // usage 账从第一轮就记（预埋 context 占用/交接协议）
        if let Some(u) = reply.usage {
            session.usage(round, u)?;
        }
        session.model_msg(round, reply.content.as_deref(), &reply.tool_calls)?;
        messages.push(Message::assistant(
            reply.content.clone(),
            reply.tool_calls.clone(),
        ));
        if reply.tool_calls.is_empty() {
            let text = reply.content.unwrap_or_default();
            session.done(round, "stop", &text)?;
            return Ok(text);
        }
        for tc in &reply.tool_calls {
            session.tool_call(round, &tc.id, &tc.function.name, &tc.function.arguments)?;
            let output = tools::execute(host, &tc.function.name, &tc.function.arguments);
            session.tool_result(round, &tc.id, &tc.function.name, &output)?;
            messages.push(Message::tool(&tc.id, output));
        }
    }
}
