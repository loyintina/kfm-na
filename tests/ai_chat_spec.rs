//! ai_chat_spec.rs — AI 对话消息状态核考题（A 档纯逻辑，期 0③）
//!
//! 判卷维度：
//! - 发送入格 + 全量历史投影（OpenAI 无状态，每轮全量上传；role 串契约为
//!   "user"/"assistant"——build_chat_request 直接吃这对串）
//! - 流式一轮：MessageStart 开流 → TextDelta 累积正文 → MessageStop 收流
//!   成消息；ThinkingDelta 分账独存不进可见回复（BAR-059：思考不是回复，
//!   kfmv4 折叠块另渲染，期 0 纯文本消息行不画）；正文空 → 思考归位
//! - 收尾兜底：无 MessageStop 直 Done 也收流；Error 成错误消息且先收流
//! - snap = 已成消息 + 流式中尾巴（渲染读数，流式半截可见）
//!
//! 变异抽检：收流不清 streaming（鬼影尾巴）/ 投影漏 assistant 轮次
//! （多轮失忆）/ Error 不收流（错误后旧流续写）/ 思考混回正文（BAR-059
//! 旧行为复活）本文件必须红。
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
fn spec_bar059_思考分流不进可见回复() {
    // BAR-059（2026-09-04 期 0③ 真机首验实拍）：Kimi highspeed 的思考流
    // （reasoning_content → ThinkingDelta）混进可见回复——用户看见一整段
    // 英文内心戏。契约：思考不是回复，分账独存（第三字段）；期 0④½ 起
    // 渲染成 ≤3 行暗色尾随块（用户拍板：限制行数自己滚动，不许占满屏）
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
    // 流式中途：正文/思考分账各就各位（第三字段 = 思考独存账户）
    let mid = chat.snap();
    assert_eq!(
        mid,
        vec![
            (true, "问".to_string(), String::new()),
            (false, "答答".to_string(), "想想".to_string()),
        ],
        "流式中途 snap 尾巴 = 正文/思考分账；思考混进正文字段就是 BAR-059 复活"
    );
    chat.apply(&ChatEvent::MessageStop);
    let snap = chat.snap();
    assert_eq!(
        snap,
        vec![
            (true, "问".to_string(), String::new()),
            (false, "答答".to_string(), "想想".to_string()),
        ],
        "收流成消息 = 分账随消息存档（期 0④½ 起渲染成 ≤3 行暗色块，不再舍弃）"
    );
}

#[test]
fn spec_bar059_正文空思考归位为正文() {
    // 归位判据与 brain.rs RunAccumulator 同源（kfmv4 陷阱 10 / R3）：
    // 某些模型把回复错放 reasoning——正文空且思考非空时，思考顶上，
    // 不许产出空回复
    let chat = AiChatState::new();
    chat.apply(&ChatEvent::MessageStart);
    chat.apply(&ChatEvent::ThinkingDelta {
        index: 0,
        text: "错放reasoning的真回复".into(),
    });
    chat.apply(&ChatEvent::Done);
    let snap = chat.snap();
    assert_eq!(
        snap,
        vec![(false, "错放reasoning的真回复".to_string(), String::new())],
        "正文空 + 思考非空 → 思考归位为正文（取消残留不归位，期 0 无取消路径）"
    );
    assert!(!chat.is_streaming());
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
        vec![(false, "半截".to_string(), String::new())],
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
    assert_eq!(snap, vec![(false, "没等到stop".to_string(), String::new())]);
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
            (false, "写了一半".to_string(), String::new()),
            (
                false,
                "【错误】API 请求失败: 401".to_string(),
                String::new()
            ),
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
        vec![(false, "正文".to_string(), String::new())],
        "工具事件 v1 纯显示不入格，正文不落"
    );
}

#[test]
fn spec_视口接线_状态机挂在chat上() {
    // 期 0④：手势/渲染不直接摸 AiPageScroll，走 AiChatState 三件套——
    // 这题钉的是接线本身（drag 改了 offset、sync 喂了上界、follow 读数真）
    let chat = AiChatState::new();
    assert!(chat.scroll_follow(), "出厂追底");
    chat.scroll_sync_layout(100, 10);
    chat.scroll_drag_rows(7);
    assert_eq!(chat.scroll_offset(), 7);
    assert!(!chat.scroll_follow());
    chat.scroll_drag_rows(-7);
    assert_eq!(chat.scroll_offset(), 0);
    assert!(chat.scroll_follow(), "滑回底恢复追底");
}

#[test]
fn spec_思考相位_正文一出立即翻折() {
    // 2026-09-04 用户拍板：三行滚动思考一结束就折叠，不等整轮收流
    // （kfmv4 同判据：首块正文到 → 思考框折）。thinking_live = 渲染
    // live_tail 的唯一数据源：思考先行期 true，首个 TextDelta 落地
    // 即 false，收流后恒 false
    let chat = AiChatState::new();
    assert!(!chat.thinking_live(), "空闲恒 false");
    chat.apply(&ChatEvent::MessageStart);
    assert!(!chat.thinking_live(), "刚开流还没思考 = false");
    chat.apply(&ChatEvent::ThinkingDelta {
        index: 0,
        text: "想".into(),
    });
    assert!(chat.thinking_live(), "思考先行期 = true（活窗）");
    chat.apply(&ChatEvent::ThinkingDelta {
        index: 0,
        text: "更多".into(),
    });
    assert!(chat.thinking_live(), "思考累积中仍 true");
    chat.apply(&ChatEvent::TextDelta {
        index: 1,
        text: "正".into(),
    });
    assert!(!chat.thinking_live(), "首个正文落地 = 思考结束立即折");
    chat.apply(&ChatEvent::TextDelta {
        index: 1,
        text: "文".into(),
    });
    assert!(!chat.thinking_live(), "正文流式中恒 false");
    chat.apply(&ChatEvent::Done);
    assert!(!chat.thinking_live(), "收流后恒 false");
}
