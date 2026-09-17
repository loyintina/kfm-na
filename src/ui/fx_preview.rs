//! fx_preview.rs — 组件池跳框预览画板的**相位表单一源**（2026-09-17
//! 十五修，用户拍板「一起做吧」= 一期乒乓化 + 二期语义化全做）。
//!
//! 病史（十四修相位表的三条实锤，na-rec 实录定罪）：
//! ①2400ms 循环大半停摆（弹簧/缓动停 62%、视口平移停 59%）——「演示」
//!   读感是「坏了」；
//! ②每圈结尾硬切复位（瞬移回起点，无回程演示）；
//! ③借用预览语义失真（光标滑行/池高伸缩/下拉开合借缓动件、标签栏层/
//!   光标层挂静态图）。
//!
//! 本模块只出**纯函数相位表**（A 档钉，tests/fx_preview_spec.rs），
//! 涂装归 termview 预览段。曲线纪律（BAR-095）：预览不得自编曲线——
//! 位移 = ease_in_out_cubic；方向分档件（缓动面板/手势松手）吃 raw_t
//! 自配 power2_out/rise_release；弹簧 = spring_pos 实曲线骑行。
//!
//! 循环 1400ms 四段（首尾同位 p=0，回卷无缝）：
//! ```text
//!   0..350    去程腿（raw_t = at/350）
//!   350..550  终点停靠（p = 1）
//!   550..900  回程腿（raw_t = (at−550)/350）
//!   900..1400 起点停靠（p = 0）
//! ```
//! 白球 = 手指，两种语义：
//! - 拖球（drag）：全程跟展品（视口平移/手势仲裁）——腿内满 α220，
//!   停靠段淡出；
//! - 点球（tap）：腿首点触即走（弹簧/缓动/池高/标签/光标/下拉/切页
//!   的触发演示）——腿首 300ms 窗内淡入→按住→淡出。

use crate::ui::fx_ease::ease_in_out_cubic;

/// 预览循环周期 ms（十五修：2400 → 1400，停摆段压缩到 200/500ms）
pub const PREVIEW_CYCLE_MS: u64 = 1400;
/// 去程腿时长 ms
pub const LEG_GO_MS: u64 = 350;
/// 终点停靠尾 = 回程腿首
pub const LEG_RETURN_START: u64 = 550;
/// 回程腿时长 ms
pub const LEG_RETURN_MS: u64 = 350;

/// 预览腿（相位分档）：Go/Return 携带腿内 raw_t ∈ [0,1)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreviewLeg {
    /// 去程腿（0..350）：展品从起点走向终点
    Go(f32),
    /// 终点停靠（350..550）：展品停终点
    EndDwell,
    /// 回程腿（550..900）：展品从终点走回起点
    Return(f32),
    /// 起点停靠（900..1400）：展品停起点（回卷无缝：p 与下一圈 0 同位）
    StartDwell,
}

/// 相位分档（纯函数）：at 任意大，内部取模
pub fn preview_leg(at: u64) -> PreviewLeg {
    let at = at % PREVIEW_CYCLE_MS;
    if at < LEG_GO_MS {
        PreviewLeg::Go(at as f32 / LEG_GO_MS as f32)
    } else if at < LEG_RETURN_START {
        PreviewLeg::EndDwell
    } else if at < LEG_RETURN_START + LEG_RETURN_MS {
        PreviewLeg::Return((at - LEG_RETURN_START) as f32 / LEG_RETURN_MS as f32)
    } else {
        PreviewLeg::StartDwell
    }
}

/// 位移类进度（BAR-095 唯一尺 = ease_in_out_cubic）：0 →1 →0 乒乓，
/// 停靠段钉死 1/0。位移类展品（视口平移/标签滑行/光标滑行/池高/切页）
/// 唯一取值口
pub fn preview_pos(at: u64) -> f32 {
    match preview_leg(at) {
        PreviewLeg::Go(rt) => ease_in_out_cubic(rt),
        PreviewLeg::EndDwell => 1.0,
        PreviewLeg::Return(rt) => 1.0 - ease_in_out_cubic(rt),
        PreviewLeg::StartDwell => 0.0,
    }
}

/// 拖球 alpha（手指全程跟展品：视口平移/手势仲裁用）：
/// 0..80 淡入 / 80..350 满 220 / 350..500 淡出（终点停靠前半让位给
/// 展品停姿）；550..630 淡入 / 630..900 满 / 900..1050 淡出；其余 0
pub fn preview_drag_ball_alpha(at: u64) -> u32 {
    let at = at % PREVIEW_CYCLE_MS;
    if at < 80 {
        at as u32 * 220 / 80
    } else if at < LEG_GO_MS {
        220
    } else if at < 500 {
        220 - (at - LEG_GO_MS) as u32 * 220 / 150
    } else if at < LEG_RETURN_START {
        0
    } else if at < 630 {
        (at - LEG_RETURN_START) as u32 * 220 / 80
    } else if at < LEG_RETURN_START + LEG_RETURN_MS {
        220
    } else if at < 1050 {
        220 - (at - (LEG_RETURN_START + LEG_RETURN_MS)) as u32 * 220 / 150
    } else {
        0
    }
}

/// 点球 alpha（腿首点触即走：弹簧/缓动/池高/标签/光标/下拉/切页的
/// 触发演示用）：0..80 淡入 / 80..160 满 220 / 160..300 淡出；
/// 回程 550..630 淡入 / 630..710 满 / 710..850 淡出；其余 0
pub fn preview_tap_ball_alpha(at: u64) -> u32 {
    let at = at % PREVIEW_CYCLE_MS;
    if at < 80 {
        at as u32 * 220 / 80
    } else if at < 160 {
        220
    } else if at < 300 {
        220 - (at - 160) as u32 * 220 / 140
    } else if at < LEG_RETURN_START {
        0
    } else if at < 630 {
        (at - LEG_RETURN_START) as u32 * 220 / 80
    } else if at < 710 {
        220
    } else if at < 850 {
        220 - (at - 710) as u32 * 220 / 140
    } else {
        0
    }
}
