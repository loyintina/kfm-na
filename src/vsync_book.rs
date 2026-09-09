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

/// 动画轮开表清账（壳 anim_run_start 调用）
pub fn reset_run() {
    GAP_N.store(0, Ordering::Relaxed);
    GAP_TOTAL_US.store(0, Ordering::Relaxed);
    GAP_MIN_US.store(u64::MAX, Ordering::Relaxed);
    GAP_MAX_US.store(0, Ordering::Relaxed);
    LAST_NS.store(0, Ordering::Relaxed);
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
}
