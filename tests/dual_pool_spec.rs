//! dual_pool_spec.rs — 双池骨架考题（主题宪法 §五，A 档）。
//!
//! 钉什么：动态高度数学（上池 min(内容, H/2) / 下池撑满 / 空占位 4 格）+
//! 可用区几何（左右各 2 格内边距、底内缘含 bottom_inset——漏算 = 下池
//! 顶穿页环，2026-09-12 真机实踩）+ 两池 1 格间距 + D9 共享句柄。
//! 涂装侧的反转渐变/环墨钉在 termview_spec。

use kfm_na::termview::{AI_PAGE_FRAME_MARGIN, AI_PAGE_FRAME_W, CELL_H, CELL_W};
use kfm_na::ui::dual_pool::{
    DualPool, POOL_EMPTY_H, POOL_GAP, POOL_SIDE_PAD, POOL_TOP_GAP, dual_pool_handle, pool_area,
    register_dual_pool,
};
use kfm_na::ui::tab_bar::TAB_ROW_H;

/// 钉①：可用区几何钉——首行之下 1 格间距起；左右各让 2 格内边距
/// （池是卡片不是贴缘内容带）；底内缘 = 页环底内缘（含 bottom_inset）
#[test]
fn spec_dual_pool_可用区几何钉() {
    let a = pool_area(720, 1280, 0);
    assert_eq!(
        a.x,
        (AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W * 3 + POOL_SIDE_PAD) as i64,
        "池区 x = 粗环内缘 + 2 格内边距（2026-09-12 实测：1 格太窄）"
    );
    assert_eq!(
        a.y,
        55 + TAB_ROW_H as i64 + POOL_TOP_GAP as i64,
        "池区顶 = 内容原点 y + 标签行 + 1 格标准间距"
    );
    assert_eq!(
        a.w,
        720 - (AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W * 3 + POOL_SIDE_PAD)
            - (AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + POOL_SIDE_PAD),
        "池区宽 = 屏宽 − 左右环内缘 − 左右各 2 格"
    );
    assert_eq!(
        a.h,
        1280 - (AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_H) - a.y as u32,
        "池区底 = 屏底 − (MARGIN + 细缘 + 1 格)"
    );
    // bottom_inset 钉：键盘+输入栏带必须啃掉池区底（顶穿回归）
    let ai = pool_area(720, 1280, 200);
    assert_eq!(ai.h, a.h - 200, "底内缘随 bottom_inset 上移（顶穿钉）");
}

/// 钉②：常量钉——间距/内边距/空占位（2026-09-12 实测二标值）
#[test]
fn spec_dual_pool_常量钉() {
    assert_eq!(POOL_TOP_GAP, CELL_H, "首行 → 上池标准间距 = 1 格");
    assert_eq!(POOL_GAP, CELL_H, "两池间距 = 1 格（0 格太密修订）");
    assert_eq!(POOL_SIDE_PAD, CELL_W * 2, "池区左右内边距各 = 2 格");
    assert_eq!(
        POOL_EMPTY_H,
        CELL_H * 4,
        "上池空占位 = 4 格（2 格太矮二标）"
    );
}

/// 钉③：空占位钉（A2 拍板）——上池无内容也占位 4 格不消失，下池撑满
#[test]
fn spec_dual_pool_空占位钉() {
    let pool = DualPool::new(720, 1280);
    let s = pool.layout();
    assert_eq!(s.upper.h, POOL_EMPTY_H, "空内容上池 = 占位高（不消失）");
    assert!(!s.upper_scroll, "空内容无滚动");
    assert_eq!(
        s.lower.y,
        s.upper.y + (s.upper.h + POOL_GAP) as i64,
        "两池留 1 格间距"
    );
    assert_eq!(
        s.upper.h + POOL_GAP + s.lower.h,
        pool.area().h,
        "下池撑满剩余（恒到卡底）"
    );
}

/// 钉④：内容跟随钉——内容几行跟几行（不足占位高按占位抬）
#[test]
fn spec_dual_pool_内容跟随钉() {
    let mut pool = DualPool::new(720, 1280);
    pool.set_upper_content_h(216); // 6 行
    let s = pool.layout();
    assert_eq!(s.upper.h, 216, "内容 < H/2：上池跟随内容高");
    assert!(!s.upper_scroll);
    assert_eq!(
        s.lower.h,
        pool.area().h - 216 - POOL_GAP,
        "下池 = H − 上池 − 间距"
    );

    pool.set_upper_content_h(10); // 不足占位高
    let s2 = pool.layout();
    assert_eq!(s2.upper.h, POOL_EMPTY_H, "内容不足占位按占位抬（A2）");
    assert!(!s2.upper_scroll, "10px 内容在占位框内无需滚动");
    assert_eq!(pool.upper_content_h(), 10, "内容高读回（状态咬口）");
}

/// 钉⑤：半高钳钉——内容多不过下池（上池 ≤ H/2），超出报滚动旗
#[test]
fn spec_dual_pool_半高钳钉() {
    let mut pool = DualPool::new(720, 1280);
    let half = pool.area().h / 2;
    pool.set_upper_content_h(100_000);
    let s = pool.layout();
    assert_eq!(s.upper.h, half, "内容爆量：上池钳到 H/2（不越下池）");
    assert!(s.upper_scroll, "被截断必须报滚动旗");
    assert_eq!(
        s.lower.h,
        pool.area().h - half - POOL_GAP,
        "两池都多 = 上池半高、下池撑满"
    );

    pool.set_upper_content_h(half); // 恰好半高
    let s2 = pool.layout();
    assert_eq!(s2.upper.h, half);
    assert!(!s2.upper_scroll, "恰好装满不滚（> 才算截断）");
}

/// 钉⑥：视口重算钉——set_viewport 后可用区/布局全按新屏尺寸+inset
#[test]
fn spec_dual_pool_视口重算钉() {
    let mut pool = DualPool::new(720, 1280);
    pool.set_viewport(400, 800, 100);
    let a = pool_area(400, 800, 100);
    assert_eq!(pool.area(), &a, "视口重算 = pool_area 同源");
    assert_eq!(a.w, 400 - 61 - 55, "左右各 2 格内边距后净宽");
    assert_eq!(
        a.h,
        800 - 100 - 55 - (55 + TAB_ROW_H + POOL_TOP_GAP),
        "底内缘含 inset"
    );
    let s = pool.layout();
    assert_eq!(s.upper.h + POOL_GAP + s.lower.h, a.h, "新视口下下池仍撑满");
}

/// 钉⑦：共享句柄钉（D9 同源）——注册后 gate 侧拿得到同一份
#[test]
fn spec_dual_pool_共享句柄钉() {
    use std::sync::{Arc, Mutex};
    let pool = Arc::new(Mutex::new(DualPool::new(720, 1280)));
    register_dual_pool(pool.clone());
    let h = dual_pool_handle().expect("注册后句柄必须在");
    // h 与 pool 是同一个 Arc<Mutex>：两把锁分时取，同表达式双锁 = 死锁
    let h_area = h.lock().unwrap().area().clone();
    let pool_area_owned = pool.lock().unwrap().area().clone();
    assert_eq!(h_area, pool_area_owned, "句柄与注册同一份");
}
