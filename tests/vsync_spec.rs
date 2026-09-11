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

use kfm_na::ui::{fx_spring, seam};
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

// ---- BAR-081（2026-09-11 残影定案：双钟不锁相）锁相泵考题 ----
//
// 案情：面板落下「好几帧同位置→突然跳变」，用户录屏逐帧实锤。仪器定罪
// （panel-anim 报表）：渲染均耗 3ms（管线跑得起 120fps）、vsync 116Hz
// （8.6ms/跳），但 swap 间隔 min/avg/max=1/9/25ms——有的 vsync 周期塞
// 两帧（一帧白画=同位置），有的周期零帧（25ms 冻结）。真凶 = 帧时钟
// 是 4ms 轮询+预算节流的定时器，与 8.6ms vsync 两个时钟互不锁相。
// 修法：动画期 fx_frame_due 改吃 vsync due 账（一跳一帧），Choreographer
// 缺席/链死自动回退预算路（看门狗 32ms）。

// BAR-081 钉①：锁相——武装后无跳不产帧；一跳产一帧、消费清零同跳
// 不续产；连跳合并产一帧（跳帧合并，不许连产）；收表回预算路。
// 变异抽检：take_due 只读不清（load 不换出），「同跳不许产第二帧」当场红。
#[test]
fn spec_bar081_锁相_一跳一帧() {
    let _g = VSYNC_TEST_LOCK.lock().unwrap();
    vsync_book::reset_for_test();
    vsync_book::disarm();
    fx_spring::reset_frame_clock_for_test(); // 跨题钟面归零（进程级静态账）
    // 起一只活跃动画（帧时钟的前提，同 fx_spring_spec 起手式）
    seam::occupy_ai_panel_offset_y(fx_spring::spring_occupier());
    assert_eq!(seam::sample_ai_panel_offset_y(-100.0, 2000), -100.0); // primed
    seam::sample_ai_panel_offset_y(0.0, 2000); // 目标翻转 = 动画开始
    // 未武装 = 预算路照旧（向后兼容钉：Choreographer 缺席平台的活法）
    assert!(fx_spring::fx_frame_due(2000), "未武装=预算路首帧");
    assert!(!fx_spring::fx_frame_due(2004), "预算路 4ms 不许产");
    // 武装（动画开表）→ 锁相：信任窗内预算早到点也不许产，只认 vsync 跳
    // （32ms 信任窗 = 看门狗地盘，锁相断言全部收在窗内）
    vsync_book::reset_run(2020);
    vsync_book::arm();
    assert!(!fx_spring::fx_frame_due(2020), "锁相后无跳不许产帧");
    // 一跳 → 一帧；消费清零 → 同跳不再产
    vsync_book::note_due(2024);
    assert!(fx_spring::fx_frame_due(2025), "一跳必须产一帧");
    assert!(!fx_spring::fx_frame_due(2026), "同跳不许产第二帧");
    assert!(!fx_spring::fx_frame_due(2028), "同跳任何时候都不许续产");
    // 跳帧合并：消费前连来 3 跳 = 产一帧（合并不是丢失：位置采样吃
    // 最新时刻，画面跟上最新 vsync），产完即干
    vsync_book::note_due(2033);
    vsync_book::note_due(2041);
    vsync_book::note_due(2050);
    assert!(fx_spring::fx_frame_due(2051), "合并跳必须产一帧");
    assert!(!fx_spring::fx_frame_due(2052), "合并跳不许续产");
    // 收表 → 回预算路（动画收尾/测量仪表下班后的退路）
    vsync_book::disarm();
    assert!(fx_spring::fx_frame_due(2100), "收表必须回预算路产帧");
    seam::release_ai_panel_offset_y();
    vsync_book::reset_for_test();
}

// BAR-081 钉②：看门狗——挂表但回调链死（>32ms 无跳）→ 预算节流兜底
// 防动画卡死；链复活（新跳到）立即重锁。覆盖两种死法：武装后从未跳
// （getInstance 空/首跳丢失）+ 跳到一半链断。
// 变异抽检：看门狗支路删掉（无跳恒 false），本题 3033/3084 臂当场红。
#[test]
fn spec_bar081_锁相_看门狗防卡死() {
    let _g = VSYNC_TEST_LOCK.lock().unwrap();
    vsync_book::reset_for_test();
    vsync_book::disarm();
    fx_spring::reset_frame_clock_for_test(); // 跨题钟面归零
    seam::occupy_ai_panel_offset_y(fx_spring::spring_occupier());
    assert_eq!(seam::sample_ai_panel_offset_y(-100.0, 3000), -100.0);
    seam::sample_ai_panel_offset_y(0.0, 3000);
    assert!(fx_spring::fx_frame_due(3000), "预算路首帧");
    // 死法一：武装后从未跳——信任期 32ms 内不产，超时看门狗放帧
    // （基线 = 预算路首帧 3000 与武装戳 3000 的较晚者）
    vsync_book::reset_run(3000);
    vsync_book::arm();
    assert!(!fx_spring::fx_frame_due(3032), "信任期内无跳不产");
    assert!(
        fx_spring::fx_frame_due(3033),
        "从未跳 33ms 后看门狗必须放帧"
    );
    // 看门狗放帧后按预算节流续走（链死期退化=定时器模式，不是放一帧又死）
    assert!(!fx_spring::fx_frame_due(3040), "看门狗后仍受预算节流");
    assert!(fx_spring::fx_frame_due(3049), "看门狗后预算到点续产");
    // 链复活：新跳到 → 立即重锁（一跳一帧，不等预算）
    vsync_book::note_due(3050);
    assert!(fx_spring::fx_frame_due(3051), "链复活立即重锁产帧");
    assert!(!fx_spring::fx_frame_due(3052), "重锁后同跳不续产");
    // 死法二：跳到一半链断——最后一帧 33ms 后看门狗再放行
    assert!(!fx_spring::fx_frame_due(3083), "链断信任期不产");
    assert!(fx_spring::fx_frame_due(3084), "链断 33ms 后看门狗放帧");
    seam::release_ai_panel_offset_y();
    vsync_book::disarm();
    vsync_book::reset_for_test();
}

// BAR-082 钉（2026-09-11 栈叠收回「无动画直接消失」案）：产线开局 =
// 脏帧先画（tap→poll→dirty 直画，不经 fx_frame_due）→ LAST_FRAME=0；
// 武装后首跳丢失 → 旧版参考点 last_due×LAST_FRAME 皆 0 → ref==0 永久
// 信任 = 动画冻结，用户读作「一松手直接消失」（panel-anim 实锤：2 帧 /
// swap 88-175ms / vsync 样本 1）。修法：武装戳入参考点三元组——零跳
// 零帧也以开表时刻为基线，33ms 后看门狗照样放帧。
// 变异抽检：arm_ms 从参考点摘除（退回二元 max），5033 臂当场红。
#[test]
fn spec_bar082_锁相_零基线首跳丢失看门狗() {
    let _g = VSYNC_TEST_LOCK.lock().unwrap();
    vsync_book::reset_for_test();
    vsync_book::disarm();
    fx_spring::reset_frame_clock_for_test(); // LAST_FRAME=0 = 产线脏帧开局
    seam::occupy_ai_panel_offset_y(fx_spring::spring_occupier());
    assert_eq!(seam::sample_ai_panel_offset_y(-100.0, 5000), -100.0); // primed
    seam::sample_ai_panel_offset_y(0.0, 5000); // 目标翻转 = 动画开始（脏帧直画）
    // 武装开表于 5000，零跳零 fx 帧（产线病况复刻）
    vsync_book::reset_run(5000);
    vsync_book::arm();
    assert!(!fx_spring::fx_frame_due(5032), "信任期内无跳不产");
    assert!(
        fx_spring::fx_frame_due(5033),
        "零基线（LAST_FRAME=0+零跳）33ms 后看门狗必须照样放帧"
    );
    // 放帧后链死期按预算节流续走（动画不卡死不跳变）
    assert!(!fx_spring::fx_frame_due(5040), "看门狗后仍受预算节流");
    assert!(fx_spring::fx_frame_due(5049), "看门狗后预算到点续产");
    // 迟到的首跳到达 → 链复活立即重锁
    vsync_book::note_due(5060);
    assert!(fx_spring::fx_frame_due(5061), "迟到首跳到达即重锁产帧");
    assert!(!fx_spring::fx_frame_due(5062), "重锁后同跳不续产");
    seam::release_ai_panel_offset_y();
    vsync_book::disarm();
    vsync_book::reset_for_test();
}

// 逐帧环形账（2026-09-11 动画监控升级）：汇总行看不出掉档轮的相位
// 结构，环形账记每帧（相对ms, 帧耗时ms）。单题串行——FRAME_TRACE 是
// 进程级单例，并行进场会互相截胡（BAR-057 教训）。
#[test]
fn spec_逐帧环形账_容量驱逐渲染取清() {
    use kfm_na::vsync_book as vb;
    vb::trace_reset();
    // 基本记账与渲染格式：[+rel:ms ...]
    vb::trace_frame(0, 2_000);
    vb::trace_frame(16, 3_500); // 3500us → 3ms（整除截断）
    vb::trace_frame(33, 25_000);
    let t = vb::take_trace();
    assert_eq!(t, vec![(0, 2), (16, 3), (33, 25)]);
    assert_eq!(vb::render_trace(&t), "[+0:2 +16:3 +33:25]");
    // 取账即清——下一轮不得见旧帧（并账防丢的第二半）
    assert!(vb::take_trace().is_empty());
    // 空账渲染
    assert_eq!(vb::render_trace(&[]), "[]");
    // 容量封顶驱逐最老：灌 CAP+10 帧，剩 CAP 帧且首帧是被挤后的第 10 帧
    vb::trace_reset();
    for i in 0..(vb::FRAME_TRACE_CAP + 10) {
        vb::trace_frame(i as u32, 1_000);
    }
    let t = vb::take_trace();
    assert_eq!(t.len(), vb::FRAME_TRACE_CAP);
    assert_eq!(t[0].0, 10, "最老的 10 帧必须被挤掉");
    assert_eq!(t.last().unwrap().0, (vb::FRAME_TRACE_CAP + 9) as u32);
}
