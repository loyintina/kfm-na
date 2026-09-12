//! ui/viewport_push.rs — 视口推移 + Q 弹形变（2026-09-12 用户拍板：面板
//! 交互从「覆盖」改「视口平移」——原内容平移走、新页面移过来；被挤走
//! 的页先轻微整体形变再出场，回来弹簧「墩一下」）。
//!
//! 推移是纯合成期派生：三面板 off（缝采样/拖拽旁路之后的渲染值）唯一
//! 决定各层 placement，**状态核目标值体系零改动**——§五B「被覆盖者
//! placement 冻结」由此改写为「被压者随动」，遮盖撤走从「零动画露出」
//! 变成「随推移滑回」。手势跟手的底页随动零新机制：拖拽旁路改写的就
//! 是 off，推移自然跟手。
//!
//! Q 弹 = 欠阻尼弹簧积分器驱动压缩量 s（scale = 1 - s）：目标骤降
//! （收尾部 p 跌出斜坡区）时速度惯性带 s 冲过零 → scale 短暂 >1 =
//! 回弹。积分器是帧态（壳每帧喂 now_ms），活性上报帧时钟
//! （fx_frame_due 第四路活性源）。
//!
//! 分层：纯逻辑（A 档考题 tests/viewport_push_spec.rs）；应用点在
//! android_app::draw_frame_gles（GLES 路径）。softbuffer 兜底路径
//! 保留旧覆盖语义（立项书红线：兜底不再投入——state.md 欠账条）。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ai_presence::Panel;

/// 一页的视口推移量（px，合成期 placement 加项）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Push {
    pub dx: f32,
    pub dy: f32,
}

/// 形变上限：scale = 1 - SQUASH_MAX = 0.94——「稍微形变」的用户拍板量级
/// （再大文字可感压扁，再小看不见）
pub const SQUASH_MAX: f32 = 0.06;
/// 压缩斜坡：进度前 22% 内压满（先形变再出去——形变是出场的预告）
pub const SQUASH_RAMP: f32 = 0.22;

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

/// 被压面板（栈内非顶）的额外位移 = 其上各面板推移之和（纯函数）。
/// 顶面板/不在栈 = 零额外。单面板 off 各自的推进度与 viewport_push 同源
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

/// 被压面板额外位移：栈底→顶序传入，目标面板之上每家的推移累加
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

/// 形变目标：进度 0 → 0；≥RAMP → 压满；中间线性（先形变再出去）
pub fn squash_target(p_max: f32) -> f32 {
    SQUASH_MAX * (p_max / SQUASH_RAMP).clamp(0.0, 1.0)
}

// ---- Q 弹积分器（欠阻尼弹簧离散步进） ----

/// 弹簧刚度/阻尼：ω=22 rad/s、ζ=0.55（欠阻尼——目标骤降必须过冲，
/// 这是「墩一下」的物理来源；ζ 再低入场鼓包可感，考题钉上限 35%）
const SQUASH_K: f32 = 22.0 * 22.0;
const SQUASH_C: f32 = 2.0 * 0.55 * 22.0;
/// 收敛判定：压缩量偏差与速度双小即贴死（防渐近空烧帧）
const SETTLE_S: f32 = 0.0005;
const SETTLE_V: f32 = 0.02;

/// 离散步进（纯函数）：(s, v) 在 dt 内朝 target 积分一步。
/// 半隐式欧拉（先 v 后 s）——显式在 8ms 步长 k=484 下数值不稳
pub fn squash_step(s: f32, v: f32, target: f32, dt_ms: u64) -> (f32, f32) {
    let dt = (dt_ms as f32 / 1000.0).min(0.05); // 帧间隔病态钳 50ms
    let a = -SQUASH_K * (s - target) - SQUASH_C * v;
    let v = v + a * dt;
    (s + v * dt, v)
}

/// 收敛判定（帧时钟停表判据）：贴目标且速度近零
pub fn squash_settled(s: f32, v: f32, target: f32) -> bool {
    (s - target).abs() < SETTLE_S && v.abs() < SETTLE_V
}

// ---- 壳层帧态（fx_spring 同族先例：静态帧态 + 活性上报帧时钟） ----

struct SquashState {
    s: f32,
    v: f32,
    target: f32,
    last_ms: u64,
    primed: bool,
}

static SQUASH: Mutex<SquashState> = Mutex::new(SquashState {
    s: 0.0,
    v: 0.0,
    target: 0.0,
    last_ms: 0,
    primed: false,
});
static SQUASH_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 每帧采样：喂当前目标（由面板 off 派生），得当前压缩量（scale=1-s）。
/// 首帧直通 0（冷启动不补演）；dt = 帧间隔实测（拖拽期事件驱动帧
/// 间隔不均，积分器照吃）
pub fn squash_sample(target: f32, now_ms: u64) -> f32 {
    let mut g = SQUASH.lock().unwrap();
    if !g.primed {
        g.primed = true;
        g.last_ms = now_ms;
        g.target = target;
        return 0.0;
    }
    let dt = now_ms.saturating_sub(g.last_ms);
    g.last_ms = now_ms;
    if target != g.target || !squash_settled(g.s, g.v, g.target) {
        let (s, v) = squash_step(g.s, g.v, target, dt);
        g.s = s;
        g.v = v;
        g.target = target;
    }
    let active = !squash_settled(g.s, g.v, g.target);
    SQUASH_ACTIVE.store(active, Ordering::Relaxed);
    if !active {
        g.s = g.target; // 贴死防漂移
        g.v = 0.0;
    }
    g.s
}

/// 形变弹簧还在动（fx_frame_due 活性源之一：面板缝停了它可能还在收）
pub fn squash_active() -> bool {
    SQUASH_ACTIVE.load(Ordering::Relaxed)
}

/// 考题/调试清态（帧态归冷启动）
#[doc(hidden)]
pub fn squash_reset_for_test() {
    *SQUASH.lock().unwrap() = SquashState {
        s: 0.0,
        v: 0.0,
        target: 0.0,
        last_ms: 0,
        primed: false,
    };
    SQUASH_ACTIVE.store(false, Ordering::Relaxed);
}
