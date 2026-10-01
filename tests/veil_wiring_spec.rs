//! tests/veil_wiring_spec.rs — BAR-212 翻案（NA0122）信箱列表卡 veil
//! 烘焙门接线守卫（源码钉，仿 viewer_fling_wiring_spec 形制）：
//! 解析页从终端直滑时配置页不在栈里（cfg_snap 恒 None），旧门
//! `let (Some(cs), Some(t)) = (cfg_snap, th)` 把整条烘焙臂掐死——
//! 列表卡状态开了但零像素，命中带铺满全屏 = 隐形输入陷阱。android_app
//! 是 `#[cfg(target_os = "android")]` 宿主编不到，壳接线断了宿主测试
//! 全绿也照烂——故用 include_str! 把门面字面钉死。
//!
//! 变异抽检：①门改回要求 cfg_snap（`let (Some(cs), Some(t))`）必须咬
//! ——即本钉 ② 的反钉；②摘掉 `CfgPage::viewer_snap_global` 兜底
//! （cfg_snap=None 时查看器内容键/滚动/涂装参全断）必须咬。两咬全中
//! （cp 备份复原，2026-10-01）。

/// 壳源（android-only，include_str 读字面）
const APP: &str = include_str!("../src/android_app.rs");

#[test]
fn spec_bar212_veil烘焙门不要cfg_snap接线守卫() {
    // ① 烘焙门只挂 th（NA0122 定罪：cfg_snap 要求 = 解析页直滑恒不烘）
    assert!(
        APP.contains("if veil_on && let Some(t) = th"),
        "veil 烘焙门必须只要求 th（解析页直滑 cfg_snap 恒 None，要求它 = 列表卡零像素全屏隐形输入陷阱，NA0122）"
    );
    // ② 旧门字面零残留（反钉：回潮即翻红）
    assert!(
        !APP.contains("let (Some(cs), Some(t)) = (cfg_snap, th)"),
        "旧门 let (Some(cs), Some(t)) = (cfg_snap, th) 不许回潮"
    );
    // ③ cfg_snap=None 时 viewer 走全局口兜底（查看器状态住 cfg_page
    // 全局，列表卡点信弹的查看器在解析页直滑下照样要画）
    assert!(
        APP.contains(".or_else(crate::ui::cfg_page::CfgPage::viewer_snap_global)"),
        "viewer 快照必须有 viewer_snap_global 全局口兜底"
    );
    // ④ veil_on 的解析页臂不丢（pt_visible && 列表卡开/查看器开）
    assert!(
        APP.contains("|| (pt_visible && (mail_list_open || viewer_open_now))"),
        "veil_on 解析页臂（pt_visible && (mail_list_open || viewer_open_now)）必须在"
    );
}
