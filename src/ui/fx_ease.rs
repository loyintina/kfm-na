//! fx_ease.rs — ui-fx 的定时缓动件：AI 面板落下/收起曲线（2026-09-04
//! 用户拍板：下落 ease-out、收起 ease-in——CSS transition 语言，取代
//! 弹簧的物理墩感；同日实测定档 350ms/250ms。弹簧退役到键盘 inset
//! 缝独占，见 fx_spring.rs）。
//!
//! 方向分档：目标 > 起点（向 0 靠泊 = 进场落下）= ease-out；反之为离场
//! 收起 = ease-in。纯函数零墙钟（A 档钉）；占缝采样自给自足——目标值
//! 变化即从当前值重定基续走（来回狂点位置不跳变）；首采样直通不重放
//! （冷启动/插件热装不补演一场）。

use std::sync::{Arc, Mutex};

/// 进场（落下）时长 ms：ease-out——开头快结尾慢，落位有「到位感」
/// （2026-09-04 实测定档：500 偏拖 → 350）
pub const ENTER_MS: u64 = 350;
/// 离场（收起）时长 ms：ease-in——开头慢结尾快，让位不拖泥
/// （2026-09-04 实测定档：400 偏拖 → 250）
pub const EXIT_MS: u64 = 250;

/// 三次 ease-out：1-(1-t)³（CSS cubic-bezier 的常用等价）
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// 三次 ease-in：t³
pub fn ease_in_cubic(t: f32) -> f32 {
    t.powi(3)
}

/// 方向分档定时缓动（纯函数）：from → target，elapsed_ms 时刻的位置。
/// 进场（target > from）= 500ms ease-out；离场 = 400ms ease-in；
/// elapsed 超时贴死 target——返回值 == target 即终态。
pub fn panel_ease_pos(from: f32, target: f32, elapsed_ms: u64) -> f32 {
    let d = target - from;
    if d == 0.0 {
        return target;
    }
    let (dur, ease) = if d > 0.0 {
        (ENTER_MS, ease_out_cubic as fn(f32) -> f32)
    } else {
        (EXIT_MS, ease_in_cubic as fn(f32) -> f32)
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

/// 装配一对缝占槽件（采样器 + 活性探针，共享同一份状态）——结构同
/// fx_spring::spring_occupier，只换曲线核
pub fn ease_occupier() -> crate::ui::seam::Occupier {
    let st = Arc::new(Mutex::new(EaseState::new()));
    let st2 = Arc::clone(&st);
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
    }
}
