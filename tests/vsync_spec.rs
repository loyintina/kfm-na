//! vsync_spec.rs — Choreographer vsync 对表考题（BAR-072 回归钉）。
//!
//! 案情（2026-09-09 面板闪退案）：vsync_cb 回调签名凭空多画了一个首位
//! *const AChoreographer 形参（NDK 契约 AChoreographer_frameCallback64
//! 只有 (frameTimeNanos, data) 两参）。回调里读到的 "c" 实为帧时间戳，
//! 自续挂表拿它当 this 回 post → Choreographer::postFrameCallbackDelayed
//! 对 时间戳+0x58 做 mutex::lock → SIGSEGV（三案故障地址低 48 位全部
//! 吻合同段开机时长纳秒数）。后台注入点光球 70+ 次不崩的「海森堡」假象
//! 同时得解：后台不渲染→动画不起→vsync 不武装，病灶根本没被喂到。
//!
//! 钉法两层：
//!   ①ABI 层（编译期执法）：gles_present 的 ChoreoCallback 类型钉死两参
//!     契约，vsync_cb_for_ndk() 以该型导出真回调——签名回退三参幻觉版
//!     即 android 编核编译红（壳是 cfg(android) 禁地，host 考题够不着，
//!     这层靠 chain 的 android-check 执法）
//!   ②记账层（本卷判卷）：vsync_book::note_tick 首跳只建基线、其后逐跳
//!     记周期 min/max/累计；账本已剥进核心层，host 可直判
//!
//! 串行纪律：两题都碰进程级静态账本，VSYNC_TEST_LOCK 串行进场
//! （BAR-057 并行截胡教训）。

use kfm_na::vsync_book;

static VSYNC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// BAR-072 钉①：记账语义——首跳只建基线不计样本（0 基线防 0 除/假巨间隔）；
// ns=0 不是合法帧时刻，不许建基线（旧版 ts 实收 data=null 恒 0 时账本
// 永远空转的哑态正是本案旁证：field-reports 史上零 vsync 样本）。
// 变异抽检：把 note_tick 的 `last != 0` 守卫摘掉，零时刻题当场红。
#[test]
fn spec_bar072_记账_首跳建基线() {
    let _g = VSYNC_TEST_LOCK.lock().unwrap();
    vsync_book::reset_for_test();
    vsync_book::note_tick(0); // 0 不建基线
    vsync_book::note_tick(0);
    let (n, _, _, _) = vsync_book::snapshot_for_test();
    assert_eq!(n, 0, "零时刻不许产样本");
    vsync_book::note_tick(5_000_000_000); // 首跳：建基线
    let (n, _, _, _) = vsync_book::snapshot_for_test();
    assert_eq!(n, 0, "首跳只建基线");
    vsync_book::note_tick(5_000_000_000 + 11_111_111); // 次跳：记 11111us
    let (n, min, max, total) = vsync_book::snapshot_for_test();
    assert_eq!((n, min, max, total), (1, 11_111, 11_111, 11_111));
    // 逆序保护：时间戳回拨不产生巨间隔（saturating_sub）
    vsync_book::note_tick(1_000_000);
    let (n, min, _, total) = vsync_book::snapshot_for_test();
    assert_eq!((n, min, total), (2, 0, 11_111));
}

// BAR-072 钉②：60Hz 三跳记两间隔，min/max/累计逐值咬合；
// take_gap 取账即清（收尾只报一次），无样本返回 None（vsync_report
// 的「vsync无样本」分支）。
// 变异抽检：saturating_sub 改普通减法，本卷回拨题 panic；take_gap 不
// 清零，二取必红。
#[test]
fn spec_bar072_记账_60hz三跳_取账清零() {
    let _g = VSYNC_TEST_LOCK.lock().unwrap();
    vsync_book::reset_for_test();
    vsync_book::note_tick(1_000_000_000);
    vsync_book::note_tick(1_000_000_000 + 16_666_666);
    vsync_book::note_tick(1_000_000_000 + 33_333_332);
    let (n, min, max, total) = vsync_book::snapshot_for_test();
    assert_eq!(n, 2, "三跳两间隔");
    assert_eq!(min, 16_666_666 / 1000);
    assert_eq!(max, 16_666_666 / 1000);
    assert_eq!(total, min + max);
    // 取账即清
    assert_eq!(vsync_book::take_gap(), Some((n, min, max, total)));
    let (n2, _, _, _) = vsync_book::snapshot_for_test();
    assert_eq!(n2, 0, "take_gap 后账本已清");
    assert_eq!(vsync_book::take_gap(), None, "无样本取账=None");
}
