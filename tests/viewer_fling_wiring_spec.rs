//! tests/viewer_fling_wiring_spec.rs — BAR-167 ① 查看器滚动接线守卫
//! （源码钉，仿 ftree_wiring_spec 症 6 同款）：android_app 是
//! `#[cfg(target_os = "android")]` 宿主编不到，壳接线断了宿主测试
//! 全绿也照烂——故用 include_str! 把关键接线字面量钉死。惯性甩尾 =
//! scroll.rs Fling 同一物理机（用户工单原话「不许手写增量」），涂装
//! 裁剪/滚动几何钉在 termview_spec/modal_spec，本册只管壳接线。

/// 壳源（android-only，include_str 读字面）
const APP: &str = include_str!("../src/android_app.rs");
const FX: &str = include_str!("../src/ui/fx_spring.rs");

#[test]
fn spec_bar167_viewer_滚动接线守卫() {
    // ① 滚动状态住核心（眼手同尺单源）：涂装/命中/壳手势同读
    // cfg_page viewer 态的 scroll 维，壳不许另起第二份账
    assert!(
        APP.contains("pg.scroll_viewer_by("),
        "壳滚动必须走 cfg_page::scroll_viewer_by（眼手同尺单源）"
    );
    // ② 接力件建机（两处起手路由：配置页 / 文件树页同族仲裁）
    assert!(
        APP.matches("self.modal_scroll = Some(crate::scroll::TouchScroll::new(")
            .count()
            >= 2,
        "两处跳框起手路由都必须建滚动接力件（配置页/文件树页）"
    );
    // ③ 拖动逐事件喂 moved_px_at（惯性速度采样唯一入口——喂无时钟的
    // moved_px = 抬手速度恒 0，甩尾哑掉）
    assert!(
        APP.contains("ms.moved_px_at(y, crate::report::boot_ms() as f64)"),
        "拖动必须喂 moved_px_at（速度采样唯一入口）"
    );
    // ④ 抬手交接甩尾 = scroll.rs Fling 物理机（用户工单「不许手写增量」）
    assert!(
        APP.contains(".and_then(|ms| ms.fling_on_release())"),
        "抬手必须从接力件交接 fling_on_release（Fling 物理机）"
    );
    // ⑤ 帧泵推进（真实间隔折帧）+ 燃尽报账 + 清点（起手取消/燃尽离场）
    assert!(
        APP.contains("self.viewer_fling.as_mut().and_then(|f| f.step(dt as f64))"),
        "帧泵必须逐圈推进 Fling::step"
    );
    assert!(
        APP.contains("查看器甩尾尽: {why}"),
        "燃尽必须报账（速度燃尽/触底/顶/车道翻牌）"
    );
    assert!(
        APP.matches("self.viewer_fling = None;").count() >= 2,
        "清点两处：新触摸落地取消 + 帧泵燃尽离场"
    );
    // ⑥ 滚动上限按实时屏尺寸现算现喂（modal::viewer_scroll_max 同一份
    // 几何——涂装裁剪/滚动上限/钳制同尺）
    assert!(
        APP.matches("md::viewer_scroll_max(sw, sh, &fields)")
            .count()
            >= 2,
        "拖动臂与甩尾泵都必须现算 viewer_scroll_max（同一份几何）"
    );
    // ⑦ 活性探针第八路（页停住甩尾在飞 = 不入表即甩尾零帧）
    assert!(
        FX.contains("pub fn viewer_fling_live()")
            && FX.contains("let viewer_fling = viewer_fling_live();"),
        "查看器甩尾活性旗必须在"
    );
    assert!(
        FX.contains("|| viewer_fling"),
        "查看器甩尾活性必须接进 fx_frame_due 的 active 表"
    );
    assert!(
        APP.contains("crate::ui::fx_spring::note_viewer_fling_live(self.viewer_fling.is_some())"),
        "壳每圈帧泵末尾必须同步活性旗"
    );
    // 活性旗的真行为（不是 grep）：默认假、置位读得回、清位读得回
    use kfm_na::ui::fx_spring::{note_viewer_fling_live, viewer_fling_live};
    assert!(!viewer_fling_live(), "默认该是假（无甩尾不产帧）");
    note_viewer_fling_live(true);
    assert!(viewer_fling_live());
    note_viewer_fling_live(false);
    assert!(!viewer_fling_live());
}
