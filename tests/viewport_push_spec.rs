//! viewport_push_spec.rs — 视口推移考题（2026-09-12 用户拍板：面板交互
//! 从「覆盖」改「视口平移」。A 档纯逻辑，答案 src/ui/viewport_push.rs）。
//! Q 弹形变同日二审取消（实拍不合预期）——squash 机械与考题③④一并
//! 退役，git 历史可查。同日下午四公民（三缘语义）：解析页入列——右缘家
//! 与配置同约定（off ∈ [0,+w]，屏外右→靠泊），推移镜像配置。
//!
//! 契约：
//! ①推移纯函数——四面板 off（已含缝采样/拖拽旁路）唯一决定基座页位移：
//!   AI 页落下 → 基座下移屏高；右缘家（配置/解析）进 → 基座左移屏宽；
//!   文件树镜像右移。面板全收（off 全在屏外位）= 基座回原位（零漂移）。
//! ②被压面板额外位移 = 其上各面板推移之和（交叉轴叠加 [配置,AI]：
//!   配置被 AI 压时随 AI 推移下移；顶面板永不受推）。
//! 变异抽检：①里把 push_y 写成 -（方向反）→ 题①红；解析家镜像符号反
//! （p_pt 写成 +）→ 题①解析臂红；②漏加上方面板 → 题②红。

use kfm_na::ai_presence::Panel;
use kfm_na::ui::viewport_push::{covered_extra, viewport_push};

const W: u32 = 720;
const H: u32 = 1280;

#[test]
fn spec_推移_ai落下基座下移() {
    // AI 屏外（off=-h）：基座原位、进度 0（其余三家全在屏外位）
    let (p, pmax) = viewport_push(-(H as i32), W as i32, -(W as i32), W as i32, W, H);
    assert_eq!((p.dx, p.dy), (0.0, 0.0), "AI 屏外 = 基座不动");
    assert_eq!(pmax, 0.0);
    // AI 落一半：基座下移半屏
    let (p, pmax) = viewport_push(-(H as i32) / 2, W as i32, -(W as i32), W as i32, W, H);
    assert_eq!(p.dy, H as f32 / 2.0, "落一半 = 基座下移半屏");
    assert!((pmax - 0.5).abs() < 1e-3);
    // AI 靠泊：基座整屏下沿
    let (p, _) = viewport_push(0, W as i32, -(W as i32), W as i32, W, H);
    assert_eq!(p.dy, H as f32, "靠泊 = 基座推出整屏");
}

#[test]
fn spec_推移_水平双向镜像() {
    // 配置从右来（off +w→0）：基座左移
    let (p, _) = viewport_push(-(H as i32), W as i32 / 2, -(W as i32), W as i32, W, H);
    assert_eq!(p.dx, -(W as f32) / 2.0, "配置进一半 = 基座左移半屏");
    let (p, _) = viewport_push(-(H as i32), 0, -(W as i32), W as i32, W, H);
    assert_eq!(p.dx, -(W as f32), "配置靠泊 = 基座推出左缘");
    // 解析页从右来（右缘家与配置同约定）：基座左移（镜像同款）
    let (p, _) = viewport_push(-(H as i32), W as i32, -(W as i32), W as i32 / 2, W, H);
    assert_eq!(p.dx, -(W as f32) / 2.0, "解析页进一半 = 基座左移半屏");
    let (p, pmax) = viewport_push(-(H as i32), W as i32, -(W as i32), 0, W, H);
    assert_eq!(p.dx, -(W as f32), "解析页靠泊 = 基座推出左缘");
    assert_eq!(pmax, 1.0);
    // 文件树从左来（off -w→0）：基座右移（镜像）
    let (p, _) = viewport_push(-(H as i32), W as i32, -(W as i32) / 2, W as i32, W, H);
    assert_eq!(p.dx, W as f32 / 2.0, "文件树进一半 = 基座右移半屏");
    let (p, pmax) = viewport_push(-(H as i32), W as i32, 0, W as i32, W, H);
    assert_eq!(p.dx, W as f32, "文件树靠泊 = 基座推出右缘");
    assert_eq!(pmax, 1.0);
}

#[test]
fn spec_被压面板_只吃上方面板的推移() {
    let offs = (-(H as i32), W as i32, -(W as i32), W as i32); // 全屏外
    // 交叉轴 [Config, Ai]：AI 落一半，配置（在下）随之下移半屏
    let stack = [Panel::Config, Panel::Ai];
    let e = covered_extra(
        &stack,
        Panel::Config,
        -(H as i32) / 2,
        W as i32,
        -(W as i32),
        W as i32,
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
        W as i32,
        W,
        H,
    );
    assert_eq!((e.dx, e.dy), (0.0, 0.0), "顶面板零额外位移");
    // 同轴 [FileTree, Config]：配置进一半，文件树（在下）被左推半屏
    let stack = [Panel::FileTree, Panel::Config];
    let e = covered_extra(
        &stack,
        Panel::FileTree,
        offs.0,
        W as i32 / 2,
        offs.2,
        offs.3,
        W,
        H,
    );
    assert_eq!(e.dx, -(W as f32) / 2.0, "被配置压的文件树随配置左移");
    // 同轴 [FileTree, Parser]（解析页右缘家镜像）：解析进一半，文件树
    // （在下）被左推半屏——变异：Parser 臂漏/符号反，本条红
    let stack = [Panel::FileTree, Panel::Parser];
    let e = covered_extra(
        &stack,
        Panel::FileTree,
        offs.0,
        offs.1,
        offs.2,
        W as i32 / 2,
        W,
        H,
    );
    assert_eq!(e.dx, -(W as f32) / 2.0, "被解析页压的文件树随其左移");
    // 不在栈 = 零额外（屏外位是它的家，无推可言）
    let e = covered_extra(&stack, Panel::Ai, offs.0, offs.1, offs.2, offs.3, W, H);
    assert_eq!((e.dx, e.dy), (0.0, 0.0), "不在栈零额外");
    let _ = offs;
}
