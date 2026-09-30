//! ctrl_feed_spec.rs — v4 推流画布播种/续喂相位机考题（A 档考题先行）
//!
//! BAR-155（2026-09-25 真机定罪，pty/ws 双路实录 fixture）：v4 首版
//! 上机推流从未生效——报表实录播种 5s 空转循环，用户看到的一直是
//! v3 轮询保底（用户实报「还是掉帧，只是把糊弄机制逼近」）。三病灶：
//! ①tmux -C attach-session 命令行命令自己的空回应块最先到达，被当
//!   播种头块判负，真播种块到达时相位已回稳态全丢弃；
//! ②头行认领后头块的 %end 被当 capture 块的 %end → 空 capture 提前
//!   Build，真 capture 正文被丢（①修好才会暴露的连环雷）；
//! ③剥 \r 留在 cfg(android) 壳层且只剥分类不剥原文——pty 流行尾恒
//!   带 \r，KFMHDR 的 pane 段 "3\r" 数字解析失败，100% 判负（真机
//!   实录「首行=KFMHDR 2898 10000 5 50 %3」完全合法却判负；服务器
//!   侧复刻正常是因为只肉眼看了流、没让字节走真代码路径）。
//! 根因共通：协议处理散在 host 不可测的壳里。**fixture 必须用真流
//! 形态——\r\n 行尾 + 跨包截半**，字节走 feed_bytes 唯一入口。
//! BAR-211（2026-09-30 定罪，判负 4876 次）：保通道重播（invalidate/
//! 判负 reset 不排空 tmux 通道）时旧轮播种对排队/在途存活，相位机按
//! 位置认领 → 旧 capture 块落进新一轮 AwaitHeader 判负 → 退避再播 →
//! 新头块又在退避窗口被丢 = 死循环。修法：播种带 token，认 token 不
//! 认到达序——陈旧对整块静默跳过，一轮内连跳 4 块无当前 token 才判负。
//! 变异抽检：摘空块跳过 → 钉一红；HdrEnd 产 Build → 钉二红；
//! 摘 \r 剥离 → 全卷红；摘 token 校验 → bar211 错位钉红；跳块仍计负
//! → bar211 三钉红；seed_sent 不记 token → 全卷红——均须实咬。

use kfm_na::ctrl_feed::{CtrlAct, CtrlFeed, STALE_SKIP_MAX};

/// 一行真流形态（pty 流恒带 \r\n）进机，取唯一动作（多数行 None）
fn line(f: &mut CtrlFeed, l: &str) -> CtrlAct {
    let acts = f.feed_bytes(format!("{l}\r\n").as_bytes());
    assert!(acts.len() <= 1, "单行不该产多动作: {acts:?}");
    acts.into_iter().next().unwrap_or(CtrlAct::None)
}

/// 多行进机收动作串（逐行 \r\n）
fn lines(f: &mut CtrlFeed, ls: &[&str]) -> Vec<CtrlAct> {
    ls.iter().map(|l| line(f, l)).collect()
}

#[test]
fn spec_bar155_实证流全序_空块跳过到头播落地() {
    // pty/ws 实录（2026-09-25，tmux 3.4）：attach 空块 → Notify →
    // 回声两行 → 头块（KFMHDR）→ capture 块 → %output 续喂
    let mut f = CtrlFeed::new();
    f.seed_sent(7);
    assert!(f.pane().is_none(), "播种发出 ≠ 播种落地（pane 未认领）");
    let acts = lines(
        &mut f,
        &[
            "%begin 1790338633 2100007 0", // attach-session 自己的空回应块
            "%end 1790338633 2100007 0",
            "%session-changed $1 kfm-na", // Notify：无 pane 不逼播
            "display-message -p 'KFMHDR 7 #{history_size} #{history_limit} #{cursor_x} #{cursor_y} #{pane_id}'", // pty 回声
            "capture-pane -p -e -S -",     // pty 回声
            "%begin 1790338635 2100019 1", // 播种头块
            "KFMHDR 7 2790 10000 5 50 %3",
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
    let CtrlAct::Build { cap, x, y } = &acts[12] else {
        panic!("capture 关块必须产 Build: {acts:?}");
    };
    // capture 正文 \r\n 缝合（与 v3 capture_parse 同料），无头行无壳
    assert_eq!(cap, "\r\n <ESC>[38;5;111m╭─╮<ESC>[39m\r\n第三行");
    // BAR-156：Build 必须携头行游标（Canvas 归位凭据，动态行不复制）
    assert_eq!((*x, *y), (5, 50), "Build 必须携头行 cursor_x/cursor_y");
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
fn spec_bar155_cr尾必剥_病灶三回归() {
    // 病灶③单行定罪钉：pty 流 KFMHDR 行尾恒带 \r——剥不干净则 pane
    // 段 "3\r" 数字解析失败、100% 判负（真机实录「首行=KFMHDR 2898
    // 10000 5 50 %3 完全合法却判负」的机理）。本钉的行全走 feed_bytes
    // 真流形态（\r\n），认领成功 = 剥离在位
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    line(&mut f, "%begin 2 2 1");
    line(&mut f, "KFMHDR 1 2898 10000 5 50 %3"); // line() 自带 \r\n 收尾
    assert_eq!(f.pane(), Some(3), "带 \\r 尾的头行必须认领（剥 \\r 在位）");
}

#[test]
fn spec_bar155_行跨包截半装配() {
    // %output 事件可在任意字节边界截半（ws 实录：「%begin …1」与
    // 「<CR>」分属两个 Output 包）——行装配必须留住余量拼回
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    // 头行拆三段喂（截半未拼齐 = 零动作空 vec，不许臆造 None）
    assert_eq!(f.feed_bytes(b"%begin 2 2"), Vec::<CtrlAct>::new());
    assert_eq!(
        f.feed_bytes(b" 1\r\nKFMHDR 1 2898 100"),
        vec![CtrlAct::None]
    );
    let acts = f.feed_bytes(b"00 5 50 %3\r\n");
    assert_eq!(acts, vec![CtrlAct::None], "拼回头行认领不产动作");
    assert_eq!(f.pane(), Some(3), "跨包截半的头行拼回后必须认领");
}

#[test]
fn spec_bar155_空块误判判负_病灶一回归() {
    // 病灶①单行版：attach 空块的 %end 不许判负——判负 = 真播种块随后
    // 全丢弃（真机实录的 5s 空转死循环就是这么来的）
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    line(&mut f, "%begin 1 1 0");
    assert_eq!(line(&mut f, "%end 1 1 0"), CtrlAct::None, "空块必须跳过");
    // 随后真播种块照常落地
    lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 1 0 10000 0 0 %7",
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
    f.seed_sent(1);
    let acts = lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 1 10 10000 0 0 %7",
            "%end 2 2 1", // ← 头块关：病灶②在此产过空 Build
        ],
    );
    assert!(
        acts.iter().all(|a| *a == CtrlAct::None),
        "头块关只许转相不许产动作: {acts:?}"
    );
    // capture 空块（空窗格）→ Build 空串（合法：空画布播种；头行游标随行）
    let acts2 = lines(&mut f, &["%begin 3 3 1", "%end 3 3 1"]);
    assert_eq!(
        acts2[1],
        CtrlAct::Build {
            cap: String::new(),
            x: 0,
            y: 0
        }
    );
}

#[test]
fn spec_播种窗口_output丢弃_building进pend() {
    // 带内对齐律：播种窗口（AwaitHeader..InCapture）的 %output 已在
    // capture 内 = 丢弃；capture 关块后（Building）的 %output 不在
    // capture 内 = Pend 待补喂；稳态 = Feed
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    assert_eq!(
        line(&mut f, r"%output %3 early\015\012"),
        CtrlAct::None,
        "播种窗口字节必须丢弃（已在快照内）"
    );
    lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 1 0 10000 0 0 %3",
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
fn spec_判负三路_error与陈旧与缺块() {
    // %error：播种任一相判负归零
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    lines(
        &mut f,
        &["%begin 2 2 1", "KFMHDR 1 0 10000 0 0 %3", "%end 2 2 1"],
    );
    assert_eq!(
        line(&mut f, "%error 3 3 1"),
        CtrlAct::SeedFail("命令块 %error".to_string())
    );
    assert!(f.pane().is_none() && f.is_steady(), "判负必须归零");
    // BAR-211：单块「有正文但无当前 token 头」不再即判负（与旧对残骸
    // 同形无从分辨）——静默跳过保持 AwaitHeader，当前 token 的头块
    // 随后照常认领（连跳上限判负走 spec_bar211_连跳上限钉）
    let mut f2 = CtrlFeed::new();
    f2.seed_sent(1);
    line(&mut f2, "%begin 2 2 1");
    line(&mut f2, "garbage line");
    assert_eq!(
        line(&mut f2, "%end 2 2 1"),
        CtrlAct::None,
        "单块无名/陈旧块必须静默跳过不判负（BAR-211）"
    );
    assert!(!f2.is_steady(), "跳过必须保持 AwaitHeader 继续等下一块");
    lines(
        &mut f2,
        &["%begin 3 3 1", "KFMHDR 1 0 10000 0 0 %7", "%end 3 3 1"],
    );
    assert_eq!(f2.pane(), Some(7), "跳过后当前 token 头块必须照常认领");
    // 头块后直接 %end（capture 块没来）= 判负
    let mut f3 = CtrlFeed::new();
    f3.seed_sent(1);
    lines(
        &mut f3,
        &["%begin 2 2 1", "KFMHDR 1 0 10000 0 0 %3", "%end 2 2 1"],
    );
    assert_eq!(
        line(&mut f3, "%end 9 9 1"),
        CtrlAct::SeedFail("capture 块缺失（头块后直接 %end）".to_string())
    );
}

#[test]
fn spec_notify逼对账与exit死亡() {
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    // 播种在途的 Notify 不逼播（attach 时 %session-changed 必到）
    assert_eq!(line(&mut f, "%session-changed $1 kfm-na"), CtrlAct::None);
    lines(
        &mut f,
        &[
            "%begin 2 2 1",
            "KFMHDR 1 0 10000 0 0 %3",
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

// ---- BAR-211：播种 token 认领（认 token 不认到达序）----

#[test]
fn spec_bar211_陈旧对错位_旧对零动作新对落地() {
    // BAR-211 主病灶 fixture：保通道重播（invalidate/判负 reset 不排空
    // tmux 通道）时，旧轮播种对（token 1）排队/在途存活，与新对错位
    // 交织到达。旧律按位置认领 → 旧 capture 块落进新一轮 AwaitHeader
    // 判负「头块有正文但无 KFMHDR 头行」→ 退避再播 → 新头块又在退避
    // 窗口被丢 = 死循环（真机实录判负 4876 次，像素级滚动瘫痪）。
    // 新律：认 token 不认序——旧对零动作零判负，新对正常 Build
    let mut f = CtrlFeed::new();
    f.seed_sent(2); // 本轮 token=2（上轮 token=1 的播种对仍在途）
    let acts = lines(
        &mut f,
        &[
            "%begin 100 1 1", // 旧对头块（token 1 ≠ 本轮 2）
            "KFMHDR 1 2790 10000 5 50 %3",
            "%end 100 1 1",
            "%begin 101 2 1", // 旧对 capture 块（本就无头行）
            "旧轮第一行",
            "旧轮第二行",
            "%end 101 2 1",
            "%begin 102 3 1", // 新对头块（token 2 = 本轮）
            "KFMHDR 2 2800 10000 6 51 %4",
            "%end 102 3 1",
            "%begin 103 4 1", // 新对 capture 块
            "新轮正文",
            "%end 103 4 1",
        ],
    );
    assert!(
        acts[..12].iter().all(|a| *a == CtrlAct::None),
        "旧对两块与新对头块必须全程零动作（静默跳过）: {acts:?}"
    );
    let CtrlAct::Build { cap, x, y } = &acts[12] else {
        panic!("新对 capture 关块必须产 Build: {acts:?}");
    };
    assert_eq!(cap, "新轮正文");
    assert_eq!((*x, *y), (6, 51), "Build 必须携对2头行游标");
    assert_eq!(f.pane(), Some(4), "认领的必须是对2的 pane，不是对1");
}

#[test]
fn spec_bar211_invalidate在途_只对本轮落地() {
    // BAR-211 F1×F2 合流 fixture：对1 半残到达（头块已认领进 AwaitBody，
    // capture 块还在 tmux 侧排队）时 invalidate（换尺 reset 不拆通道）
    // → 对2 发出；对1 残余随后到达必须静默跳过，只有对2 落地
    let mut f = CtrlFeed::new();
    f.seed_sent(1);
    lines(
        &mut f,
        &["%begin 1 1 1", "KFMHDR 1 100 10000 0 0 %3", "%end 1 1 1"],
    );
    // invalidate 路径：reset 不排空通道 + 新一轮播种（token 递增）
    f.reset();
    f.seed_sent(2);
    let acts = lines(
        &mut f,
        &[
            "%begin 2 2 1", // 对1 残余 capture 块（在 AwaitHeader 到达）
            "对1capture正文",
            "%end 2 2 1",
            "%begin 3 3 1", // 对2 头块
            "KFMHDR 2 200 10000 1 2 %3",
            "%end 3 3 1",
            "%begin 4 4 1", // 对2 capture 块
            "对2正文",
            "%end 4 4 1",
        ],
    );
    assert!(
        acts[..8].iter().all(|a| *a == CtrlAct::None),
        "对1残余与对2头块必须全程零动作零判负: {acts:?}"
    );
    assert_eq!(
        acts[8],
        CtrlAct::Build {
            cap: "对2正文".to_string(),
            x: 1,
            y: 2
        },
        "只有对2许落地"
    );
    assert_eq!(f.pane(), Some(3));
}

#[test]
fn spec_bar211_连跳上限_四块无当前token判负() {
    // 陈旧/无名块单跳不判负（与旧对残骸同形，单块无从分辨）；一轮内
    // 连跳 STALE_SKIP_MAX 块 = 真失败（残骸不可能这么多——旧死循环每轮
    // 最多留一对两块）——归零 + SeedFail 新文案（与旧「头块有正文但无
    // KFMHDR 头行」判负分列：判卷红线是旧计数不新增）
    let mut f = CtrlFeed::new();
    f.seed_sent(9);
    for i in 1..STALE_SKIP_MAX {
        let acts = lines(&mut f, &["%begin 1 1 1", "garbage", "%end 1 1 1"]);
        assert!(
            acts.iter().all(|a| *a == CtrlAct::None),
            "第 {i} 块陈旧/无名块必须静默跳过: {acts:?}"
        );
        assert!(!f.is_steady(), "跳过必须保持 AwaitHeader 继续等下一块");
    }
    line(&mut f, "%begin 1 1 1");
    line(&mut f, "garbage-4");
    let CtrlAct::SeedFail(why) = line(&mut f, "%end 1 1 1") else {
        panic!("连跳 {STALE_SKIP_MAX} 块必须判负");
    };
    assert!(
        why.contains("连跳 4 块无当前 token") && why.contains("garbage-4"),
        "判负必须带新文案与肇事首行存证: {why}"
    );
    assert!(
        !why.contains("无 KFMHDR 头行"),
        "新旧判负文案必须分列（旧计数不新增是判卷红线）: {why}"
    );
    assert!(f.pane().is_none() && f.is_steady(), "判负必须归零");
    // 判负归零后新一轮播种照常可落地（退避再播不再被旧残骸拖死）
    f.seed_sent(10);
    lines(
        &mut f,
        &["%begin 5 5 1", "KFMHDR 10 0 10000 0 0 %8", "%end 5 5 1"],
    );
    assert_eq!(f.pane(), Some(8), "判负归零后新一轮必须能认领");
}
