//! insets_spec.rs — 键盘 inset 轮询裁决考题（A 档纯逻辑）
//!
//! BAR-112（2026-09-19 用户真机报「视口只有半屏，重排都没有用」）：
//! 挂起态轮询把 inset 新值记了账却跳过重算（apply_window_size 要窗口），
//! 回前台后新旧值相等再也检测不到变化 → kb_shift 卡死在键盘弹起态 = 半屏。
//! 裁决 = 窗口死了不记账，留旧值待回前台同差再判。
//!
//! 变异抽检：裁决退化为「值变即记账」（不看窗口）本文件必须红。
//! 答案 src/insets.rs on_inset_poll。

use kfm_na::insets::on_inset_poll;

#[test]
fn spec_bar112_轮询裁决_窗口死了不记账() {
    // 键盘收起（702→0）但窗口已弃（挂起态）：不许记账——
    // 留旧值 702，回前台后同差再判才带重算
    assert_eq!(on_inset_poll(0, 702, false), None);
    // 反向同理：键盘弹起（0→702）但窗口已弃，同样不许记账
    assert_eq!(on_inset_poll(702, 0, false), None);
    // 同值不抖（轮询 100ms 一遍，同值不假报不白重算）
    assert_eq!(on_inset_poll(702, 702, true), None);
    assert_eq!(on_inset_poll(0, 0, true), None);
    // 值变 + 窗口活着 = 记账重算（正路，键盘开合的正常周期）
    assert_eq!(on_inset_poll(0, 702, true), Some(0));
    assert_eq!(on_inset_poll(702, 0, true), Some(702));
}
