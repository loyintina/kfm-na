//! ai_chat_spec.rs — AI 对话消息状态核考题（A 档纯逻辑，期 0③）
//!
//! 判卷维度：
//! - 发送入格 + 全量历史投影（OpenAI 无状态，每轮全量上传；role 串契约为
//!   "user"/"assistant"——build_chat_request 直接吃这对串）
//! - 流式一轮：MessageStart 开流 → TextDelta/ThinkingDelta 累积 →
//!   MessageStop 收流成消息；thinking+正文同块混排 v1 全收（简版纯文本）
//! - 收尾兜底：无 MessageStop 直 Done 也收流；Error 成错误消息且先收流
//! - snap = 已成消息 + 流式中尾巴（渲染读数，流式半截可见）
//!
//! 变异抽检：收流不清 streaming（鬼影尾巴）/ 投影漏 assistant 轮次
//! （多轮失忆）/ Error 不收流（错误后旧流续写）本文件必须红。
//! 答案 src/ai_chat.rs；判卷成本倒挂的 getter 不出题。

use kfm_na::ai_chat::AiChatState;
use kfm_na::brain::ChatEvent;

#[test]
fn spec_发送入格_历史投影全量有序() {
    let chat = AiChatState::new();
    let h1 = chat.user_send("第一问");
    assert_eq!(h1, vec![("user".to_string(), "第一问".to_string())]);
    //  assistant 轮次落格后，下一次投影必须带上（多轮记忆）
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::TextDelta {
        index: 0,
        text: "第一答".into(),
    });
    chat.apply(&ChatEvent::MessageStop);
    let h2 = chat.user_send("第二问");
    assert_eq!(
        h2,
        vec![
            ("user".to_string(), "第一问".to_string()),
            ("assistant".to_string(), "第一答".to_string()),
            ("user".to_string(), "第二问".to_string()),
        ],
        "OpenAI 无状态——历史投影必须全量有序，漏轮次 = 多轮失忆"
    );
}

#[test]
fn spec_流式一轮_thinking正文混排全收() {
    let chat = AiChatState::new();
    chat.user_send("问");
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::ThinkingDelta {
        index: 0,
        text: "想想".into(),
    });
    chat.apply(&ChatEvent::TextDelta {
        index: 0,
        text: "答答".into(),
    });
    chat.apply(&ChatEvent::MessageStop);
    let snap = chat.snap();
    assert_eq!(
        snap,
        vec![(true, "问".to_string()), (false, "想想答答".to_string()),],
        "thinking+正文同块混排（§四A），v1 简版全收进同一条 assistant 消息"
    );
}

#[test]
fn spec_流式半截_snap带尾巴() {
    let chat = AiChatState::new();
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::TextDelta {
        index: 0,
        text: "半截".into(),
    });
    let snap = chat.snap();
    assert_eq!(
        snap,
        vec![(false, "半截".to_string())],
        "流式进行中 snap 必须带半截尾巴——渲染尾随的读数"
    );
}

#[test]
fn spec_done兜底收流() {
    let chat = AiChatState::new();
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::TextDelta {
        index: 0,
        text: "没等到stop".into(),
    });
    chat.apply(&ChatEvent::Done);
    let snap = chat.snap();
    assert_eq!(snap, vec![(false, "没等到stop".to_string())]);
    assert!(
        !chat.is_streaming(),
        "Done 后 streaming 必清——不清就是下一条消息的鬼影开头"
    );
}

#[test]
fn spec_error收流且成错误消息() {
    let chat = AiChatState::new();
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::TextDelta {
        index: 0,
        text: "写了一半".into(),
    });
    chat.apply(&ChatEvent::Error {
        content: "API 请求失败: 401".into(),
    });
    let snap = chat.snap();
    assert_eq!(
        snap,
        vec![
            (false, "写了一半".to_string()),
            (false, "【错误】API 请求失败: 401".to_string()),
        ],
        "Error 先收流（半截不丢）再成人话错误消息——kfmv4 语义"
    );
    assert!(!chat.is_streaming());
}

#[test]
fn spec_工具事件_v1忽略不崩() {
    // tools 白名单 v1 全关（期 0③ 不放手），但解析器对 tool_use 块形状
    // 必须容忍——来了不崩不入格（§四A：tool_use 从 index=1 起）
    let chat = AiChatState::new();
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::ContentBlockStart {
        index: 1,
        tool_use: Some(("t1".into(), "read".into())),
    });
    chat.apply(&ChatEvent::InputJsonDelta {
        index: 1,
        text: "{\"path\":".into(),
    });
    chat.apply(&ChatEvent::ContentBlockStop { index: 1 });
    chat.apply(&ChatEvent::ToolResult {
        tool_use_id: "t1".into(),
        text: "内容".into(),
        is_error: false,
    });
    chat.apply(&ChatEvent::TextDelta {
        index: 0,
        text: "正文".into(),
    });
    chat.apply(&ChatEvent::Done);
    let snap = chat.snap();
    assert_eq!(
        snap,
        vec![(false, "正文".to_string())],
        "工具事件 v1 纯显示不入格，正文不落"
    );
}
