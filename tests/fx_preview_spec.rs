//! fx_preview 相位表考题（A 档：纯函数，考题先行）——十五修乒乓相位表
//! （循环 1400ms：0..350 去程 / 350..550 终点停靠 / 550..900 回程 /
//! 900..1400 起点停靠）的机械钉。相位表是预览动画的唯一依据
//! （src/ui/fx_preview.rs 模块头），窗口挪一格即红。

use kfm_na::ui::fx_preview::{
    PREVIEW_CYCLE_MS, PreviewLeg, preview_drag_ball_alpha, preview_leg, preview_pos,
    preview_tap_ball_alpha,
};

#[test]
fn spec_fx_preview_相位分档边界钉() {
    // 腿边界（变异抽检口：任一边界挪 1ms 即红）
    assert_eq!(preview_leg(0), PreviewLeg::Go(0.0));
    match preview_leg(349) {
        PreviewLeg::Go(rt) => assert!((rt - 349.0 / 350.0).abs() < 1e-6),
        other => panic!("349 必须在去程腿（实采 {other:?}）"),
    }
    assert_eq!(preview_leg(350), PreviewLeg::EndDwell);
    assert_eq!(preview_leg(549), PreviewLeg::EndDwell);
    assert_eq!(preview_leg(550), PreviewLeg::Return(0.0));
    match preview_leg(899) {
        PreviewLeg::Return(rt) => assert!((rt - 349.0 / 350.0).abs() < 1e-6),
        other => panic!("899 必须在回程腿（实采 {other:?}）"),
    }
    assert_eq!(preview_leg(900), PreviewLeg::StartDwell);
    assert_eq!(preview_leg(1399), PreviewLeg::StartDwell);
    // 回卷：1400 ≡ 0
    assert_eq!(preview_leg(PREVIEW_CYCLE_MS), PreviewLeg::Go(0.0));
    assert_eq!(preview_leg(PREVIEW_CYCLE_MS + 350), PreviewLeg::EndDwell);
}

#[test]
fn spec_fx_preview_位移乒乓钉() {
    // 端点钉死
    assert_eq!(preview_pos(0), 0.0, "起点必须 p=0");
    for at in 350..=549 {
        assert_eq!(preview_pos(at), 1.0, "终点停靠必须钉死 p=1（at={at}）");
    }
    for at in 900..1400 {
        assert_eq!(preview_pos(at), 0.0, "起点停靠必须钉死 p=0（at={at}）");
    }
    // 去程单调不减（ease-in-out 单调）
    let mut prev = 0.0f32;
    for at in (0..350).step_by(7) {
        let p = preview_pos(at);
        assert!(p >= prev, "去程必须单调（at={at}：{p} < {prev}）");
        prev = p;
    }
    // 回程单调不增
    let mut prev = 1.0f32;
    for at in (550..900).step_by(7) {
        let p = preview_pos(at);
        assert!(p <= prev, "回程必须单调（at={at}：{p} > {prev}）");
        prev = p;
    }
    // 腿-停靠接缝连续（回卷无缝条款：首尾同位）
    assert!(
        (preview_pos(349) - 1.0).abs() < 0.001,
        "去程尾必须贴 1（实采 {}）",
        preview_pos(349)
    );
    assert!(
        preview_pos(899).abs() < 0.001,
        "回程尾必须贴 0（实采 {}）",
        preview_pos(899)
    );
    assert_eq!(preview_pos(1399), preview_pos(0), "回卷必须同位");
    // 曲线身份钉（BAR-095：位移 = ease_in_out_cubic，变异成线性即红：
    // rt=88/350≈0.251 线性得 0.251，ease_in_out 得 ≈0.064）
    assert!(
        preview_pos(88) < 0.10,
        "位移曲线必须是 ease_in_out_cubic（实采 {}）",
        preview_pos(88)
    );
    // 整周期平移不变
    for at in [0, 88, 349, 400, 550, 700, 899, 1000] {
        assert_eq!(preview_pos(at), preview_pos(at + PREVIEW_CYCLE_MS));
        assert_eq!(preview_pos(at), preview_pos(at + 7 * PREVIEW_CYCLE_MS));
    }
}

#[test]
fn spec_fx_preview_拖球窗钉() {
    // 拖球 = 全程跟展品：腿内满 α220，停靠段淡出
    let cases: [(u64, u32); 15] = [
        (0, 0),
        (79, 79 * 220 / 80),
        (80, 220),
        (349, 220),
        (350, 220), // 淡出起点值 = 满
        (400, 220 - 50 * 220 / 150),
        (499, 220 - 149 * 220 / 150),
        (500, 0),
        (549, 0),
        (550, 0),
        (630, 220),
        (899, 220),
        (900, 220), // 回程淡出起点 = 满
        (975, 220 - 75 * 220 / 150),
        (1050, 0),
    ];
    for (at, want) in cases {
        assert_eq!(
            preview_drag_ball_alpha(at),
            want,
            "拖球窗 at={at} 必须 = {want}"
        );
    }
    assert_eq!(preview_drag_ball_alpha(1399), 0);
    assert_eq!(preview_drag_ball_alpha(1200), 0, "起点停靠球必须退场");
}

#[test]
fn spec_fx_preview_点球窗钉() {
    // 点球 = 腿首点触即走：腿首 300ms 窗内淡入→按住→淡出
    let cases: [(u64, u32); 13] = [
        (0, 0),
        (80, 220),
        (159, 220),
        (160, 220),
        (230, 220 - 70 * 220 / 140),
        (299, 220 - 139 * 220 / 140),
        (300, 0),
        (549, 0),
        (550, 0),
        (630, 220),
        (709, 220),
        (710, 220),
        (850, 0),
    ];
    for (at, want) in cases {
        assert_eq!(
            preview_tap_ball_alpha(at),
            want,
            "点球窗 at={at} 必须 = {want}"
        );
    }
    assert_eq!(preview_tap_ball_alpha(1399), 0);
    // 终点停靠全程无球（点触语义：停靠时手指早离开）
    for at in 350..550 {
        assert_eq!(preview_tap_ball_alpha(at), 0, "终点停靠不许有球（at={at}）");
    }
}
