//! ai_chat.rs — AI 对话消息状态核（期 0③，A 档纯逻辑）。
//!
//! 契约真相源：docs/active/ai-presence.md §五（发送流）/ §四A（九事件）。
//! v1 简版纯文本：user/assistant 消息列表 + 流式累积 + 全量历史投影
//! （OpenAI 无状态，每轮全量上传——token 不缺，用户 2026-08-31 拍板）。
//! thinking 与正文分流独存（BAR-059）：§四A 线路上 index=0 同块混排靠
//! deltaType 分流，本核同样分账——思考不是回复（kfmv4 渲染成「已思考」
//! 折叠块另存，期 0 纯文本消息行不画思考，折叠块是期 0④⑤ 的活）；
//! 收流时正文空且思考非空 → 思考归位为正文（kfmv4 陷阱 10 / R3，
//! 与 brain.rs RunAccumulator 同判据）。工具事件 v1 纯容忍不入格
//! （tools 白名单全关）。
//!
//! 发送方（脑线程）apply 事件、渲染方（事件循环）snap 读数——跨线程，
//! 与 ai_presence/input_bar 同 pattern：状态核内自持 Mutex。
//! markdown-lite/合成网格美化是期 0⑤，本核只管消息语义不管排版。

use crate::brain::ChatEvent;
use std::sync::Mutex;

#[derive(Default)]
struct Inner {
    /// 已成消息（(is_user, text, thinking)——思考分账随消息存档，
    /// 期 0④½ 起渲染成 ≤3 行暗色尾随块，不再是收流即弃）
    messages: Vec<(bool, String, String)>,
    /// 流式中的 assistant 半截正文（MessageStart 开、MessageStop/Done/Error 收）
    streaming: Option<String>,
    /// 流式中的思考缓冲（BAR-059 与正文分账：不进正文行，渲染走
    /// ≤3 行暗色尾随块——期 0④½；收流归位判据不变）
    thinking: String,
}

pub struct AiChatState {
    inner: Mutex<Inner>,
    /// 代际计数：每次 user_send/apply +1——置脏比对用（脑线程流式落格
    /// 不经触摸/快照，事件循环拿它判「该画帧了」）
    generation: std::sync::atomic::AtomicU64,
    /// 对话页视口（期 0④：滚动/追底状态机，ui/ai_page.rs 纯逻辑）
    scroll: Mutex<crate::ui::ai_page::AiPageScroll>,
}

impl AiChatState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            generation: std::sync::atomic::AtomicU64::new(0),
            scroll: Mutex::new(crate::ui::ai_page::AiPageScroll::new()),
        }
    }

    /// 对话页视口三件套（期 0④）：手势拖行 / 布局写回 / 渲染读偏移。
    /// 判卷成本倒挂（转发）不出题——状态机本身的考题在 ai_page_scroll_spec
    pub fn scroll_drag_rows(&self, delta: i32) {
        self.scroll
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drag_rows(delta);
    }

    pub fn scroll_sync_layout(&self, total: u32, fit: u32) {
        self.scroll
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sync_layout(total, fit);
    }

    pub fn scroll_offset(&self) -> u32 {
        self.scroll
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .offset()
    }

    pub fn scroll_follow(&self) -> bool {
        self.scroll
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .follow()
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
        g.messages.push((true, text.to_string(), String::new()));
        g.messages
            .iter()
            .map(|(is_user, t, _)| {
                (
                    if *is_user { "user" } else { "assistant" }.to_string(),
                    t.clone(),
                )
            })
            .collect()
    }

    /// 流事件落格（脑线程调用）。九事件里 v1 只消费六个：
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
            ChatEvent::TextDelta { text, .. } => {
                // 容忍无 MessageStart 直出 delta（野脑）——隐式开流
                g.streaming.get_or_insert_with(String::new).push_str(text);
            }
            ChatEvent::ThinkingDelta { text, .. } => {
                // BAR-059：思考与正文分账——隐式开流保 is_streaming 语义
                // （思考先行阶段 = 回复已在流式），但一个字不进可见正文
                g.streaming.get_or_insert_with(String::new);
                g.thinking.push_str(text);
            }
            ChatEvent::MessageStop | ChatEvent::Done => Self::flush(&mut g),
            ChatEvent::Error { content } => {
                // 先收流（半截不丢）再成人话错误消息——kfmv4 error 事件语义
                Self::flush(&mut g);
                g.messages
                    .push((false, format!("【错误】{content}"), String::new()));
            }
            _ => {} // 工具事件 v1 不入格
        }
    }

    /// 渲染快照：已成消息 + 流式中尾巴（(is_user, text, thinking)，
    /// 出生序）。流式尾巴带活思考——思考先行的阶段这就是可见的
    /// 「自己滚动」暗色块（期 0④½）
    pub fn snap(&self) -> Vec<(bool, String, String)> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = g.messages.clone();
        if let Some(s) = &g.streaming {
            out.push((false, s.clone(), g.thinking.clone()));
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

    /// 思考相位进行中（2026-09-04 用户拍板：思考一结束就折叠，不等
    /// 整轮收流——kfmv4 同判据：首块正文到 → 思考框折）。判据 = 流式
    /// 开着且正文还一个字没来（思考先行阶段）；首个 TextDelta 落地
    /// 即翻 false → 渲染折成「· 已思考 ·」
    pub fn thinking_live(&self) -> bool {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.streaming.as_ref().is_some_and(|s| s.is_empty()) && !g.thinking.is_empty()
    }

    /// 收流：半截 assistant 成消息（空流不产空消息）。BAR-059 归位：
    /// 正文空且思考非空 → 思考归位为正文（kfmv4 陷阱 10 / R3，与
    /// brain.rs RunAccumulator 同判据）；正文非空则思考随消息存档
    /// （期 0④½ 起渲染成 ≤3 行暗色尾随块——不再整段舍弃）
    fn flush(g: &mut Inner) {
        let text = g.streaming.take().unwrap_or_default();
        let thinking = std::mem::take(&mut g.thinking);
        if text.is_empty() {
            if !thinking.is_empty() {
                g.messages.push((false, thinking, String::new()));
            }
        } else {
            g.messages.push((false, text, thinking));
        }
    }
}

impl Default for AiChatState {
    fn default() -> Self {
        Self::new()
    }
}
