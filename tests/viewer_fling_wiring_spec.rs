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
    // ⑥ 滚动上限按实时屏尺寸现算现喂（2026-09-27 BAR-169 换芯：md
    // 排版 total_h 直喂版 viewer_scroll_max_h——旧字段版随 md 渲染器
    // 退役，涂装裁剪/滚动上限/钳制同尺纪律不变）
    assert!(
        APP.matches("md::viewer_scroll_max_h(sw, sh, lay.total_h)")
            .count()
            >= 2,
        "拖动臂与甩尾泵都必须现算 viewer_scroll_max_h（同一份几何）"
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

// ---- BAR-169 md 渲染器接线守卫（源码钉，同律：壳断了宿主全绿也照烂）----

const TV: &str = include_str!("../src/termview.rs");

#[test]
fn spec_bar169_md_渲染接线守卫() {
    // ① 查看器涂装换芯：paint_viewer_card 走 md 排版（layout_md）+
    // md_style 全局口 + 内容高直喂卡几何
    assert!(
        TV.contains(
            "crate::ui::md_layout::layout_md(&v.content, md::viewer_content_w(w), &style, self)"
        ),
        "查看器涂装必须吃 md 排版（layout_md + viewer_content_w 同尺）"
    );
    assert!(
        TV.contains("md::viewer_card_rect_h(w, h, lay.total_h)"),
        "卡高必须吃 md 排版 total_h（内容高直喂版）"
    );
    // ② 壳三处接线全走内容高直喂版（拖动/甩尾滚 max + 抬手命中卡几何）
    assert!(
        APP.matches("viewer_scroll_max_h(sw, sh, lay.total_h)")
            .count()
            >= 2,
        "拖动与甩尾两臂的滚动上限都必须走 md total_h"
    );
    assert!(
        APP.contains("md::viewer_card_rect_h(sw, sh, lay.total_h)"),
        "抬手命中卡几何必须走 md total_h"
    );
    // ③ 旧字段版不再被壳调用（残留 = 两份几何账分叉）
    assert!(
        !APP.contains("md::viewer_fields("),
        "壳不许再调旧字段版 viewer_fields（分叉账）"
    );
    // ④ VeilSig 补维：滚动 + md 样式（漏维 = 滚动/换样式不重烘鬼影）
    assert!(
        APP.contains("viewer_scroll: cs.viewer.as_ref().map_or(0, |v| v.scroll)"),
        "VeilSig 必须含 scroll 维（GLES 烘焙滚动重烘）"
    );
    assert!(APP.contains("md_style: ("), "VeilSig 必须含 md 样式维");
    // ⑤ 量宽同尺：壳排版走 viewer_md_layout（term 真字尺 = 涂装同一把
    // MdMeasure），三处接线同读这一个 helper
    assert!(
        APP.matches("self.viewer_md_layout(").count() >= 3,
        "三处接线必须同读 viewer_md_layout（量宽单源）"
    );
    // ⑥ 渲染设置卡：render.json 写盘 + 全局样式口灌（两旋钮同路）
    assert!(
        APP.contains("crate::settings::render_to_json(&self.render_cfg)"),
        "渲染设置必须写盘 render.json"
    );
    assert!(
        APP.matches("crate::ui::md_layout::set_md_style(").count() >= 2,
        "启动与换选两处都必须灌全局样式口"
    );
}
