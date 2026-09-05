//! fx_ease.rs — ui-fx 的定时缓动件：AI 面板落下/收起曲线（2026-09-04
//! 用户拍板：下落 ease-out、收起 ease-in——CSS transition 语言，取代
//! 弹簧的物理墩感；同日实测定档 350ms/250ms。弹簧退役到键盘 inset
//! 缝独占，见 fx_spring.rs。2026-09-05 曲线升级：裸 ease-out 起步即
//! 峰值速度，大幅面实看不适——换 Material emphasized 族（Android 12+
//! 大面板转场用曲），enter=emphasized(0.2,0,0,1) / exit=
//! emphasized-accelerate(0.3,0,0.8,0.15)，时长照旧 350/250）。
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

/// 三次贝塞尔单分量：B(u) = 3(1-u)²u·P1 + 3(1-u)u²·P2 + u³
fn bezier_component(u: f32, p1: f32, p2: f32) -> f32 {
    let om = 1.0 - u;
    3.0 * om * om * u * p1 + 3.0 * om * u * u * p2 + u * u * u
}

/// CSS cubic-bezier(x1,y1,x2,y2) 求值（A 档纯函数）：x(u) 单调
/// （x1,x2 ∈ (0,1)）→ 二分解 u（24 轮，亚 1e-4 px 精度）→ 代入 y(u)。
/// Material Design 3 emphasized 曲线的实现底座
pub fn cubic_bezier_y(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bezier_component(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let u = (lo + hi) / 2.0;
    bezier_component(u, y1, y2)
}

/// Material emphasized（cubic-bezier(0.2, 0, 0, 1)）：Android 12+ 全系统
/// 大面板转场用曲——慢起、快中、缓收，两端都缓（裸 ease-out 的起步即
/// 峰值速度在大幅面上显得「突然起跳」，2026-09-05 用户实看不适换装）
pub fn emphasized(t: f32) -> f32 {
    cubic_bezier_y(t, 0.2, 0.0, 0.0, 1.0)
}

/// Material emphasized-accelerate（cubic-bezier(0.3, 0, 0.8, 0.15)）：
/// 离场加速——开头迟疑、末段呼啸离屏（MD3 规范值）
pub fn emphasized_accelerate(t: f32) -> f32 {
    cubic_bezier_y(t, 0.3, 0.0, 0.8, 0.15)
}

/// 方向分档定时缓动（纯函数）：from → target，elapsed_ms 时刻的位置。
/// 进场（target > from）= emphasized；离场 = emphasized-accelerate；
/// elapsed 超时贴死 target——返回值 == target 即终态。
pub fn panel_ease_pos(from: f32, target: f32, elapsed_ms: u64) -> f32 {
    let d = target - from;
    if d == 0.0 {
        return target;
    }
    let (dur, ease) = if d > 0.0 {
        (ENTER_MS, emphasized as fn(f32) -> f32)
    } else {
        (EXIT_MS, emphasized_accelerate as fn(f32) -> f32)
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
