//! pan_capture_spec.rs — PanOld 捕获源裁决考题（BAR-100，2026-09-16）
//!
//! 背景：平移升合成期后，PanOld 捕获从「26MB 画布拷贝+13MB 重传」改
//! 纹理互换零拷贝。捕获源判错 = 旧代封存到残影/空图（连环平移时上屏
//! 旧代直接错图）——裁决逻辑抽纯函数钉死：
//!
//! - 上一笔是 Upper 域平移：配置槽是 hold 烘焙（无上池行），旧代的
//!   行在上一笔 PanMove 里 → 捕获源必须接力 PanMove
//! - 首笔（无上一笔）或上一笔是 Page 域：配置槽纹理即旧代（Page 域
//!   新代与配置槽同图，PanMove 整页不画）→ 捕获源必须是配置槽
//!
//! 变异抽检：if 两臂对调（prev_upper 给到 Config）两题全咬；常量槽
//! 对调（PanMove↔Config）两题全咬。

use kfm_na::ui::cfg_page::{PanCaptureSrc, pan_capture_src};

#[test]
fn bar100_capture_src_首笔或上笔page捕配置槽() {
    assert_eq!(pan_capture_src(false), PanCaptureSrc::Config);
}

#[test]
fn bar100_capture_src_上笔upper接力panmove() {
    assert_eq!(pan_capture_src(true), PanCaptureSrc::PanMove);
}
