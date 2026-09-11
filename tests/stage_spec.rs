//! stage_spec.rs — 图层合成器状态机考题（ui-base §七 渲染成本模型）。
//! DirtyGuard 主考题在 src/ui/stage.rs 内联（同文件同改同跑）；本卷钉
//! 跨模块契约与 BAR 回归钉。

use kfm_na::ai_presence::Panel;
use kfm_na::ui::stage;

// BAR-070：上层槽恒可见——图层化首版漏设 Over.visible（默认 false），
// 输入栏/光球/放大镜集体隐身（2026-09-07 用户实看）。可见性槽位单源，
// 上层那条翻 false 即本考题红。2026-09-11 三公民第五槽：被覆盖面板仍
// 可见（placement 不动，遮盖撤走零动画露出）；同日第六槽终端卡片壳：
// 与键行同规跟 grid_keybar 走（基座壳恒靠泊，面板靠泊即整页盖住）。
// 变异抽检：返回数组第 5/6 槽改 false → 断言红；槽位次序换序即红。
#[test]
fn spec_bar070_上层槽恒可见() {
    // 槽序：[键行, AI, 配置, 文件树, 上层, 终端卡]
    assert_eq!(
        stage::slot_visibility(true, true, true, true),
        [true, true, true, true, true, true]
    );
    assert_eq!(
        stage::slot_visibility(false, false, false, false),
        [false, false, false, false, true, false]
    );
    // 显式锁六条语义：键行/终端卡跟网格未靠泊走，三面板各跟各的 visible 走
    let v = stage::slot_visibility(false, true, true, false);
    assert!(!v[0] && v[1] && v[2] && !v[3] && v[4] && !v[5]);
    // 被覆盖的面板：在栈（visible=true）哪怕顶是别家——露出零动画的承载
    let v = stage::slot_visibility(true, true, true, true);
    assert!(v[2] && v[3]);
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

// BAR-083/三公民泛化：z 序「动者在上，不动者按栈序」——旧规逐帧跟栈顶，
// 撤 AI 瞬 AI 出栈、不透明配置页当场压顶，AI 退出动画在它背后播完 =
// 用户见瞬消（2026-09-11 用户实机：先配置后 AI，撤 AI 无动画；反向撤
// 配置有）。三公民版：多动者之间仍按栈序，不在栈者垫最底按声明序。
// 变异抽检：活性权重删了（排序键只剩栈位阶）→ 第 1/3 条红；
// 动者间 tie-break 改反 → 第 5 条红。
#[test]
fn spec_bar083_z序_动者在上_三公民() {
    use kfm_na::ui::stage::panel_z_order as z;
    // 1. 撤 AI（AI 缝活跃、配置静止在栈）：哪怕栈顶已翻成配置，AI 仍压顶
    //    active 与 PANELS=[Ai,Config,FileTree] 对齐
    assert_eq!(
        z(&[Panel::Config, Panel::Ai], [true, false, false]),
        [Panel::FileTree, Panel::Config, Panel::Ai]
    );
    // 2. 撤配置（配置缝活跃、AI 静止在栈顶）：配置在上滑出可见
    assert_eq!(
        z(&[Panel::Config, Panel::Ai], [false, true, false]),
        [Panel::FileTree, Panel::Ai, Panel::Config]
    );
    // 3. 文件树家镜像：撤文件树（缝活跃）时它压过栈顶 AI
    assert_eq!(
        z(&[Panel::FileTree, Panel::Ai], [false, false, true]),
        [Panel::Config, Panel::Ai, Panel::FileTree]
    );
    // 4. 双静止纯栈序：底→顶原样（露出零动画的承载）
    assert_eq!(
        z(&[Panel::Config, Panel::Ai], [false, false, false]),
        [Panel::FileTree, Panel::Config, Panel::Ai]
    );
    // 5. 多动者按栈序：AI 撤+配置撤同帧（双活跃），栈序 Config<Ai 不变
    assert_eq!(
        z(&[Panel::Config, Panel::Ai], [true, true, false]),
        [Panel::FileTree, Panel::Config, Panel::Ai]
    );
    // 6. 不在栈者垫最底且次序确定（按 PANELS 声明序 Ai<Config<FileTree）
    assert_eq!(
        z(&[Panel::Ai], [false, false, false]),
        [Panel::Config, Panel::FileTree, Panel::Ai]
    );
}

// BAR-084：z 序活性泄漏进 presence→target 回路——BAR-083 引入的活性读数
// 被一并喂给「在不在场」判定 → 退场面板 target 翻回靠泊 → 采样器掉头
// 回粘，栈空但配置页视觉满屏停住（2026-09-11 redroid 实锤：stats
// top=none、双截图 diff=0 静止靠泊）。修法单源 panel_target_and_draw：
// **target 只问栈，draw 才看活性**。
// 变异抽检：target 分支改成 `in_stack || active` → 第 1 条红（回粘复活）；
// draw 分支删掉 active → 第 2 条红（退场动画瞬消）。
#[test]
fn spec_bar084_target只问栈_draw才看活性() {
    use kfm_na::ui::stage::panel_target_and_draw as td;
    // 1. 回归案例本体：不在栈但缝活跃（退场中）→ target=屏外，draw=true
    assert_eq!(td(false, true, 720.0), (720.0, true));
    // 2. 在栈且活跃（拖拽/入场）→ 靠泊+画
    assert_eq!(td(true, true, 720.0), (0.0, true));
    // 3. 在栈静止 → 靠泊+画（露出零动画的承载）
    assert_eq!(td(true, false, 720.0), (0.0, true));
    // 4. 不在栈不活跃 → 屏外+不画（彻底离场）
    assert_eq!(td(false, false, 720.0), (720.0, false));
    // 5. 符号忠实：文件树家屏外是 -w，target 原样带符号（不取绝对值）
    assert_eq!(td(false, true, -720.0), (-720.0, true));
}
