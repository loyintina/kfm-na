//! boot_mark_spec.rs — BAR-256 冷启动引导附着账考题（追踪信 NA0185；
//! 设计楼3，白露楼4 改 Pending 不落 AI 路，南舟楼2 三判据点）。
//!
//! 判卷维度：剥离器流式状态机（单 chunk／分片重组／普通 OSC 不误剥／
//! 剥不动透传／ST 终结）／四值账迁移（Pending→Stripped/Missed/Bad，
//! 超窗不静默）／路由真值表（Pending=Hold 留栏不落 AI——白露楼4）／
//! 引导脚本打标覆盖（每个 exec 必有 mark）／泵接线（三消费方全净）。

use kfm_na::ai_presence::{Page, Panel};
use kfm_na::boot_mark::{
    Apply, BootMark, BootStrip, INTRO, MARK_CAP, Record, Seg, WINDOW_MS, close, open, peek, record,
    take_apply, tick,
};
use kfm_na::deliver::{Route, decide};

/// 拼标记字节：ESC ] 7777 ; kfmboot= <payload> BEL
fn mark(payload: &str) -> String {
    format!("{INTRO}{payload}\x07")
}

fn clean_of(segs: &[Seg]) -> String {
    segs.iter()
        .filter_map(|s| match s {
            Seg::Clean(c) => Some(c.as_str()),
            _ => None,
        })
        .collect()
}

fn marks_of(segs: &[Seg]) -> Vec<&str> {
    segs.iter()
        .filter_map(|s| match s {
            Seg::Mark { payload, .. } => Some(payload.as_str()),
            _ => None,
        })
        .collect()
}

// ---- A. 剥离器 ----

#[test]
fn spec_bar256_单chunk完整标记_剥出且两侧文本保序() {
    let mut st = BootStrip::new();
    let segs = st.feed(&format!("abc{}def", mark("清和")));
    assert_eq!(clean_of(&segs), "abcdef");
    assert_eq!(marks_of(&segs), vec!["清和"]);
    // 顺序：Clean(abc) → Mark → Clean(def)
    assert!(matches!(&segs[0], Seg::Clean(c) if c == "abc"));
    assert!(matches!(&segs[1], Seg::Mark { .. }));
    assert!(matches!(&segs[2], Seg::Clean(c) if c == "def"));
}

#[test]
fn spec_bar256_分片样本_跨chunk重组必剥() {
    // 南舟楼2③：OSC 切两半喂进字节流，剥离器跨 chunk 兜得住。
    // 三种切法都要剥：introducer 断／payload 断／终结符孤悬。
    let full = format!("pre{}post", mark("悬铃"));
    let n = full.len();
    for cut1 in 1..n {
        for cut2 in cut1 + 1..=n {
            // 只按 UTF-8 边界切（串事件是 String，不会断在字符腰里）
            if !full.is_char_boundary(cut1) || !full.is_char_boundary(cut2) {
                continue;
            }
            let mut st = BootStrip::new();
            let mut out = st.feed(&full[..cut1]);
            out.extend(st.feed(&full[cut1..cut2]));
            out.extend(st.feed(&full[cut2..]));
            assert_eq!(clean_of(&out), "prepost", "切点 {cut1}/{cut2}");
            assert_eq!(marks_of(&out), vec!["悬铃"], "切点 {cut1}/{cut2}");
        }
    }
}

#[test]
fn spec_bar256_普通osc穿过不误剥() {
    let mut st = BootStrip::new();
    // 标题 OSC／剪贴板 OSC／超链接 OSC 都不是我们的私有键
    let input = "x\x1b]0;窗口标题\x07y\x1b]52;c;aGVsbG8=\x07z\x1b]8;;http://e\x1b\\w";
    let segs = st.feed(input);
    assert_eq!(clean_of(&segs), input, "普通 OSC 一个字节都不许动");
    assert!(marks_of(&segs).is_empty());
}

#[test]
fn spec_bar256_剥不动_超上限原样透传() {
    // 白露①③：剥不动就原样透传——永无终结符的标记不能吞掉后续全部字节。
    // 护栏 = MARK_CAP：超过上限未终结 ⇒ 已吞字节原样吐回，后续直通过。
    let mut st = BootStrip::new();
    let head = format!("{INTRO}{}", "啊".repeat(MARK_CAP)); // 超上限仍无终结
    let segs = st.feed(&head);
    assert_eq!(clean_of(&segs), head, "剥不动的字节必须原样吐回");
    assert!(marks_of(&segs).is_empty());
    // 吐回后状态机复位：后续正常字节不再被吞
    let segs2 = st.feed("正常后续");
    assert_eq!(clean_of(&segs2), "正常后续");
}

#[test]
fn spec_bar256_st终结形也认() {
    let mut st = BootStrip::new();
    let segs = st.feed(&format!("a{INTRO}观澜\x1b\\b"));
    assert_eq!(clean_of(&segs), "ab");
    assert_eq!(marks_of(&segs), vec!["观澜"]);
}

// ---- B. 四值账 ----

#[test]
fn spec_bar256_账迁移_窗内到标_stripped且幂等取用() {
    let leg = "t-256-a";
    open(leg, 1000);
    assert_eq!(peek(leg), Some(BootMark::Pending));
    assert_eq!(record(leg, "清和", 2000), Record::Kept);
    assert_eq!(peek(leg), Some(BootMark::Stripped("清和".into())));
    assert_eq!(take_apply(leg), Some(Apply::Session("清和".into())));
    // 幂等：二次取用不再给
    assert_eq!(take_apply(leg), None);
    close(leg);
    assert_eq!(peek(leg), None);
}

#[test]
fn spec_bar256_裸壳哨兵_取用为bare() {
    let leg = "t-256-b";
    open(leg, 0);
    assert_eq!(
        record(leg, kfm_na::tmux_ctl::BOOTSTRAP_SHELL_SENTINEL, 10),
        Record::Kept
    );
    assert_eq!(take_apply(leg), Some(Apply::Bare));
    close(leg);
}

#[test]
fn spec_bar256_超窗未收到_missed且迁移可留痕() {
    let leg = "t-256-c";
    open(leg, 1000);
    // 窗内无事；超窗一拍 tick → Missed，迁移行带起算点与窗长（白露②：不许静默）
    let tr = tick(leg, 1000 + WINDOW_MS + 1);
    assert_eq!(tr, Some((1000, WINDOW_MS)), "迁移须报起算点与窗长");
    assert_eq!(peek(leg), Some(BootMark::Missed));
    // Missed 是终态：再 tick 不再迁移
    assert_eq!(tick(leg, 1000 + WINDOW_MS + 2), None);
    close(leg);
}

#[test]
fn spec_bar256_坏载荷_bad报红不静默() {
    let leg = "t-256-d";
    open(leg, 0);
    assert_eq!(record(leg, "含\x07控制", 10), Record::Kept); // 结构合法但载荷非法
    match peek(leg) {
        Some(BootMark::Bad(p)) => assert!(p.contains("控制"), "{p}"),
        other => panic!("坏载荷应落 Bad，实得 {other:?}"),
    }
    close(leg);
}

#[test]
fn spec_bar256_窗外与无账_拒收回原样透传() {
    // 无账腿：标记不记账（仿冒/巧合载荷只在窗内有效——楼3 失效安全）
    assert_eq!(record("t-256-noacct", "清和", 0), Record::Rejected);
    // 超窗后到标：拒收（泵把原字节透回网格，alacritty 静默忽略）
    let leg = "t-256-e";
    open(leg, 0);
    assert_eq!(record(leg, "清和", WINDOW_MS + 1), Record::Rejected);
    assert_eq!(peek(leg), Some(BootMark::Pending), "窗外到标不改账");
    close(leg);
}

// ---- C. 路由真值表（白露楼4：Pending 不落 AI 路） ----

#[test]
fn spec_bar256_路由_pending留栏不吞字() {
    match decide(Page::Terminal, None, None, Some(&BootMark::Pending), true) {
        Route::Hold(h) => assert!(h.contains("接通"), "{h}"),
        other => panic!("Pending 应留栏 Hold，实得 {other:?}"),
    }
}

#[test]
fn spec_bar256_路由_missed_bad归ai带引导账提示() {
    // BAR-252 返工（白露楼26）：引导账认不出窗＝明确失败，不改投 AI
    match decide(Page::Terminal, None, None, Some(&BootMark::Missed), true) {
        Route::Fail(h) => assert!(h.contains("引导账未回"), "{h}"),
        other => panic!("Missed 应明确失败，实得 {other:?}"),
    }
    match decide(
        Page::Terminal,
        None,
        None,
        Some(&BootMark::Bad("x".into())),
        true,
    ) {
        Route::Fail(h) => assert!(h.contains("引导账"), "{h}"),
        other => panic!("Bad 应明确失败，实得 {other:?}"),
    }
}

#[test]
fn spec_bar256_路由_stripped快路与哨兵() {
    // 剥到名 = 服务器真值，直连窗路（不必等 UI 应用账）
    assert_eq!(
        decide(
            Page::Terminal,
            None,
            None,
            Some(&BootMark::Stripped("清和".into())),
            true
        ),
        Route::Window("清和".into())
    );
    // 哨兵 = 已知裸壳：明确失败（不投 AI），原因＋出路
    match decide(
        Page::Terminal,
        None,
        None,
        Some(&BootMark::Stripped(
            kfm_na::tmux_ctl::BOOTSTRAP_SHELL_SENTINEL.into(),
        )),
        true,
    ) {
        Route::Fail(h) => assert!(h.contains("裸 shell") && h.contains("先"), "{h}"),
        other => panic!("哨兵应明确失败，实得 {other:?}"),
    }
    // 无账（非引导流）＝同样明确失败，不改投
    match decide(Page::Terminal, None, None, None, true) {
        Route::Fail(h) => assert!(h.contains("裸 shell"), "{h}"),
        other => panic!("无账应明确失败，实得 {other:?}"),
    }
}

#[test]
fn spec_bar256_路由_附着真值压账() {
    // 用户已手动附着：账是什么都不许压过真附着
    for boot in [
        None,
        Some(BootMark::Pending),
        Some(BootMark::Missed),
        Some(BootMark::Stripped("别的窗".into())),
    ] {
        assert_eq!(
            decide(Page::Terminal, None, Some("清和"), boot.as_ref(), true),
            Route::Window("清和".into()),
            "attached 在时账={boot:?} 不许改路由"
        );
    }
    // 面板开着 / AI 页：Pending 也不 Hold（人没在看窗，归 AI 无提示）
    assert_eq!(
        decide(
            Page::Terminal,
            Some(Panel::Ai),
            None,
            Some(&BootMark::Pending),
            true
        ),
        Route::Ai(None)
    );
}

// ---- D. 引导脚本打标覆盖 ----

#[test]
fn spec_bar256_引导脚本_每个exec前必有mark() {
    for last in [
        None,
        Some("清和"),
        Some(kfm_na::tmux_ctl::BOOTSTRAP_SHELL_SENTINEL),
    ] {
        let cmd = kfm_na::tmux_ctl::cmd_bootstrap(last);
        // 逐次找 "exec "，其前 64 字节内须有 kfm_mark 调用（函数定义体
        // 不含 exec，天然不计）
        let mut n_exec = 0;
        let mut n_marked = 0;
        let mut pos = 0;
        while let Some(i) = cmd[pos..].find("exec ") {
            let at = pos + i;
            n_exec += 1;
            let lo = at.saturating_sub(64);
            if cmd[lo..at].contains("kfm_mark") {
                n_marked += 1;
            }
            pos = at + 5;
        }
        assert!(n_exec >= 3, "引导裁决链至少三条 exec 臂：{cmd}");
        assert_eq!(n_exec, n_marked, "last={last:?} 有 exec 臂漏打标：{cmd}");
        // 私有键形状与哨兵打标
        assert!(cmd.contains("7777;kfmboot="), "{cmd}");
        assert!(
            cmd.contains(&format!(
                "kfm_mark '{}'",
                kfm_na::tmux_ctl::BOOTSTRAP_SHELL_SENTINEL
            )),
            "裸壳臂须打哨兵标：{cmd}"
        );
    }
}

// ---- E. 泵接线：三消费方全净 ----

#[test]
fn spec_bar256_泵内剥壳_显示记录补屏三消费方全净() {
    use kfm_na::gate::SessionPump;
    use kfm_na::session::SessionEvent;
    use std::sync::mpsc::channel;

    let leg = "t-256-pump";
    let (tx, rx) = channel();
    let mut pump = SessionPump::new();
    pump.register(leg, rx);
    open(leg, kfm_na::report::boot_ms() as u64);

    tx.send(SessionEvent::Output {
        data: format!("屏上字{}", mark("清和")),
    })
    .unwrap();
    tx.send(SessionEvent::Output {
        data: "后继字".into(),
    })
    .unwrap();

    let mut sunk = String::new();
    let mut recd = String::new();
    let fed = pump.pump(
        leg,
        &mut |b| sunk.push_str(std::str::from_utf8(b).unwrap()),
        &mut |_n, b| recd.push_str(std::str::from_utf8(b).unwrap()),
    );
    assert!(fed);
    assert_eq!(sunk, "屏上字后继字", "显示 sink 不许见标记");
    assert_eq!(recd, "屏上字后继字", "飞行记录也不许留标记（白露①）");
    assert_eq!(take_apply(leg), Some(Apply::Session("清和".into())));
    close(leg);

    // 待机槽：非活跃方 Output 进 replay——同样先剥壳
    let (tx2, rx2) = channel();
    pump.register(leg, rx2);
    open(leg, kfm_na::report::boot_ms() as u64);
    tx2.send(SessionEvent::Output {
        data: format!("待机{}", mark("清和")),
    })
    .unwrap();
    let _ = pump.pump("别的活跃", &mut |_| {}, &mut |_, _| {});
    let replay = pump.take_replay(leg);
    assert_eq!(replay, vec!["待机".to_string()], "replay 补屏料不许带标记");
    close(leg);
}
