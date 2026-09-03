//! ai_chat.rs — AI 对话消息状态核（期 0③，A 档纯逻辑）。
//!
//! 契约真相源：docs/active/ai-presence.md §五（发送流）/ §四A（九事件）。
//! v1 简版纯文本：user/assistant 消息列表 + 流式累积 + 全量历史投影
//! （OpenAI 无状态，每轮全量上传——token 不缺，用户 2026-08-31 拍板）。
//! thinking+正文同块混排（§四A index=0 恒为 text 块），v1 全收进同一条
//! assistant 消息；工具事件 v1 纯容忍不入格（tools 白名单全关）。
//!
//! 发送方（脑线程）apply 事件、渲染方（事件循环）snap 读数——跨线程，
//! 与 ai_presence/input_bar 同 pattern：状态核内自持 Mutex。
//! markdown-lite/合成网格美化是期 0⑤，本核只管消息语义不管排版。

use crate::brain::ChatEvent;
use std::sync::Mutex;

#[derive(Default)]
struct Inner {
    /// 已成消息（(is_user, text)）
    messages: Vec<(bool, String)>,
    /// 流式中的 assistant 半截（MessageStart 开、MessageStop/Done/Error 收）
    streaming: Option<String>,
}

pub struct AiChatState {
    inner: Mutex<Inner>,
    /// 代际计数：每次 user_send/apply +1——置脏比对用（脑线程流式落格
    /// 不经触摸/快照，事件循环拿它判「该画帧了」）
    generation: std::sync::atomic::AtomicU64,
}

impl AiChatState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            generation: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 代际读数（置脏比对；判卷成本倒挂不出题）
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 发送：用户消息入格，返回全量历史投影（含刚入的这条）。
    /// role 串契约 = "user"/"assistant"，build_chat_request 直吃。
    pub fn user_send(&self, text: &str) -> Vec<(String, String)> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        g.messages.push((true, text.to_string()));
        g.messages
            .iter()
            .map(|(is_user, t)| {
                (
                    if *is_user { "user" } else { "assistant" }.to_string(),
                    t.clone(),
                )
            })
            .collect()
    }

    /// 流事件落格（脑线程调用）。九事件里 v1 只消费五个：
    /// MessageStart/TextDelta/ThinkingDelta/MessageStop/Done/Error——
    /// 工具块（ContentBlockStart tool_use/InputJsonDelta/ToolResult）容忍忽略。
    pub fn apply(&self, ev: &ChatEvent) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match ev {
            ChatEvent::MessageStart => {
                // 防御：上一轮没收尾（异常流）先收掉，不丢半截
                Self::flush(&mut g);
                g.streaming = Some(String::new());
            }
            ChatEvent::TextDelta { text, .. } | ChatEvent::ThinkingDelta { text, .. } => {
                // 容忍无 MessageStart 直出 delta（野脑）——隐式开流
                g.streaming.get_or_insert_with(String::new).push_str(text);
            }
            ChatEvent::MessageStop | ChatEvent::Done => Self::flush(&mut g),
            ChatEvent::Error { content } => {
                // 先收流（半截不丢）再成人话错误消息——kfmv4 error 事件语义
                Self::flush(&mut g);
                g.messages.push((false, format!("【错误】{content}")));
            }
            _ => {} // 工具事件 v1 不入格
        }
    }

    /// 渲染快照：已成消息 + 流式中尾巴（(is_user, text)，出生序）
    pub fn snap(&self) -> Vec<(bool, String)> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = g.messages.clone();
        if let Some(s) = &g.streaming {
            out.push((false, s.clone()));
        }
        out
    }

    /// 流式进行中（观测/判卷用；渲染不需要——snap 已含尾巴）
    pub fn is_streaming(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .streaming
            .is_some()
    }

    /// 收流：半截 assistant 成消息（空流不产空消息）
    fn flush(g: &mut Inner) {
        if let Some(s) = g.streaming.take()
            && !s.is_empty()
        {
            g.messages.push((false, s));
        }
    }
}

impl Default for AiChatState {
    fn default() -> Self {
        Self::new()
    }
}
