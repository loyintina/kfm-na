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
        replay: None, // 键盘 inset 无「入场」概念（BAR-079：重播踢归面板缝）
    }
}

// ---- 帧时钟（ui-base §四：按需启停，帧预算=显示刷新周期，动画停即停表） ----

static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);

/// 帧预算（一次动画帧的最小间隔，BAR-077）：默认 16ms（≈60fps 保守基线——
/// 核心层零平台依赖，壳没喂数字前必须能活）。壳每次 resumed 经 JNI 读
/// `Display.getMode().getRefreshRate()` 写真实刷新周期（120Hz 屏 → 8ms）。
/// 拍板存档：本机屏 120Hz（vsync 账本实测 110-120Hz），写死 16ms =
/// 屏幕每刷两次画面才动一次 = 用户实看的「落下拖影」。
static FRAME_BUDGET_MS: AtomicU64 = AtomicU64::new(16);

/// 写帧预算并回生效值：钳 4~33ms 防病态——4ms（250fps）封顶防烧 CPU，
/// 33ms（30fps）托底防动画冻死；离谱读数（0/负/千级）不许进系统
pub fn set_frame_budget_ms(ms: u64) -> u64 {
    let v = ms.clamp(4, 33);
    FRAME_BUDGET_MS.store(v, Ordering::Relaxed);
    v
}

/// 当前帧预算（考题探视口/壳层回报用）
pub fn frame_budget_ms() -> u64 {
    FRAME_BUDGET_MS.load(Ordering::Relaxed)
}

/// BAR-081 考题清钟（vsync_spec 锁相考题串行进场用）：LAST_FRAME_MS 归 0。
/// 归 0 语义 = 「动画刚开始」——下一笔 fx_frame_due 直通首帧，与产线一致
#[doc(hidden)]
pub fn reset_frame_clock_for_test() {
    LAST_FRAME_MS.store(0, Ordering::Relaxed);
}

/// 锁相看门狗阈值（BAR-081）：挂表后超过这么久没收到 vsync 跳 =
/// 回调链死（getInstance 空/首跳丢失/链半路断）——预算节流兜底防动画
/// 卡死。取 32ms ≈ 30fps 两跳：正常 120Hz 屏 8.6ms 一跳绝不会误伤，
/// 60Hz 屏 16.7ms 一跳也够不着
const VSYNC_WATCHDOG_MS: u64 = 32;

/// 该画动画帧了：无活跃动画恒 false——零额外帧零唤醒（夜判据 0.45%
/// 单核红线）。四道缝共用一只钟（2026-09-04 键盘 inset 缝入册、09-10
/// 配置面板 X 缝入册、09-11 文件树面板 X 缝入册：同窗同帧不双泵。
/// 09-12 Q 弹形变弹簧曾入册为第四路活性源，同日二审取消退役）。
/// 产帧许可双模（BAR-081，2026-09-11 残影定案）：
/// - **vsync 挂表期 = 锁相**：只认 due 账一跳一帧（定时器泵与 8.6ms
///   vsync 双钟不锁相 = swap 间隔 1/9/25ms 的残影真凶，锁相后相位跟屏）；
///   连跳合并产一帧；链死 >32ms 看门狗落预算节流兜底，链复活立即重锁
/// - **未挂表 = 预算节流**（旧路保留）：距上帧 ≥帧预算产一帧，壳喂真实
///   刷新周期后跟屏走（BAR-077）
pub fn fx_frame_due(now_ms: u64) -> bool {
    let active = crate::ui::seam::ai_panel_offset_y_active()
        || crate::ui::seam::chrome_ime_inset_active()
        || crate::ui::seam::config_panel_offset_x_active()
        || crate::ui::seam::filetree_panel_offset_x_active();
    if !active {
        LAST_FRAME_MS.store(0, Ordering::Relaxed);
        return false;
    }
    if crate::vsync_book::armed() {
        // 锁相支路：一跳一帧，消费清零
        if crate::vsync_book::take_due() > 0 {
            LAST_FRAME_MS.store(now_ms, Ordering::Relaxed);
            return true;
        }
        // 链死闩已上 = 回调链死后整体退预算节奏（落下方节流），
        // 新跳到由 note_due 解闩即重锁
        if !crate::vsync_book::chain_dead() {
            // 看门狗：参考点 = 最近一跳 / 最近一帧 / 本轮武装戳 的较晚者
            // （BAR-082：武装戳入列——「武装后零跳」时前两者皆 0，旧版
            // ref==0 永久信任 = 首跳丢失即动画冻结，栈叠收回「无动画
            // 直接消失」案真凶）；超阈上闩落预算路，未超信任期等跳
            let ref_ms = crate::vsync_book::last_due_ms()
                .max(LAST_FRAME_MS.load(Ordering::Relaxed))
                .max(crate::vsync_book::arm_ms());
            if ref_ms == 0 || now_ms.saturating_sub(ref_ms) <= VSYNC_WATCHDOG_MS {
                return false;
            }
            crate::vsync_book::mark_chain_dead();
        }
    }
    let last = LAST_FRAME_MS.load(Ordering::Relaxed);
    if last != 0 && now_ms.saturating_sub(last) < FRAME_BUDGET_MS.load(Ordering::Relaxed) {
        return false;
    }
    LAST_FRAME_MS.store(now_ms, Ordering::Relaxed);
    true
}

/// 旧名委托（AI 面板缝独存时代的考题还在用——语义已泛化成「任一缝」）
pub fn panel_frame_due(now_ms: u64) -> bool {
    fx_frame_due(now_ms)
}
