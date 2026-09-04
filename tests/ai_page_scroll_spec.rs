//! ai_page_scroll_spec.rs — AI 对话页视口状态机考卷（期 0④，A 档）。
//! 判卷对象：src/ui/ai_page.rs AiPageScroll（纯逻辑）。
//!
//! 变异抽检预期：
//! - follow_tail 恒 true 不改 → 「上滑取消追底」「取消后新内容不抢视口」红；
//! - drag_rows 去掉 0 下钳 → 「上滑过量钳在顶部」红（offset 负值绕行）；
//! - drag_rows 去掉 max 上钳 → 「钳制上界=总行-一屏」红；
//! - sync_layout 去掉 follow 贴底 → 「追底态新内容自动贴底」红。

use kfm_na::ui::ai_page::AiPageScroll;

/// 辅助：铺开 total=100 fit=10 的布局（上界 max=90）
fn laid_out() -> AiPageScroll {
    let mut s = AiPageScroll::new();
    s.sync_layout(100, 10);
    s
}

#[test]
fn spec_默认追底_offset恒零() {
    let s = laid_out();
    assert!(s.follow(), "出厂必须是追底态");
    assert_eq!(s.offset(), 0);
}

#[test]
fn spec_上滑看更早_取消追底() {
    let mut s = laid_out();
    s.drag_rows(5); // 正 = 看更早（手势侧已换算好方向）
    assert_eq!(s.offset(), 5);
    assert!(!s.follow(), "上滑看历史即取消追底");
}

#[test]
fn spec_下滑回底_恢复追底() {
    let mut s = laid_out();
    s.drag_rows(5);
    s.drag_rows(-3);
    assert!(!s.follow(), "没到底不恢复");
    s.drag_rows(-2); // 正好回 0
    assert_eq!(s.offset(), 0);
    assert!(s.follow(), "滑回最底 = 恢复追底");
}

#[test]
fn spec_下滑过量_钳在底不负债() {
    let mut s = laid_out();
    s.drag_rows(3);
    s.drag_rows(-100); // 远超剩余
    assert_eq!(s.offset(), 0, "offset 不许变负（底之下没有内容）");
    assert!(s.follow());
}

#[test]
fn spec_上滑过量_钳在总行减一屏() {
    let mut s = laid_out();
    s.drag_rows(10_000);
    assert_eq!(s.offset(), 90, "上界 = total(100) - fit(10) = 90");
}

#[test]
fn spec_追底态_新内容自动贴底() {
    let mut s = laid_out();
    // AI 流式输出：布局膨胀 → 写回 → 追底态 offset 恒 0
    s.sync_layout(130, 10);
    assert_eq!(s.offset(), 0);
    assert!(s.follow());
}

#[test]
fn spec_取消追底后_新内容不抢视口() {
    let mut s = laid_out();
    s.drag_rows(20);
    s.sync_layout(130, 10); // 新内容来了：上界涨到 120，offset 不动
    assert_eq!(s.offset(), 20, "看历史时新消息不许把视口顶走");
    assert!(!s.follow());
}

#[test]
fn spec_布局缩水_offset不悬空() {
    let mut s = laid_out();
    s.drag_rows(80);
    s.sync_layout(30, 10); // 内容缩水（如清屏重排）：上界 20
    assert_eq!(s.offset(), 20, "offset 不许悬在已不存在的行上");
}

#[test]
fn spec_内容不足一屏_永远贴底() {
    let mut s = AiPageScroll::new();
    s.sync_layout(5, 10); // total < fit：max=0
    s.drag_rows(10);
    assert_eq!(s.offset(), 0, "不满一屏没有可滚的");
    assert!(s.follow());
}

// ---- 思考块可见窗（期 0④½：≤3 行尾随自滚） ----

#[test]
fn spec_思考窗_不足三行全给() {
    use kfm_na::ui::ai_page::thinking_window;
    assert_eq!(thinking_window(0), 0..0);
    assert_eq!(thinking_window(1), 0..1);
    assert_eq!(thinking_window(3), 0..3);
}

#[test]
fn spec_思考窗_超三行只给尾三行() {
    use kfm_na::ui::ai_page::thinking_window;
    assert_eq!(thinking_window(4), 1..4, "窗恒 3 行且跟尾——头部丢");
    assert_eq!(thinking_window(10), 7..10);
    assert_eq!(thinking_window(999), 996..999);
}

#[test]
fn spec_bar064_手势方向_下滑看更早() {
    // BAR-064：AI 页滚动手势反了（2026-09-04 用户实看：手指上滑居然
    // 翻出更早的消息）。主流手感 = 内容跟手：下滑 dy>0 拉内容向下
    // 露出更早消息 = 正行增量；上滑看更新 = 负
    use kfm_na::ui::ai_page::drag_accum_rows;
    let lh = 64.0;
    let (_acc, rows) = drag_accum_rows(0.0, 100.0, lh); // 手指下滑 100px
    assert_eq!(rows, 1, "下滑 100px = 看更早 1 行");
    let (_acc, rows) = drag_accum_rows(0.0, -200.0, lh); // 手指上滑 200px
    assert_eq!(rows, -3, "上滑 200px = 看更新 3 行（trunc 去尾）");
}

#[test]
fn spec_bar064_像素累积_余数不丢() {
    // 像素级跟手：两次半行滑动必须各出 0 行后第三下凑满出行——
    // 行增量取整的余数留在累积里，不丢小数（丢了就发涩）
    use kfm_na::ui::ai_page::drag_accum_rows;
    let lh = 64.0;
    let (acc, rows) = drag_accum_rows(0.0, 40.0, lh);
    assert_eq!(rows, 0);
    let (acc, rows) = drag_accum_rows(acc, 40.0, lh);
    assert_eq!(rows, 1, "40+40=80 ≥ 64 出 1 行");
    assert!((acc - 16.0).abs() < 1e-9, "余数 16 必须留账");
    // 反向同理：负余数也留账
    let (acc, rows) = drag_accum_rows(0.0, -40.0, lh);
    assert_eq!(rows, 0);
    let (_acc, rows) = drag_accum_rows(acc, -40.0, lh);
    assert_eq!(rows, -1);
}
