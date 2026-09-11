//! vsync_book.rs — Choreographer vsync 对表账本（核心层纯数据面）。
//!
//! 为什么独立成册：BAR-072（2026-09-09 面板闪退案）的考题必须 host 可
//! 判卷，而平台壳 gles_present 是 cfg(android) 禁地。账本=纯原子计数，
//! 零平台依赖；壳只负责 dlsym/回调 ABI（两参契约钉在壳的 ChoreoCallback
//! 类型注释里——签名回退三参幻觉版即编译红）。
//!
//! 案情存档：vsync_cb 曾按 (choreographer, ts, data) 三参幻觉声明，实际
//! NDK 喂进来的是 (frameTimeNanos, data) 两参——回调把帧时间戳当 this
//! 回 postFrameCallback64 → 时间戳+0x58 处 mutex::lock → SIGSEGV。
//! 旁证：ts 实收 data=null → 本账史上零样本（field-reports 无一行
//! vsync 数据），与 crash 互证。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ARMED: AtomicBool = AtomicBool::new(false);
static LAST_NS: AtomicU64 = AtomicU64::new(0);
static GAP_MIN_US: AtomicU64 = AtomicU64::new(0);
static GAP_MAX_US: AtomicU64 = AtomicU64::new(0);
static GAP_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static GAP_N: AtomicU64 = AtomicU64::new(0);

pub fn arm() {
    ARMED.store(true, Ordering::Relaxed);
}
pub fn disarm() {
    ARMED.store(false, Ordering::Relaxed);
}
pub fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// ---- BAR-081 锁相泵 due 账（2026-09-11）----
/// 动画期帧时钟的产帧许可：Choreographer 每跳由壳（gles_present::vsync_cb）
/// 记一笔 due，fx_frame_due 消费一笔产一帧——一跳一帧、相位跟屏（旧定时器
/// 泵与 vsync 互不锁相的残影病灶根治，案卷见 tests/vsync_spec.rs BAR-081）。
/// 连跳合并：消费前积 N 跳也只产一帧（跳帧合并，采样吃最新时刻）。
static DUE_N: AtomicU64 = AtomicU64::new(0);
/// 最近一次 due 的壳层毫秒戳（report::boot_ms 同钟）——看门狗判链死用；
/// 0 = 本轮武装以来还没跳过
static LAST_DUE_MS: AtomicU64 = AtomicU64::new(0);
/// 链死闩锁（BAR-081 看门狗）：fx_frame_due 判死即上闩——链死期帧时钟
/// 整体退成预算节流节奏（不是放一帧又憋 32ms）；新跳到（note_due）
/// 自动解闩重锁。复位归 reset_run/reset_for_test
static CHAIN_DEAD: AtomicBool = AtomicBool::new(false);
/// 本轮武装时刻（BAR-082 看门狗基线）：动画开表（reset_run）落戳——
/// 「武装后零跳」时看门狗以此为参考点（旧版参考点只有 last_due×
/// LAST_FRAME，两者皆 0 = 永久信任 = 首跳丢失即动画冻结，栈叠收回
/// 「无动画直接消失」案的真凶）
static ARM_MS: AtomicU64 = AtomicU64::new(0);

/// vsync 一跳记账（壳 vsync_cb 专用入口）——顺带解链死闩（链复活即重锁）
pub fn note_due(now_ms: u64) {
    DUE_N.fetch_add(1, Ordering::Relaxed);
    LAST_DUE_MS.store(now_ms, Ordering::Relaxed);
    CHAIN_DEAD.store(false, Ordering::Relaxed);
}

/// 消费全部积欠（返回跳数；0 = 本跳周期还没来——无跳不帧）
pub fn take_due() -> u64 {
    DUE_N.swap(0, Ordering::Relaxed)
}

/// 最近一跳时刻（看门狗：now - last_due_ms > 32 = 回调链死）
pub fn last_due_ms() -> u64 {
    LAST_DUE_MS.load(Ordering::Relaxed)
}

/// 链死闩锁探视/上闩（fx_frame_due 看门狗专用）
pub fn chain_dead() -> bool {
    CHAIN_DEAD.load(Ordering::Relaxed)
}
pub fn mark_chain_dead() {
    CHAIN_DEAD.store(true, Ordering::Relaxed);
}

/// 本轮武装时刻（BAR-082：fx_frame_due 看门狗参考点三元组之一；
/// 0 = 本轮还没开表——老代码路径/考题零基线语义）
pub fn arm_ms() -> u64 {
    ARM_MS.load(Ordering::Relaxed)
}

/// 相位判卷用：最近一次回调的帧时间戳（0=还没收到过）
pub fn last_ns() -> u64 {
    LAST_NS.load(Ordering::Relaxed)
}

/// 逐跳记账：首跳只建基线不计样本（0 基线防假巨间隔），其后逐跳
/// 记周期 min/max/累计；ns=0 不是合法帧时刻，不建基线。
/// 时间戳回拨 saturating_sub 归零，不产巨间隔。
pub fn note_tick(ns: u64) {
    let last = LAST_NS.swap(ns, Ordering::Relaxed);
    if last != 0 {
        let gap = ns.saturating_sub(last) / 1000;
        GAP_N.fetch_add(1, Ordering::Relaxed);
        GAP_TOTAL_US.fetch_add(gap, Ordering::Relaxed);
        GAP_MIN_US.fetch_min(gap, Ordering::Relaxed);
        GAP_MAX_US.fetch_max(gap, Ordering::Relaxed);
    }
}

/// 动画轮开表清账（壳 anim_run_start 调用）。now_ms = 武装基线戳
/// （BAR-082：看门狗「从未跳」死法的参考点，report::boot_ms 同钟）
pub fn reset_run(now_ms: u64) {
    ARM_MS.store(now_ms, Ordering::Relaxed);
    GAP_N.store(0, Ordering::Relaxed);
    GAP_TOTAL_US.store(0, Ordering::Relaxed);
    GAP_MIN_US.store(u64::MAX, Ordering::Relaxed);
    GAP_MAX_US.store(0, Ordering::Relaxed);
    LAST_NS.store(0, Ordering::Relaxed);
    // BAR-081：due 账+链死闩同清——上轮回调整链的尾随跳不许污染新一轮锁相
    DUE_N.store(0, Ordering::Relaxed);
    LAST_DUE_MS.store(0, Ordering::Relaxed);
    CHAIN_DEAD.store(false, Ordering::Relaxed);
}

/// 动画收尾取账并清（壳 vsync_report 调用；无样本返回 None）
pub fn take_gap() -> Option<(u64, u64, u64, u64)> {
    let n = GAP_N.load(Ordering::Relaxed);
    if n == 0 {
        return None;
    }
    let total = GAP_TOTAL_US.swap(0, Ordering::Relaxed);
    let min = GAP_MIN_US.swap(0, Ordering::Relaxed);
    let max = GAP_MAX_US.swap(0, Ordering::Relaxed);
    GAP_N.store(0, Ordering::Relaxed);
    Some((n, min, max, total))
}

/// BAR-072 考题探视口：(样本数, min_us, max_us, total_us) 只读快照
#[doc(hidden)]
pub fn snapshot_for_test() -> (u64, u64, u64, u64) {
    (
        GAP_N.load(Ordering::Relaxed),
        GAP_MIN_US.load(Ordering::Relaxed),
        GAP_MAX_US.load(Ordering::Relaxed),
        GAP_TOTAL_US.load(Ordering::Relaxed),
    )
}

/// BAR-072 考题清账（考题串行进场，见 tests/vsync_spec.rs）。
/// 纪律同 reset_run：min 归位 u64::MAX——fetch_min 语义下 0 会永远
/// 压住真值（取 min 的账底必须是天花板）。
#[doc(hidden)]
pub fn reset_for_test() {
    LAST_NS.store(0, Ordering::Relaxed);
    GAP_N.store(0, Ordering::Relaxed);
    GAP_MIN_US.store(u64::MAX, Ordering::Relaxed);
    GAP_MAX_US.store(0, Ordering::Relaxed);
    GAP_TOTAL_US.store(0, Ordering::Relaxed);
    DUE_N.store(0, Ordering::Relaxed);
    LAST_DUE_MS.store(0, Ordering::Relaxed);
    CHAIN_DEAD.store(false, Ordering::Relaxed);
    ARM_MS.store(0, Ordering::Relaxed);
}
