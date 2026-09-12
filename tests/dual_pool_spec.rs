//! dual_pool_spec.rs — 双池骨架考题（主题宪法 §五，A 档）。
//!
//! 钉什么：动态高度数学（上池 min(内容, H/2) / 下池撑满 / 空占位）+
//! 可用区几何（与标签栏内容带同源、卡底内缘对称）+ 直接衔接 +
//! D9 共享句柄。涂装侧的反转渐变/环墨钉在 termview_spec。

use kfm_na::termview::{AI_PAGE_FRAME_MARGIN, AI_PAGE_FRAME_W, CELL_H};
use kfm_na::ui::dual_pool::{
    DualPool, POOL_EMPTY_H, POOL_TOP_GAP, dual_pool_handle, pool_area, register_dual_pool,
};
use kfm_na::ui::tab_bar::{TAB_ROW_H, content_viewport_w};

/// 钉①：可用区几何钉——首行之下 1 格间距起，卡底内缘止；x/w 与标签栏
/// 内容带同源（眼手同尺）
#[test]
fn spec_dual_pool_可用区几何钉() {
    let a = pool_area(720, 1280);
    assert_eq!(a.x, 43, "池区 x = 内容原点 x（与标签栏同源）");
    assert_eq!(
        a.y,
        55 + TAB_ROW_H as i64 + POOL_TOP_GAP as i64,
        "池区顶 = 内容原点 y + 标签行 + 1 格标准间距（§四 二层双框）"
    );
    assert_eq!(a.w, content_viewport_w(720), "池区宽 = 标签栏内容带宽");
    assert_eq!(
        a.h,
        1280u32 - (AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_H) - a.y as u32,
        "池区底 = 屏底 − (MARGIN + 细缘 + 1 格)（与原点 y=55 上下对称）"
    );
}

/// 钉②：常量钉——标准间距 1 格、空占位 2 格（§四/§五 池行双行同尺）
#[test]
fn spec_dual_pool_常量钉() {
    assert_eq!(POOL_TOP_GAP, CELL_H, "首行 → 上池标准间距 = 1 格");
    assert_eq!(POOL_EMPTY_H, CELL_H * 2, "上池空占位 = 2 格 = 一行池行高");
}

/// 钉③：空占位钉（A2 拍板）——上池无内容也占位 2 格不消失，下池撑满
#[test]
fn spec_dual_pool_空占位钉() {
    let pool = DualPool::new(720, 1280);
    let s = pool.layout();
    assert_eq!(s.upper.h, POOL_EMPTY_H, "空内容上池 = 占位高（不消失）");
    assert!(!s.upper_scroll, "空内容无滚动");
    assert_eq!(s.lower.y, s.upper.y + s.upper.h as i64, "两池直接衔接");
    assert_eq!(
        s.upper.h + s.lower.h,
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
    assert_eq!(s.lower.h, pool.area().h - 216, "下池 = H − 上池");

    pool.set_upper_content_h(10); // 不足占位高
    let s2 = pool.layout();
    assert_eq!(s2.upper.h, POOL_EMPTY_H, "内容不足占位按占位抬（A2）");
    assert!(!s2.upper_scroll, "10px 内容在 72px 框内无需滚动");
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
    assert_eq!(s.lower.h, pool.area().h - half, "两池都多 = 各半");

    pool.set_upper_content_h(half); // 恰好半高
    let s2 = pool.layout();
    assert_eq!(s2.upper.h, half);
    assert!(!s2.upper_scroll, "恰好装满不滚（> 才算截断）");
}

/// 钉⑥：视口重算钉——set_viewport 后可用区/布局全按新屏尺寸
#[test]
fn spec_dual_pool_视口重算钉() {
    let mut pool = DualPool::new(720, 1280);
    pool.set_viewport(400, 800);
    let a = pool_area(400, 800);
    assert_eq!(pool.area(), &a, "视口重算 = pool_area 同源");
    assert_eq!(a.w, 400 - 43 - 37);
    assert_eq!(a.h, 800 - 55 - (55 + TAB_ROW_H + POOL_TOP_GAP));
    let s = pool.layout();
    assert_eq!(s.upper.h + s.lower.h, a.h, "新视口下下池仍撑满");
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
