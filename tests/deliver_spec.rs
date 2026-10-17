//! deliver_spec.rs — BAR-252 客户端投递路由考题（追踪信 NA0181；设计楼1，
//! 裁决白露楼2 四点全裁）。
//!
//! 判卷维度：路由判定真值表／请求体与回执解析（与服务端同形往返）／
//! 投成小字降级透出／bar-inject 投递三形解析／瞬态小字 TTL 与编辑清除／
//! 接线守卫（发送闭包分叉、渲染 hint 臂、值守 Deliver 臂、词表同票）。

use kfm_na::ai_presence::{Page, Panel};
use kfm_na::deliver::{Action, Route, decide, hint_for, parse_resp, request_json};
use kfm_na::gate::{BarCmd, parse_bar_line};
use kfm_na::input_bar::{HINT_TTL_MS, InputBarState};

#[test]
fn spec_bar252_路由判定_真值表() {
    // 窗路：终页 + 栈顶空 + 附着名在 + 已装配
    assert_eq!(
        decide(Page::Terminal, None, Some("清和"), None, true),
        Route::Window("清和".to_string())
    );
    // AI 路无提示：AI 全屏页本归 AI；面板开着 = 人没看窗
    assert_eq!(
        decide(Page::AiFullscreen, None, Some("清和"), None, true),
        Route::Ai(None)
    );
    assert_eq!(
        decide(Page::Terminal, Some(Panel::Ai), Some("清和"), None, true),
        Route::Ai(None)
    );
    assert_eq!(
        decide(
            Page::Terminal,
            Some(Panel::FileTree),
            Some("清和"),
            None,
            true
        ),
        Route::Ai(None)
    );
    // AI 路无提示：投递面未装配（kfmv4 后端）= 旧行为零变化
    assert_eq!(
        decide(Page::Terminal, None, Some("清和"), None, false),
        Route::Ai(None)
    );
    // AI 路带提示：裸 shell 归 AI 须不静默（白露楼2④）
    match decide(Page::Terminal, None, None, None, true) {
        Route::Ai(Some(h)) => {
            assert!(h.contains("裸 shell") && h.contains("AI 页"), "{h}")
        }
        other => panic!("裸 shell 应归 AI 且带提示，实得 {other:?}"),
    }
}

#[test]
fn spec_bar252_请求体与回执_往返同形() {
    // 请求体四键
    let j: serde_json::Value =
        serde_json::from_str(&request_json("清和", Action::Send, "你好")).unwrap();
    assert_eq!(j["v"], 1);
    assert_eq!(j["target"], "清和");
    assert_eq!(j["action"], "send");
    assert_eq!(j["text"], "你好");
    // 中文/引号转义不破（serde 单源）
    let j: serde_json::Value =
        serde_json::from_str(&request_json("悬铃", Action::Queue, "引\"号")).unwrap();
    assert_eq!(j["text"], "引\"号");
    // 回执解析：五键吃三面；降级形；坏形 None
    let r = parse_resp(
        r#"{"ok":true,"target":"清和","action":"queue","action_effective":"send","detail":"kimi 无排队键——已降级为发送"}"#,
    )
    .unwrap();
    assert!(r.ok);
    assert_eq!(r.action_effective, "send");
    assert!(r.detail.contains("降级"));
    let r = parse_resp(
        r#"{"ok":false,"target":"proxy","action":"send","action_effective":"send","detail":"窗 proxy 是常驻服务窗（不接投递）——改投对话窗，或发往 AI 页"}"#,
    )
    .unwrap();
    assert!(!r.ok);
    assert!(r.detail.contains("AI 页"));
    assert!(parse_resp("not json").is_none());
    assert!(parse_resp(r#"{"ok":true}"#).is_none());
}

#[test]
fn spec_bar252_投成小字_降级透出() {
    // 同动作：直述
    assert_eq!(hint_for("清和", Action::Send, "send"), "→ 清和·发送");
    assert_eq!(hint_for("悬铃", Action::Queue, "queue"), "→ 悬铃·排队");
    assert_eq!(
        hint_for("清和", Action::Interrupt, "interrupt"),
        "→ 清和·打断"
    );
    // 降级：「排队（已降级为发送）」——白露楼2①「否则用户以为排上了」
    assert_eq!(
        hint_for("清和", Action::Queue, "send"),
        "→ 清和·排队（已降级为发送）"
    );
    // 未知 effective 不炸（旧服务端无此键之外的坏形兜底）
    assert_eq!(hint_for("清和", Action::Send, "???"), "→ 清和·发送");
}

#[test]
fn spec_bar252_bar_inject_投递三形解析() {
    assert_eq!(
        parse_bar_line("deliver send 你好 世界"),
        Some(Ok(BarCmd::Deliver(Action::Send, "你好 世界".to_string())))
    );
    assert_eq!(
        parse_bar_line("deliver interrupt"),
        Some(Ok(BarCmd::Deliver(Action::Interrupt, String::new())))
    );
    assert_eq!(
        parse_bar_line("deliver queue 排着"),
        Some(Ok(BarCmd::Deliver(Action::Queue, "排着".to_string())))
    );
    // 坏动作 fail-closed
    assert!(matches!(parse_bar_line("deliver explode x"), Some(Err(_))));
}

#[test]
fn spec_bar252_瞬态小字_ttl与编辑清除() {
    let bar = InputBarState::new();
    // TTL 内可见、到期消（snap_at 注入时刻——host boot_ms 恒 0）
    bar.set_hint_at("→ 清和·发送", 1000);
    assert_eq!(bar.snap_at(1000).hint, "→ 清和·发送");
    assert_eq!(bar.snap_at(1000 + HINT_TTL_MS - 1).hint, "→ 清和·发送");
    assert_eq!(bar.snap_at(1000 + HINT_TTL_MS).hint, "");
    // 编辑清除：打字
    bar.set_hint_at("x", 0);
    bar.insert_text("a");
    assert_eq!(bar.snap_at(0).hint, "");
    // 发送清除（submit 消费后新回执才写新小字——顺序不许倒）
    let bar2 = InputBarState::new();
    bar2.insert_text("hi");
    bar2.set_hint_at("y", 0);
    bar2.submit();
    assert_eq!(bar2.snap_at(0).hint, "");
    // 空串 = 立即清
    bar.set_hint_at("z", 0);
    bar.set_hint_at("", 0);
    assert_eq!(bar.snap_at(0).hint, "");
    // 生产壳 set_hint（boot_ms 注时刻，host 恒 0）同路
    bar.set_hint("w");
    assert_eq!(bar.snap_at(0).hint, "w");
}

/// 渲染臂：空栏有 hint 时代占位符画出（文本行有墨）；hint 空 = 占位符
/// 照旧（回归闸——占位符渲染被 hint 臂吃掉就红）
#[test]
fn spec_bar252_空栏hint渲染上屏() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let mk = |hint: &str| kfm_na::input_bar::BarSnap {
        text: String::new(),
        focused: false,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
        hint: hint.to_string(),
    };
    let render = |snap: &kfm_na::input_bar::BarSnap| {
        let mut buf = vec![0u32; (w * h) as usize];
        tv.render_inputbar(&mut buf, w, h, 0, snap, false, false);
        buf
    };
    // 字墨 = field 内芯带里的亮像素（任一通道 > 80）。带底/内芯渐变/描边
    // 全带非零（数非零像素没有分辨力——本钉首版就这么红的）：内芯渐变
    // 最深 (29,23,57)，占位灰 0x616165 与品牌青 0x00D4FF 双双过阈；
    // 内缩 3px 排掉描边亮行（border_l 0x7C3AED 过阈）。x 取中段避开
    // 左右描边与发送钮。
    let bright_ink = |buf: &[u32]| {
        let top = (0..h)
            .find(|&y| (0..w).any(|x| buf[(y * w + x) as usize] & 0x00FF_FFFF != 0))
            .expect("栏带必画");
        let (ft, fh) = (top + 32, h - top - 64); // field：带内上下各收 32
        let mut n = 0usize;
        for y in (ft + 3)..(ft + fh - 3) {
            for x in (w / 4)..(w * 3 / 4) {
                let c = buf[(y * w + x) as usize];
                if (c & 0xFF) > 80 || ((c >> 8) & 0xFF) > 80 || ((c >> 16) & 0xFF) > 80 {
                    n += 1;
                }
            }
        }
        n
    };
    let buf_hint = render(&mk("→ 清和·发送"));
    let buf_ph = render(&mk(""));
    let hint_ink = bright_ink(&buf_hint);
    let placeholder_ink = bright_ink(&buf_ph);
    assert!(hint_ink > 0, "hint 应画出字墨");
    assert!(placeholder_ink > 0, "占位符应照旧画出（回归闸）");
    // 两文本不同形（hint 不是占位符换色——文案真换了）：像素级比对，
    // 不押字墨计数碰巧不等
    let diff = buf_hint
        .iter()
        .zip(buf_ph.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(diff > 0, "hint 与占位符渲染应不同（diff={diff}）");
}

/// 接线守卫四锚（源码咬——防「钉还在、线被摘」）：
/// ① 发送闭包分叉 route_now ② 渲染 hint 臂 ③ 值守 Deliver 臂 +
/// deliver-res 落盘 ④ 词表与服务端同票（send/interrupt/queue 三形同字）
#[test]
fn spec_bar252_接线守卫() {
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("crate::deliver::route_now()")
            && app.contains("crate::deliver::deliver_spawn"),
        "发送闭包应分叉 deliver 路由"
    );
    assert!(
        app.contains("crate::deliver::configure(srv)"),
        "na-server 后端装配处应登记投递面配置"
    );
    let bar_rs = include_str!("../src/ui/prompt_bar.rs");
    assert!(
        bar_rs.contains("snap.hint"),
        "render_inputbar 空栏臂应画 snap.hint"
    );
    let gate_rs = include_str!("../src/gate.rs");
    assert!(
        gate_rs.contains("BarCmd::Deliver") && gate_rs.contains("deliver-res"),
        "值守线程应收 Deliver 指令并落 deliver-res"
    );
    // 词表同票：客户端三形字面与服务端 deliver.rs 同源
    let server_rs = include_str!("../crates/na-server/src/deliver.rs");
    for w in ["\"send\"", "\"interrupt\"", "\"queue\""] {
        assert!(server_rs.contains(w), "服务端词表缺 {w}");
    }
    for a in [Action::Send, Action::Interrupt, Action::Queue] {
        assert!(
            server_rs.contains(&format!("\"{}\"", a.as_str())),
            "服务端词表缺 {}",
            a.as_str()
        );
    }
}
