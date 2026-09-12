//! viewport_push_spec.rs — 视口推移+Q 弹形变考题（2026-09-12 用户拍板：
//! 面板交互从「覆盖」改「视口平移」，被挤走的页先轻微整体形变再出场，
//! 回来时用弹簧「墩一下」。A 档纯逻辑，答案 src/ui/viewport_push.rs）。
//!
//! 契约：
//! ①推移纯函数——三面板 off（已含缝采样/拖拽旁路）唯一决定基座页位移：
//!   AI 页落下 → 基座下移屏高；配置右来 → 基座左移屏宽；文件树镜像。
//!   面板全收（off 全在屏外位）= 基座回原位（零漂移）。
//! ②被压面板额外位移 = 其上各面板推移之和（交叉轴叠加 [配置,AI]：
//!   配置被 AI 压时随 AI 推移下移；顶面板永不受推）。
//! ③形变目标：进度 0 → 不压；进度 ≥ RAMP → 压满 SQUASH_MAX；中间线性。
//! ④Q 弹积分器：目标骤降时速度惯性带过零（scale 短暂 >1 = 回弹）；
//!   目标恒定时有限步内收敛贴死（不许永动烧帧）。
//! 变异抽检：①里把 push_y 写成 -（方向反）→ 题①红；②漏加上方面板 →
//! 题②红；③clamp 删掉 → 题③红；④速度项丢 dt → 题④红。

use kfm_na::ai_presence::Panel;
use kfm_na::ui::viewport_push::{
    SQUASH_MAX, SQUASH_RAMP, covered_extra, squash_settled, squash_step, squash_target,
    viewport_push,
};

const W: u32 = 720;
const H: u32 = 1280;

#[test]
fn spec_推移_ai落下基座下移() {
    // AI 屏外（off=-h）：基座原位、进度 0
    let (p, pmax) = viewport_push(-(H as i32), W as i32, -(W as i32), W, H);
    assert_eq!((p.dx, p.dy), (0.0, 0.0), "AI 屏外 = 基座不动");
    assert_eq!(pmax, 0.0);
    // AI 落一半：基座下移半屏
    let (p, pmax) = viewport_push(-(H as i32) / 2, W as i32, -(W as i32), W, H);
    assert_eq!(p.dy, H as f32 / 2.0, "落一半 = 基座下移半屏");
    assert!((pmax - 0.5).abs() < 1e-3);
    // AI 靠泊：基座整屏下沿
    let (p, _) = viewport_push(0, W as i32, -(W as i32), W, H);
    assert_eq!(p.dy, H as f32, "靠泊 = 基座推出整屏");
}

#[test]
fn spec_推移_水平双向镜像() {
    // 配置从右来（off +w→0）：基座左移
    let (p, _) = viewport_push(-(H as i32), W as i32 / 2, -(W as i32), W, H);
    assert_eq!(p.dx, -(W as f32) / 2.0, "配置进一半 = 基座左移半屏");
    let (p, _) = viewport_push(-(H as i32), 0, -(W as i32), W, H);
    assert_eq!(p.dx, -(W as f32), "配置靠泊 = 基座推出左缘");
    // 文件树从左来（off -w→0）：基座右移（镜像）
    let (p, _) = viewport_push(-(H as i32), W as i32, -(W as i32) / 2, W, H);
    assert_eq!(p.dx, W as f32 / 2.0, "文件树进一半 = 基座右移半屏");
    let (p, pmax) = viewport_push(-(H as i32), W as i32, 0, W, H);
    assert_eq!(p.dx, W as f32, "文件树靠泊 = 基座推出右缘");
    assert_eq!(pmax, 1.0);
}

#[test]
fn spec_被压面板_只吃上方面板的推移() {
    let offs = (-(H as i32), W as i32, -(W as i32)); // 全屏外
    // 交叉轴 [Config, Ai]：AI 落一半，配置（在下）随之下移半屏
    let stack = [Panel::Config, Panel::Ai];
    let e = covered_extra(
        &stack,
        Panel::Config,
        -(H as i32) / 2,
        W as i32,
        -(W as i32),
        W,
        H,
    );
    assert_eq!(e.dy, H as f32 / 2.0, "被 AI 压的配置随 AI 下移");
    // 顶面板（Ai）永不受推
    let e = covered_extra(
        &stack,
        Panel::Ai,
        -(H as i32) / 2,
        W as i32,
        -(W as i32),
        W,
        H,
    );
    assert_eq!((e.dx, e.dy), (0.0, 0.0), "顶面板零额外位移");
    // 同轴 [FileTree, Config]：配置进一半，文件树（在下）被左推半屏
    let stack = [Panel::FileTree, Panel::Config];
    let e = covered_extra(&stack, Panel::FileTree, offs.0, W as i32 / 2, offs.2, W, H);
    assert_eq!(e.dx, -(W as f32) / 2.0, "被配置压的文件树随配置左移");
    // 不在栈 = 零额外（屏外位是它的家，无推可言）
    let e = covered_extra(&stack, Panel::Ai, offs.0, offs.1, offs.2, W, H);
    assert_eq!((e.dx, e.dy), (0.0, 0.0), "不在栈零额外");
    let _ = offs;
}

#[test]
fn spec_形变目标_斜坡与饱和() {
    assert_eq!(squash_target(0.0), 0.0, "没动不压");
    assert_eq!(squash_target(SQUASH_RAMP), SQUASH_MAX, "斜坡顶压满");
    assert_eq!(squash_target(1.0), SQUASH_MAX, "过斜坡恒压满（clamp）");
    let half = squash_target(SQUASH_RAMP / 2.0);
    assert!((half - SQUASH_MAX / 2.0).abs() < 1e-6, "斜坡中点线性");
}

#[test]
fn spec_q弹_回弹过冲与收敛() {
    // 场景：压满（s=SQUASH_MAX）后目标骤降 0（面板收尾部）——
    // 欠阻尼惯性必须带 s 冲过 0（scale = 1-s 短暂 >1 = 「墩一下」）
    let mut s = SQUASH_MAX;
    let mut v = 0.0;
    let mut overshot = false;
    let mut steps = 0;
    loop {
        let (ns, nv) = squash_step(s, v, 0.0, 8); // 120Hz 帧步
        s = ns;
        v = nv;
        steps += 1;
        if s < -0.002 {
            overshot = true; // 冲过零 ≥0.2% 屏尺 = 可感回弹
        }
        if squash_settled(s, v, 0.0) {
            break;
        }
        assert!(steps < 500, "4 秒内必须收敛（500×8ms），不许永动");
    }
    assert!(overshot, "目标骤降必须过冲（Q 弹的「墩」），无过冲 = 硬");
    assert!(s.abs() < 0.001, "收敛后贴死目标");
    // 反向：从 0 追压满目标 = 平滑压缩不过冲过量（入场不「鼓包」）
    let mut s = 0.0;
    let mut v = 0.0;
    let mut peak = 0.0_f32;
    for _ in 0..500 {
        let (ns, nv) = squash_step(s, v, SQUASH_MAX, 8);
        s = ns;
        v = nv;
        peak = peak.max(s);
        if squash_settled(s, v, SQUASH_MAX) {
            break;
        }
    }
    assert!(
        peak <= SQUASH_MAX * 1.35,
        "入场过冲不许超 35%（实际 {peak:.4}）——压缩是配角不是主角"
    );
}
