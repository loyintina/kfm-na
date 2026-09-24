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

/// BAR-149 源码守卫（2026-09-24 redroid 判卷定罪：武装态不进解析槽
/// 烘焙 sig = 钮面「再点确认」永不重烘的死钮面——用户见字没变再点 =
/// 意外重启）：①sig 必须含武装态维（漏维 = 武装不重烘）；②事件环
/// 必须有翻相泵（armed 是时间函数无事件驱动，漏泵 = 落回后钮面卡死）。
/// 两处都是「漏了就静默」的装配点，纯函数考题管不到，源码守卫钉死
#[test]
fn spec_bar149_武装态进烘焙sig_翻相泵在环() {
    let src = include_str!("../src/android_app.rs");
    assert!(
        src.contains("let restart_armed_now = crate::self_restart::restart_armed("),
        "解析槽烘焙 sig 的武装态维必须真从 restart_armed( 喂（BAR-149：恒值/漏维 = 死钮面）"
    );
    assert!(
        src.contains("if restart_armed != self.restart_armed_last {"),
        "事件环必须有武装态翻相泵（BAR-149：armed 是时间函数无事件驱动，漏泵 = 落回后钮面卡死）"
    );
}
