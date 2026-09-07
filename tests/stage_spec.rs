//! stage_spec.rs — 图层合成器状态机考题（ui-base §七 渲染成本模型）。
//! DirtyGuard 主考题在 src/ui/stage.rs 内联（同文件同改同跑）；本卷钉
//! 跨模块契约与 BAR 回归钉。

use kfm_na::ui::stage;

// BAR-070：上层槽恒可见——图层化首版漏设 Over.visible（默认 false），
// 输入栏/光球/放大镜集体隐身（2026-09-07 用户实看）。可见性三槽单源，
// 上层那条翻 false 即本考题红。
#[test]
fn spec_bar070_上层槽恒可见() {
    // 任何键行/面板组合下，上层槽都是 true
    assert_eq!(stage::slot_visibility(true, true), [true, true, true]);
    assert_eq!(stage::slot_visibility(false, false), [false, false, true]);
    assert_eq!(stage::slot_visibility(true, false), [true, false, true]);
    assert_eq!(stage::slot_visibility(false, true), [false, true, true]);
    // 显式锁两条语义：键行跟面板未靠泊走，面板跟 panel_visible 走
    let v = stage::slot_visibility(false, true);
    assert!(!v[0] && v[1]);
}

// DirtyGuard 复用契约跨卷再钉：同 sig 复喂=照用烘焙（动画帧零光栅
// 零上传的承载点），sig 变化=重烘焙。
#[test]
fn spec_置脏判定_同sig复用_变化重烘() {
    use kfm_na::ui::stage::DirtyGuard;
    let mut g: DirtyGuard<(u32, bool)> = DirtyGuard::new();
    assert!(g.feed((7, false))); // 冷启动必烘
    assert!(!g.feed((7, false))); // 动画帧照用
    assert!(g.feed((7, true))); // 内容变了必烘
    g.invalidate(); // GL 重建：烘焙物全死
    assert!(g.feed((7, true)));
}
