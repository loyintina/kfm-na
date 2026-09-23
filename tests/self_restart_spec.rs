//! self_restart 考题（A 档）：两段确认裁决 tap_verdict / 武装判定
//! armed_at——纯函数先行钉死；JNI 预约杀进程是 B 档胶水（系统让不
//! 让活，无输入输出可判卷）。
//!
//! 变异抽检方向：①Arm/Execute 颠倒（一击就重启 = 误触防全废）；
//! ②武装窗判据改 <=（边界时刻该落回却放行）；③Execute 后账不清零
//! （重启失败后永远一击即杀）。本卷必须全咬。

use kfm_na::self_restart::{ARM_WINDOW_MS, TapVerdict, armed_at, tap_verdict};

#[test]
fn spec_两段确认_完整舞步() {
    // 一击：未武装 → Arm，账 = now + 窗
    let (book, v) = tap_verdict(0, 1000);
    assert_eq!(v, TapVerdict::Arm);
    assert_eq!(book, 1000 + ARM_WINDOW_MS);
    // 窗内二击 → Execute，账清零（变异③：不清零必须咬）
    let (book2, v2) = tap_verdict(book, 1000 + ARM_WINDOW_MS - 1);
    assert_eq!(v2, TapVerdict::Execute);
    assert_eq!(book2, 0);
    // 过窗再点 = 重新武装（不是执行）
    let (_, v3) = tap_verdict(book, 1000 + ARM_WINDOW_MS + 1);
    assert_eq!(v3, TapVerdict::Arm, "过窗必须落回未武装");
}

#[test]
fn spec_武装判定_边界() {
    assert!(!armed_at(0, 0), "0 = 未武装（不是「武装到 epoch」）");
    assert!(armed_at(5000, 4999));
    // 变异②：边界 = 落回（开区间；<= 变异必须咬）
    assert!(!armed_at(5000, 5000), "边界时刻该落回（开区间）");
    assert!(!armed_at(5000, 5001));
}
