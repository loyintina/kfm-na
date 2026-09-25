//! ctrl_feed_spec.rs — v4 推流画布播种/续喂相位机考题（A 档考题先行）
//!
//! BAR-155（2026-09-25 真机定罪，pty 实录 fixture）：v4 首版上机推流
//! 从未生效——报表实录「播种发出→头块无 KFMHDR 头行」5s 循环，用户
//! 看到的一直是 v3 轮询保底。两病灶：
//! ①tmux -C attach-session 命令行命令自己的空回应块最先到达，被当
//!   播种头块判负，真播种块到达时相位已回稳态全丢弃；
//! ②（①修好才会暴露的连环病灶）头行认领后头块的 %end 被当 capture
//!   块的 %end → 空 capture 提前 Build，真 capture 正文被丢。
//! fixture 按 pty 实录块序逐行钉。变异抽检：摘空块跳过 → 钉①红；
//! 并相 HdrEnd → 钉②红——均须实咬。

use kfm_na::ctrl_feed::{CtrlAct, CtrlFeed};
use kfm_na::tmux_ctl::parse_ctrl_line;

/// 一行进机（与 android_app 薄壳同路：先分类再消费）
fn line(f: &mut CtrlFeed, l: &str) -> CtrlAct {
    f.on_event(parse_ctrl_line(l), l)
}

/// 多行进机收动作串
fn lines(f: &mut CtrlFeed, ls: &[&str]) -> Vec<CtrlAct> {
    ls.iter().map(|l| line(f, l)).collect()
}

#[test]
fn spec_bar155_实证流全序_空块跳过到头播落地() {
    // pty 实录（2026-09-25，tmux 3.4）：attach 空块 → Notify → 回声两行
    // → 头块（KFMHDR）→ capture 块 → %output 续喂。全序走完动作串必须
    // 逐拍对上——这条钉 = BAR-155 两病灶的合卷
    let mut f = CtrlFeed::new();
    f.seed_sent();
    assert!(f.pane().is_none(), "播种发出 ≠ 播种落地（pane 未认领）");
    let acts = lines(
        &mut f,
        &[
            "%begin 1790338633 2100007 0", // attach-session 自己的空回应块
            "%end 1790338633 2100007 0",
            "%session-changed $1 kfm-na", // Notify：无 pane 不逼播
            "display-message -p 'KFMHDR #{history_size} #{history_limit} #{cursor_x} #{cursor_y} #{pane_id}'", // pty 回声
            "capture-pane -p -e -S -",     // pty 回声
            "%begin 1790338635 2100019 1", // 播种头块
            "KFMHDR 2790 10000 5 50 %3",
            "%end 1790338635 2100019 1",   // 头块关（不许提前 Build！）
            "%begin 1790338635 2100020 1", // capture 块
            "",
            " <ESC>[38;5;111m╭─╮<ESC>[39m",
            "第三行",
            "%end 1790338635 2100020 1", // capture 收齐 → Build
        ],
    );
    // attach 空块/Notify/回声/块边界全 None
    assert!(
        acts[..12].iter().all(|a| *a == CtrlAct::None),
        "播种落地前不许有任何动作: {acts:?}"
    );
    let CtrlAct::Build(cap) = &acts[12] else {
        panic!("capture 关块必须产 Build: {acts:?}");
    };
    // capture 正文 \r\n 缝合（与 v3 capture_parse 同料），无头行无壳
    assert_eq!(cap, "\r\n <ESC>[38;5;111m╭─╮<ESC>[39m\r\n第三行");
    assert_eq!(f.pane(), Some(3), "头行 pane 必须认领入账");
    f.built();
    assert!(f.is_steady());
    // 稳态续喂：pane 匹配才喂
    assert_eq!(
        line(&mut f, r"%output %3 hello\015\012"),
        CtrlAct::Feed(b"hello\r\n".to_vec())
    );
    assert_eq!(
        line(&mut f, r"%output %99 stray\015\012"),
        CtrlAct::None,
        "非活动窗格字节不许混进画布"
    );
}

#[test]
fn spec_bar155_空块误判判负_病灶一回归() {
    // 病灶①单行版：attach 空块的 %end 不许判负——判负 = 真播种块随后
    // 全丢弃（真机实录的 5s 空转死循环就是这么来的）
    let mut f = CtrlFeed::new();
    f.seed_sent();
    line(&mut f, "%begin 1 1 0");
    assert_eq!(line(&mut f, "%end 1 1 0"), CtrlAct::None, "空块必须跳过");
    // 随后真播种块照常落地
    lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 0 10000 0 0 %7",
            "%end 2 2 1",
            "%begin 3 3 1",
            "%end 3 3 1",
        ],
    );
    assert_eq!(f.pane(), Some(7), "空块跳过后真播种必须能落地");
}

#[test]
fn spec_bar155_头块关不产_build_病灶二回归() {
    // 病灶②单行版：头行认领后头块的 %end 若产 Build = 空 capture 提前
    // 落地、真正文被丢。钉：头块 %end 只能转相，唯一 Build 在 capture 关
    let mut f = CtrlFeed::new();
    f.seed_sent();
    let acts = lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 10 10000 0 0 %7",
            "%end 2 2 1", // ← 头块关：病灶②在此产过空 Build
        ],
    );
    assert!(
        acts.iter().all(|a| *a == CtrlAct::None),
        "头块关只许转相不许产动作: {acts:?}"
    );
    // capture 空块（空窗格）→ Build 空串（合法：空画布播种）
    let acts2 = lines(&mut f, &["%begin 3 3 1", "%end 3 3 1"]);
    assert_eq!(acts2[1], CtrlAct::Build(String::new()));
}

#[test]
fn spec_播种窗口_output丢弃_building进pend() {
    // 带内对齐律：播种窗口（AwaitHeader..InCapture）的 %output 已在
    // capture 内 = 丢弃；capture 关块后（Building）的 %output 不在
    // capture 内 = Pend 待补喂；稳态 = Feed
    let mut f = CtrlFeed::new();
    f.seed_sent();
    assert_eq!(
        line(&mut f, r"%output %3 early\015\012"),
        CtrlAct::None,
        "播种窗口字节必须丢弃（已在快照内）"
    );
    lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 0 10000 0 0 %3",
            "%end 2 2 1",
            "%begin 3 3 1",
            "%end 3 3 1",
        ],
    );
    assert_eq!(
        line(&mut f, r"%output %3 late\015\012"),
        CtrlAct::Pend(b"late\r\n".to_vec()),
        "Building 期字节必须进 pend（安装补喂）"
    );
    assert_eq!(line(&mut f, r"%output %9 x\015\012"), CtrlAct::None);
    f.built();
    assert_eq!(
        line(&mut f, r"%output %3 live\015\012"),
        CtrlAct::Feed(b"live\r\n".to_vec())
    );
}

#[test]
fn spec_判负三路_error与畸形与缺块() {
    // %error：播种任一相判负归零
    let mut f = CtrlFeed::new();
    f.seed_sent();
    lines(
        &mut f,
        &["%begin 2 2 1", "KFMHDR 0 10000 0 0 %3", "%end 2 2 1"],
    );
    assert_eq!(
        line(&mut f, "%error 3 3 1"),
        CtrlAct::SeedFail("命令块 %error")
    );
    assert!(f.pane().is_none() && f.is_steady(), "判负必须归零");
    // 头块有正文但认不出头行 = 判负（与空块跳过对立——漏判=病灶①回潮）
    let mut f2 = CtrlFeed::new();
    f2.seed_sent();
    line(&mut f2, "%begin 2 2 1");
    line(&mut f2, "garbage line");
    assert_eq!(
        line(&mut f2, "%end 2 2 1"),
        CtrlAct::SeedFail("头块有正文但无 KFMHDR 头行")
    );
    // 头块后直接 %end（capture 块没来）= 判负
    let mut f3 = CtrlFeed::new();
    f3.seed_sent();
    lines(
        &mut f3,
        &["%begin 2 2 1", "KFMHDR 0 10000 0 0 %3", "%end 2 2 1"],
    );
    assert_eq!(
        line(&mut f3, "%end 9 9 1"),
        CtrlAct::SeedFail("capture 块缺失（头块后直接 %end）")
    );
}

#[test]
fn spec_notify逼对账与exit死亡() {
    let mut f = CtrlFeed::new();
    f.seed_sent();
    // 播种在途的 Notify 不逼播（attach 时 %session-changed 必到）
    assert_eq!(line(&mut f, "%session-changed $1 kfm-na"), CtrlAct::None);
    lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 0 10000 0 0 %3",
            "%end 2 2 1",
            "%begin 3 3 1",
            "%end 3 3 1",
        ],
    );
    f.built();
    // 稳态推流中 Notify = 逼对账（切窗/布局变化 %output 覆盖不到）
    assert_eq!(
        line(&mut f, "%window-pane-changed @2 %4"),
        CtrlAct::Reconcile
    );
    assert_eq!(line(&mut f, "%layout-change @2 x"), CtrlAct::Reconcile);
    // %exit = 死亡（归零 + Dead）
    assert!(matches!(line(&mut f, "%exit"), CtrlAct::Dead(_)));
    assert!(f.pane().is_none() && f.is_steady());
    // 未播种的稳态 Notify 不逼播
    assert_eq!(line(&mut f, "%window-add @3"), CtrlAct::None);
}
