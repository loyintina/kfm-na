//! brain.rs — AI 脑协议层（纯逻辑，零 IO）。
//!
//! 契约真相源：docs/active/ai-presence.md §四
//! - 四A 内部九事件协议（UI 面，脑无关）：kfmv4 shared/chat-protocol/events.ts 血统
//! - 四B 上游 OpenAI 协议（direct-api-brain 对外面）：双 fixture 互证
//!   tests/fixtures/ai-chat/upstream-{kimi-k2.7-highspeed,glm-5.3-flash}-20260830.sse
//!
//! 分层纪律：本模块只做字节→事件、事件→累积态的纯函数变换，不碰 socket/线程。
//! IO 由 direct-api-brain 插件（期 0② 后半）挂到本模块上。

use serde_json::json;

// ========== 四A·内部九事件（delta 按 deltaType 拆变体，判卷更顺手） ==========

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    MessageStart,
    /// text 块 tool_use=None；tool_use 块 = Some((tool_use_id, tool_name))
    ContentBlockStart {
        index: u32,
        tool_use: Option<(String, String)>,
    },
    TextDelta {
        index: u32,
        text: String,
    },
    ThinkingDelta {
        index: u32,
        text: String,
    },
    InputJsonDelta {
        index: u32,
        text: String,
    },
    ContentBlockStop {
        index: u32,
    },
    ToolResult {
        tool_use_id: String,
        text: String,
        is_error: bool,
    },
    MessageStop,
    Done,
    Error {
        content: String,
    },
    RuleWarning {
        content: String,
    },
}

// ========== SSE 帧解析器（四B/四C 共用骨架：data: 行 + 空行分隔，无 event: 行） ==========

/// 增量 SSE 解析器：碎喂/粘包/半帧/CRLF/注释行全容忍。
/// 产出 = 每帧的 data 载荷（多行 data 按 SSE 规范以 \n 拼接）。
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入任意切块的字节流。
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// 取下一个完整帧的载荷；无完整帧返回 None（半帧暂存不吐）。
    pub fn next_frame(&mut self) -> Option<String> {
        loop {
            let nl = self.buf.iter().position(|&b| b == b'\n')?;
            let mut line: Vec<u8> = self.buf.drain(..=nl).collect();
            line.pop(); // \n
            if line.last() == Some(&b'\r') {
                line.pop(); // CRLF 容忍
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            if line.is_empty() {
                // 空行 = 帧界；只有攒了 data 才成帧（连续空行不算帧）
                if self.data_lines.is_empty() {
                    continue;
                }
                return Some(std::mem::take(&mut self.data_lines).join("\n"));
            }
            if line.starts_with(':') {
                continue; // SSE 注释行
            }
            if let Some(rest) = line.strip_prefix("data:") {
                // 规范：冒号后至多去掉一个前导空格
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                self.data_lines.push(rest.to_string());
            }
            // event:/id:/retry: 等字段四B/四C 均不使用，静默忽略
        }
    }

    /// 便捷：把 buffer 里当前所有完整帧取空。
    pub fn drain_frames(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(f) = self.next_frame() {
            out.push(f);
        }
        out
    }
}

// ========== 上游 OpenAI chunk → 四A 事件翻译器 ==========

/// 状态极薄：首个内容帧懒发 MessageStart+ContentBlockStart（对齐 kfmv4 事件序）。
#[derive(Default)]
pub struct OpenAiTranslator {
    started: bool,
}

impl OpenAiTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    fn lazy_start(&mut self, out: &mut Vec<ChatEvent>) {
        if !self.started {
            self.started = true;
            out.push(ChatEvent::MessageStart);
            out.push(ChatEvent::ContentBlockStart {
                index: 0,
                tool_use: None,
            });
        }
    }

    /// 翻译一帧 SSE 载荷为 0..N 个内部事件。
    /// 静默帧（零事件）是常态：role-only / 空 delta / usage-only / [DONE] 外的杂项。
    /// 坏 JSON 帧 → 单个 Error 事件（入流不例外，四B 错误语义）。
    pub fn translate_payload(&mut self, payload: &str) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        let payload = payload.trim();
        if payload == "[DONE]" {
            out.push(ChatEvent::Done);
            return out;
        }
        let chunk: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(e) => {
                out.push(ChatEvent::Error {
                    content: format!("上游帧 JSON 解析失败: {e}"),
                });
                return out;
            }
        };
        let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) else {
            return out; // 无 choices（异常形状），静默容忍
        };
        let Some(choice) = choices.first() else {
            return out; // choices: [] usage-only 帧（Kimi 方言），记账不进事件流
        };
        let empty_delta;
        let delta = match choice.get("delta") {
            Some(d) => d,
            None => {
                empty_delta = json!({});
                &empty_delta
            }
        };
        // role 字段：Kimi 仅首帧、GLM 每帧重复——一律忽略（方言表已登记）
        if let Some(t) = delta.get("reasoning_content").and_then(|v| v.as_str())
            && !t.is_empty()
        {
            self.lazy_start(&mut out);
            out.push(ChatEvent::ThinkingDelta {
                index: 0,
                text: t.to_string(),
            });
        }
        if let Some(t) = delta.get("content").and_then(|v| v.as_str())
            && !t.is_empty()
        {
            self.lazy_start(&mut out);
            out.push(ChatEvent::TextDelta {
                index: 0,
                text: t.to_string(),
            });
        }
        // delta.tool_calls（OpenAI 碎片格式）：期 2 手再翻译，期 0 容忍不炸
        if choice
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .is_some()
            && self.started
        {
            out.push(ChatEvent::ContentBlockStop { index: 0 });
            out.push(ChatEvent::MessageStop);
        }
        out
    }
}

// ========== 运行累积器（含 reasoning 归位，kfmv4 陷阱 10 / 风险 R3） ==========

/// 把事件流累积成终态消息。归位判据：正常结束（MessageStop/Done）时
/// 正文空且思考非空 → 思考归位为正文（取消残留不归位——期 0 无取消路径，从简）。
#[derive(Default)]
pub struct RunAccumulator {
    text: String,
    thinking: String,
    relocated: bool,
}

impl RunAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, ev: &ChatEvent) {
        match ev {
            ChatEvent::TextDelta { text, .. } => self.text.push_str(text),
            ChatEvent::ThinkingDelta { text, .. } => self.thinking.push_str(text),
            ChatEvent::MessageStop | ChatEvent::Done => self.maybe_relocate(),
            _ => {}
        }
    }

    fn maybe_relocate(&mut self) {
        if self.text.is_empty() && !self.thinking.is_empty() {
            self.text = std::mem::take(&mut self.thinking);
            self.relocated = true;
        }
    }

    pub fn final_text(&self) -> &str {
        &self.text
    }
    pub fn thinking(&self) -> &str {
        &self.thinking
    }
    /// 是否发生了归位（观测用：某些模型把回复错放 reasoning 的实锤信号）
    pub fn relocated(&self) -> bool {
        self.relocated
    }
}

// ========== 上游 HTTP 错误体 → Error 事件（kfmv4 chat.ts 口径） ==========

/// 非 200 响应体转人话 Error 事件：`API 请求失败: <status> — <message>`。
/// 能解析出 error.message 用 message（两家 401 体形状不同但都有它）；
/// 否则原样截断 300 字符（kfmv4 同款上限，防巨型 HTML 错误页糊脸）。
pub fn error_event_from_http(status: u16, body: &str) -> ChatEvent {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error")?.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| {
            const CAP: usize = 300;
            if body.chars().count() <= CAP {
                body.to_string()
            } else {
                body.chars().take(CAP).collect()
            }
        });
    ChatEvent::Error {
        content: format!("API 请求失败: {status} — {detail}"),
    }
}

// ========== 上游请求体构造（OpenAI chat/completions 流式形态） ==========

/// messages = (role, content) 纯文本对（期 0 简版；tool_calls 投影随期 2）。
pub fn build_chat_request(model: &str, messages: &[(String, String)]) -> String {
    let msgs: Vec<serde_json::Value> = messages
        .iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect();
    json!({
        "model": model,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": msgs,
    })
    .to_string()
}
