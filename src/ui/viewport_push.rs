//! ui/viewport_push.rs — 视口推移（2026-09-12 用户拍板：面板交互从
//! 「覆盖」改「视口平移」——原内容平移走、新页面移过来）。
//!
//! 推移是纯合成期派生：三面板 off（缝采样/拖拽旁路之后的渲染值）唯一
//! 决定各层 placement，**状态核目标值体系零改动**——§五B「被覆盖者
//! placement 冻结」由此改写为「被压者随动」，遮盖撤走从「零动画露出」
//! 变成「随推移滑回」。手势跟手的底页随动零新机制：拖拽旁路改写的就
//! 是 off，推移自然跟手。
//!
//! Q 弹形变同日二审取消（用户实拍「跟想的不一样」）：squash 弹簧机械
//! 全部退役（git 历史可查），实例仿射管线的 scale 维保留（恒 1.0 恒等
//! 早退——变换函数的正当通用签名，不是死码）。
//!
//! 分层：纯逻辑（A 档考题 tests/viewport_push_spec.rs）；应用点在
//! android_app::draw_frame_gles（GLES 路径）。softbuffer 兜底路径
//! 保留旧覆盖语义（立项书红线：兜底不再投入——state.md 欠账条）。

use crate::ai_presence::Panel;

/// 一页的视口推移量（px，合成期 placement 加项）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Push {
    pub dx: f32,
    pub dy: f32,
}

/// 三面板 off → 基座页推移 + 最大进度（纯函数）。
/// off 语义：AI ∈ [-h, 0]（屏外顶→靠泊）、配置 ∈ [0, +w]（靠泊→屏外右）、
/// 文件树 ∈ [-w, 0]（屏外左→靠泊）。推移方向：AI 落 → 基座下移；
/// 配置进 → 基座左移；文件树进 → 基座右移。
pub fn viewport_push(panel_off: i32, cfg_off: i32, ft_off: i32, w: u32, h: u32) -> (Push, f32) {
    let (w, h) = (w as f32, h as f32);
    let p_ai = ((h + panel_off as f32) / h).clamp(0.0, 1.0);
    let p_cfg = ((w - cfg_off as f32) / w).clamp(0.0, 1.0);
    let p_ft = ((w + ft_off as f32) / w).clamp(0.0, 1.0);
    (
        Push {
            dx: p_ft * w - p_cfg * w,
            dy: p_ai * h,
        },
        p_ai.max(p_cfg).max(p_ft),
    )
}

/// 单面板 off → 自身推进度的推移量（纯函数，与 viewport_push 同源）
fn panel_progress(panel: Panel, panel_off: i32, cfg_off: i32, ft_off: i32, w: f32, h: f32) -> Push {
    match panel {
        Panel::Ai => Push {
            dx: 0.0,
            dy: ((h + panel_off as f32) / h).clamp(0.0, 1.0) * h,
        },
        Panel::Config => Push {
            dx: -((w - cfg_off as f32) / w).clamp(0.0, 1.0) * w,
            dy: 0.0,
        },
        Panel::FileTree => Push {
            dx: ((w + ft_off as f32) / w).clamp(0.0, 1.0) * w,
            dy: 0.0,
        },
    }
}

/// 被压面板（栈内非顶）的额外位移 = 其上各面板推移之和（纯函数）。
/// 顶面板/不在栈 = 零额外。栈底→顶序传入
pub fn covered_extra(
    stack: &[Panel],
    panel: Panel,
    panel_off: i32,
    cfg_off: i32,
    ft_off: i32,
    w: u32,
    h: u32,
) -> Push {
    let (w, h) = (w as f32, h as f32);
    let Some(pos) = stack.iter().position(|p| *p == panel) else {
        return Push { dx: 0.0, dy: 0.0 };
    };
    let mut acc = Push { dx: 0.0, dy: 0.0 };
    for above in &stack[pos + 1..] {
        let p = panel_progress(*above, panel_off, cfg_off, ft_off, w, h);
        acc.dx += p.dx;
        acc.dy += p.dy;
    }
    acc
}
