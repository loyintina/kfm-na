//! fx_ease.rs — ui-fx 的定时缓动件：AI 面板落下/收起曲线（2026-09-04
//! 用户拍板：下落 ease-out、收起 ease-in——CSS transition 语言，取代
//! 弹簧的物理墩感。弹簧退役到键盘 inset
//! 缝独占，见 fx_spring.rs。曲线沿革：09-05 裸 ease-out → Material
//! emphasized；09-06 重力 t²；09-10 加配滑动淡入；**09-11 定稿
//! power2.out（1-(1-t)²，nz/kfmv4 的 GSAP 同款）**——重力 t² 被用户
//! 录屏逐帧实锤判「冻结→跳变」：起步亚像素蠕动 + alpha 从落程推导
//! 跟着 t² 走，前 100ms 面板近乎不可见，显影时已在加速段。power2.out
//! 起步即快（10% 时间走 19% 路程），第一帧就有可见位移。**09-11 二审：
//! 淡入淡出取消（调用点恒 1.0，panel_fade_alpha 退役备查）+ 时长提速
//! 350/250 → 250/180。**
//!
//! 方向分档：进场落下 = power2.out 减速；离场收起 = 镜像减速。纯函数零墙钟（A 档钉）；占缝采样自给自足——目标值
//! 变化即从当前值重定基续走（来回狂点位置不跳变）；首采样直通不重放
//! （冷启动/插件热装不补演一场）。

use std::sync::{Arc, Mutex};

/// 进场（落下）时长 ms：ease-out——开头快结尾慢，落位有「到位感」
/// （定档沿革：09-04 实测 500 偏拖 → 350；09-11 用户拍板再提速 → 250）
pub const ENTER_MS: u64 = 250;
/// 离场（收起）时长 ms：ease-in——开头慢结尾快，让位不拖泥
/// （定档沿革：09-04 实测 400 偏拖 → 250；09-11 用户拍板再提速 → 180）
pub const EXIT_MS: u64 = 180;

/// 三次 ease-out：1-(1-t)³（CSS cubic-bezier 的常用等价）
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// 三次 ease-in：t³
pub fn ease_in_cubic(t: f32) -> f32 {
    t.powi(3)
}

/// power2.out 落下（2026-09-11 定稿，nz/kfmv4 GSAP 同款）：减速曲线
/// 1-(1-t)²——起步即快（第一帧就有可见位移）、缓停到位。前身重力 t²
/// （自由落体）退役：起步亚像素蠕动 + alpha 随落程平方显影，被录屏
/// 逐帧实锤读作「冻结→跳变」
pub fn power2_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// 收起 = 落下的镜像旅程（面板原路回去）：减速上升，1-(1-t)²
pub fn rise_release(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// 淡入窗。**2026-09-11 用户拍板退役**：面板淡入淡出取消（「不好看」），
/// 调用点改恒 1.0；纯函数与考题保留备查（将来若要复用显影配方直接接线）
pub const FADE_PORTION: f32 = 0.35;

/// 滑动淡入显影（纯函数零状态，A 档钉）：alpha 从 placement 推导。
/// **已退役（2026-09-11）**：调用点恒 1.0，本函数仅考题钉住备查。
/// off ∈ [-h, 0]：屏外 → 靠泊
pub fn panel_fade_alpha(off: f32, screen_h: f32) -> f32 {
    if screen_h <= 0.0 {
        return 1.0; // 病态尺寸不许黑屏——直通全实
    }
    let progress = (1.0 + off / screen_h).clamp(0.0, 1.0);
    (progress / FADE_PORTION).clamp(0.0, 1.0)
}

/// 方向分档定时缓动（纯函数）：from → target，elapsed_ms 时刻的位置。
/// 进场（target > from）= power2.out 减速；离场 = 镜像减速；
/// elapsed 超时贴死 target——返回值 == target 即终态。
pub fn panel_ease_pos(from: f32, target: f32, elapsed_ms: u64) -> f32 {
    let d = target - from;
    if d == 0.0 {
        return target;
    }
    let (dur, ease) = if d > 0.0 {
        (ENTER_MS, power2_out as fn(f32) -> f32)
    } else {
        (EXIT_MS, rise_release as fn(f32) -> f32)
    };
    if elapsed_ms >= dur {
        return target;
    }
    let t = elapsed_ms as f32 / dur as f32;
    from + d * ease(t)
}

/// 缓动采样器状态：目标值变化即从当前值重定基（from=此刻位置）
struct EaseState {
    from: f32,
    target: f32,
    start_ms: u64,
    settled: bool,
    primed: bool, // 首采样直通：冷启动不重放历史
}

impl EaseState {
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

/// 装配一对缝占槽件（采样器 + 活性探针，共享同一份状态）+ 入场重播踢
/// （BAR-079 坍缩②：覆盖再召唤时壳层踢来屏外位——重定基 from=屏外位、
/// 目标不动 → 重播入场；被覆盖者不可见，踢跳变不可见。未首采忽略：
/// 冷启动/插件热装直通语义不破）——结构同 fx_spring::spring_occupier，
/// 只换曲线核
pub fn ease_occupier() -> crate::ui::seam::Occupier {
    let st = Arc::new(Mutex::new(EaseState::new()));
    let st2 = Arc::clone(&st);
    let st3 = Arc::clone(&st);
    crate::ui::seam::Occupier {
        sampler: Arc::new(move |target: f32, now_ms: u64| {
            let mut g = st.lock().unwrap();
            if !g.primed {
                *g = EaseState {
                    from: target,
                    target,
                    start_ms: now_ms,
                    settled: true,
                    primed: true,
                };
                return target;
            }
            if target != g.target {
                // 重定基：从当前值续走（来回狂点位置不跳变）
                let pos = panel_ease_pos(g.from, g.target, now_ms.saturating_sub(g.start_ms));
                g.from = pos;
                g.target = target;
                g.start_ms = now_ms;
                g.settled = false;
            }
            let pos = panel_ease_pos(g.from, g.target, now_ms.saturating_sub(g.start_ms));
            g.settled = pos == target;
            pos
        }),
        is_active: Arc::new(move || !st2.lock().unwrap().settled),
        replay: Some(Arc::new(move |offscreen: f32, now_ms: u64| {
            let mut g = st3.lock().unwrap();
            if !g.primed {
                return; // 未首采 = 直通态，无需重播（冷启动语义）
            }
            g.from = offscreen;
            g.start_ms = now_ms;
            g.settled = false;
        })),
    }
}
