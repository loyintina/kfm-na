//! fx_spring.rs — ui-fx 的弹簧核：键盘 inset（chrome 跟随）曲线。
//! （2026-09-04 沿革：本件原是 AI 面板落下/升起曲线；用户拍板面板
//! 改定时缓动「落 500ms ease-out / 收 400ms ease-in」→ fx_ease.rs，
//! 弹簧退役到键盘 inset 缝独占——100ms 轮询轨迹是阶梯，纯镜像太硬。）
//!
//! 曲线 = 欠阻尼弹簧（纯函数零墙钟，A 档钉）；占缝采样自给自足——
//! 目标值变化即从当前值重定基续弹（来回狂点不跳变）；首采样直通
//! 不重放（冷启动/插件热装不补演一场）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 阻尼比：过冲 ≈2.5% 屏高——「落下来墩一下」的手感（C 档实拍可调）
const ZETA: f32 = 0.76;
/// 阻尼角频率 rad/s：首过冲峰 ≈150ms，全程 ≈350ms 收敛
const OMEGA_D: f32 = 20.9;
/// 收敛判定：位置偏差与速度双小即贴死目标（防无限渐近空烧帧）
const SETTLE_PX: f32 = 0.5;
const SETTLE_VEL: f32 = 40.0; // px/s
/// 兜底：超时强制贴死（病态参数也不许永动）
const SETTLE_TIMEOUT_MS: u64 = 600;

/// 欠阻尼弹簧采样（纯函数）：from → target，elapsed_ms 时刻的位置。
/// 收敛（位置/速度双小或超时）贴死 target——返回值 == target 即终态。
pub fn spring_pos(from: f32, target: f32, elapsed_ms: u64) -> f32 {
    let d = from - target;
    if d == 0.0 {
        return target;
    }
    let t = elapsed_ms as f32 / 1000.0;
    let omega = OMEGA_D / (1.0 - ZETA * ZETA).sqrt(); // 固有角频率
    let env = (-ZETA * omega * t).exp();
    let phase = OMEGA_D * t;
    let k = ZETA * omega / OMEGA_D;
    let pos = target + d * env * (phase.cos() + k * phase.sin());
    // v(t) = -d·e^(-ζωt)·(ω²/ω_d)·sin(ω_d·t)（解析导数，收敛判据用）
    let vel = (-d * env * (omega * omega / OMEGA_D) * phase.sin()).abs();
    if ((pos - target).abs() < SETTLE_PX && vel < SETTLE_VEL) || elapsed_ms >= SETTLE_TIMEOUT_MS {
        target
    } else {
        pos
    }
}

/// 弹簧采样器状态：目标值变化即从当前值重定基（from=此刻位置）
struct SpringState {
    from: f32,
    target: f32,
    start_ms: u64,
    settled: bool,
    primed: bool, // 首采样直通：冷启动不重放历史
}

impl SpringState {
    fn new() -> Self {
        Self {
            from: 0.0,
            target: 0.0,
            start_ms: 0,
            settled: true,
            primed: false,
        }
    }
}

/// 装配一对缝占槽件（采样器 + 活性探针，共享同一份状态）
pub fn spring_occupier() -> crate::ui::seam::Occupier {
    let st = Arc::new(Mutex::new(SpringState::new()));
    let st2 = Arc::clone(&st);
    crate::ui::seam::Occupier {
        sampler: Arc::new(move |target: f32, now_ms: u64| {
            let mut g = st.lock().unwrap();
            if !g.primed {
                *g = SpringState {
                    from: target,
                    target,
                    start_ms: now_ms,
                    settled: true,
                    primed: true,
                };
                return target;
            }
            if target != g.target {
                // 重定基：从当前值续弹（来回狂点不跳变）
                let pos = spring_pos(g.from, g.target, now_ms.saturating_sub(g.start_ms));
                g.from = pos;
                g.target = target;
                g.start_ms = now_ms;
                g.settled = false;
            }
            let pos = spring_pos(g.from, g.target, now_ms.saturating_sub(g.start_ms));
            g.settled = pos == target;
            pos
        }),
        is_active: Arc::new(move || !st2.lock().unwrap().settled),
    }
}

// ---- 帧时钟（ui-base §四：按需启停，≤60fps，动画停即停表） ----

static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);

/// 该画动画帧了：任一缝上有活跃动画且距上帧 ≥16ms（约 60fps 上限）；
/// 无活跃动画恒 false——零额外帧零唤醒（夜判据 0.45% 单核红线）。
/// 两道缝共用一只钟（2026-09-04 键盘 inset 缝入册：同窗同帧不双泵）
pub fn fx_frame_due(now_ms: u64) -> bool {
    let active =
        crate::ui::seam::ai_panel_offset_y_active() || crate::ui::seam::chrome_ime_inset_active();
    if !active {
        LAST_FRAME_MS.store(0, Ordering::Relaxed);
        return false;
    }
    let last = LAST_FRAME_MS.load(Ordering::Relaxed);
    if last != 0 && now_ms.saturating_sub(last) < 16 {
        return false;
    }
    LAST_FRAME_MS.store(now_ms, Ordering::Relaxed);
    true
}

/// 旧名委托（AI 面板缝独存时代的考题还在用——语义已泛化成「任一缝」）
pub fn panel_frame_due(now_ms: u64) -> bool {
    fx_frame_due(now_ms)
}
