//! stage_spec.rs — 图层合成器状态机考题（ui-base §七 渲染成本模型）。
//! DirtyGuard 主考题在 src/ui/stage.rs 内联（同文件同改同跑）；本卷钉
//! 跨模块契约与 BAR 回归钉。

use kfm_na::ai_presence::Panel;
use kfm_na::ui::stage;

// BAR-070：上层槽恒可见——图层化首版漏设 Over.visible（默认 false），
// 输入栏/光球/放大镜集体隐身（2026-09-07 用户实看）。可见性槽位单源，
// 上层那条翻 false 即本考题红。2026-09-11 三公民第五槽：被覆盖面板仍
// 可见（placement 不动，遮盖撤走零动画露出）；同日第六槽终端卡片壳：
// 与键行同规跟 grid_keybar 走（基座壳恒靠泊，面板靠泊即整页盖住）；
// 2026-09-12 四公民解析页槽入列（右缘家，与配置同规）；2026-09-15
// 十九修 D8 第七槽平移旧代入列（平移升合成期：配置可见 ∧ 平移进行中）；
// 2026-09-27 五公民阅读页槽入列（BAR-170，index 9，右缘家同规）；
// 2026-09-30 BAR-207 Demo 打样页槽退役回收（原 index 9，槽序顺移）；
// 2026-10-01 六公民信箱页槽入列（BAR-214，index 10，右缘家同规）。
// 变异抽检：返回数组第 6/7 槽改 false → 断言红；槽位次序换序即红；
// 阅读槽漏追加/错位 → v[9] 断言红；信箱槽漏追加/错位 → v[10] 断言红。
#[test]
fn spec_bar070_上层槽恒可见() {
    // 槽序：[键行, AI, 配置, 文件树, 解析, 上层, 终端卡, 平移旧代, 平移新代, 阅读, 信箱]
    assert_eq!(
        stage::slot_visibility(true, true, true, true, true, false, false, false),
        [
            true, true, true, true, true, true, true, false, false, false, false
        ]
    );
    assert_eq!(
        stage::slot_visibility(false, false, false, false, false, true, false, false),
        [
            false, false, false, false, false, true, false, false, false, false, false
        ]
    );
    // 显式锁十一条语义：键行/终端卡跟网格未靠泊走，六面板各跟各的 visible 走
    let v = stage::slot_visibility(false, true, true, false, true, true, true, true);
    assert!(
        !v[0] && v[1] && v[2] && !v[3] && v[4] && v[5] && !v[6] && v[7] && v[8] && v[9] && v[10]
    );
    // 阅读页槽独立一票（BAR-170）：rd_visible 翻 false 只灭 index 9
    let v = stage::slot_visibility(true, true, true, true, true, true, false, true);
    assert!(v[8] && !v[9] && v[10]);
    // 信箱页槽独立一票（BAR-214）：mail_visible 翻 false 只灭 index 10
    let v = stage::slot_visibility(true, true, true, true, true, true, true, false);
    assert!(v[9] && !v[10]);
    // 被覆盖的面板：在栈（visible=true）哪怕顶是别家——露出随推移滑回的承载
    let v = stage::slot_visibility(true, true, true, true, true, true, true, true);
    assert!(v[2] && v[3] && v[4] && v[9] && v[10]);
    // 十九修 D8：平移双槽 = 配置可见 ∧ 平移进行中（贴死即隐/页隐即隐）
    assert!(!stage::slot_visibility(true, false, true, false, false, false, false, false)[7]);
    assert!(stage::slot_visibility(true, false, true, false, false, true, false, false)[7]);
    assert!(!stage::slot_visibility(true, false, false, false, false, true, false, false)[7]);
    assert!(stage::slot_visibility(true, false, true, false, false, true, false, false)[8]);
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

// BAR-083/六公民泛化：z 序「动者在上，不动者按栈序」——旧规逐帧跟栈顶，
// 撤 AI 瞬 AI 出栈、不透明配置页当场压顶，AI 退出动画在它背后播完 =
// 用户见瞬消（2026-09-11 用户实机：先配置后 AI，撤 AI 无动画；反向撤
// 配置有）。六公民版（2026-09-27 阅读页入列 BAR-170；2026-09-30 BAR-207
// Demo 退役；2026-10-01 信箱页入列 BAR-214，PANELS 声明序
// [Ai, Config, FileTree, Parser, Reader, Mail]）：
// 多动者之间仍按栈序，不在栈者垫最底按声明序。
// 变异抽检：活性权重删了（排序键只剩栈位阶）→ 第 1/3 条红；
// 动者间 tie-break 改反 → 第 5 条红；PANELS 漏 Parser → 第 7 条红；
// PANELS 漏 Reader → 第 8 条红；PANELS 漏 Mail → 第 9 条红。
#[test]
fn spec_bar083_z序_动者在上_六公民() {
    use kfm_na::ui::stage::panel_z_order as z;
    // 1. 撤 AI（AI 缝活跃、配置静止在栈）：哪怕栈顶已翻成配置，AI 仍压顶
    //    active 与 PANELS=[Ai,Config,FileTree,Parser,Reader,Mail] 对齐
    assert_eq!(
        z(
            &[Panel::Config, Panel::Ai],
            [true, false, false, false, false, false, false]
        ),
        [
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Config,
            Panel::Ai
        ]
    );
    // 2. 撤配置（配置缝活跃、AI 静止在栈顶）：配置在上滑出可见
    assert_eq!(
        z(
            &[Panel::Config, Panel::Ai],
            [false, true, false, false, false, false, false]
        ),
        [
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Ai,
            Panel::Config
        ]
    );
    // 3. 文件树家镜像：撤文件树（缝活跃）时它压过栈顶 AI
    assert_eq!(
        z(
            &[Panel::FileTree, Panel::Ai],
            [false, false, true, false, false, false, false]
        ),
        [
            Panel::Config,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Ai,
            Panel::FileTree
        ]
    );
    // 4. 双静止纯栈序：底→顶原样（露出随推移滑回的承载）
    assert_eq!(
        z(
            &[Panel::Config, Panel::Ai],
            [false, false, false, false, false, false, false]
        ),
        [
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Config,
            Panel::Ai
        ]
    );
    // 5. 多动者按栈序：AI 撤+配置撤同帧（双活跃），栈序 Config<Ai 不变
    assert_eq!(
        z(
            &[Panel::Config, Panel::Ai],
            [true, true, false, false, false, false, false]
        ),
        [
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Config,
            Panel::Ai
        ]
    );
    // 6. 不在栈者垫最底且次序确定（按 PANELS 声明序 Ai<Config<FileTree<Parser<Reader<Mail）
    assert_eq!(
        z(
            &[Panel::Ai],
            [false, false, false, false, false, false, false]
        ),
        [
            Panel::Config,
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Ai
        ]
    );
    // 7. 解析页入题：撤解析页（缝活跃）压过栈顶文件树；解析在栈被压时
    //    双静止跟栈序
    assert_eq!(
        z(
            &[Panel::FileTree, Panel::Parser],
            [false, false, false, true, false, false, false]
        ),
        [
            Panel::Ai,
            Panel::Config,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::FileTree,
            Panel::Parser
        ]
    );
    assert_eq!(
        z(
            &[Panel::Parser, Panel::FileTree],
            [false, false, true, false, false, false, false]
        ),
        [
            Panel::Ai,
            Panel::Config,
            Panel::Reader,
            Panel::Mail,
            Panel::Web,
            Panel::Parser,
            Panel::FileTree
        ]
    );
    // 8. 阅读页入题（2026-09-27 五公民 BAR-170）：撤阅读页（缝活跃）压过
    //    栈顶配置——变异：PANELS 漏 Reader → 本条红
    assert_eq!(
        z(
            &[Panel::Config, Panel::Reader],
            [false, false, false, false, true, false, false]
        ),
        [
            Panel::Ai,
            Panel::FileTree,
            Panel::Parser,
            Panel::Mail,
            Panel::Web,
            Panel::Config,
            Panel::Reader
        ]
    );
    // 9. 信箱页入题（2026-10-01 六公民 BAR-214）：撤信箱页（缝活跃）压过
    //    栈顶配置；信箱不在栈垫底序 = 声明序末位——变异：
    //    PANELS 漏 Mail → 本条红
    assert_eq!(
        z(
            &[Panel::Config, Panel::Mail],
            [false, false, false, false, false, true, false]
        ),
        [
            Panel::Ai,
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Web,
            Panel::Config,
            Panel::Mail,
        ]
    );
}

// BAR-207 反向钉：Demo 打样页公民位必须死透——PANELS 六件、Panel 枚举
// 无 Demo 变体（编译期钉：本卷能引用 Panel::Mail 六件即证；源码面
// 再钉一道防复活）
#[test]
fn spec_bar207_demo公民位死透() {
    // BAR-260 段③（判据③）：第七公民 Web 入列——本条原判「六件」升为
    // 「七件」，但**钉的本意不变**：Demo 打样页公民位必须死透（枚举无
    // Demo 变体、声明表无 Demo 槽）
    assert_eq!(
        stage::PANELS.len(),
        7,
        "PANELS 必须六公民（Demo 已退役，信箱 BAR-214 入列）"
    );
    assert!(stage::PANELS.contains(&Panel::Reader));
    assert!(stage::PANELS.contains(&Panel::Mail));
    let ap = include_str!("../src/ai_presence.rs");
    assert!(
        !ap.contains("Demo,") && !ap.contains("Panel::Demo"),
        "ai_presence Demo 变体/臂必须死透（BAR-207）"
    );
    let st = include_str!("../src/ui/stage.rs");
    assert!(
        !st.contains("Panel::Demo"),
        "stage PANELS 不许再出现 Demo（BAR-207）"
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

// ---- BAR-260 段③（NA0189 楼6 判据③：z 序进 PANELS 不许扰动六公民）----

/// 测试内**独立参照**（不复用生产排序函数）：六公民在「Web 不参与」时的
/// 底→顶次序，按生产文档口径自己写一遍——排序键 =（活性, 栈位阶），栈位阶
/// 不在栈＝垫底序按声明序。与生产实现不同源，才能证明「入列没扰动六公民」。
fn six_citizens_reference(stack: &[Panel], active6: [bool; 6]) -> Vec<Panel> {
    const SIX: [Panel; 6] = [
        Panel::Ai,
        Panel::Config,
        Panel::FileTree,
        Panel::Parser,
        Panel::Reader,
        Panel::Mail,
    ];
    let mut v: Vec<(i32, i32, Panel)> = Vec::new();
    for (decl, p) in SIX.iter().enumerate() {
        let pos = stack.iter().position(|x| *x == *p);
        let rank = match pos {
            Some(i) => i as i32,
            None => -(SIX.len() as i32) + decl as i32,
        };
        v.push((active6[decl] as i32, rank, *p));
    }
    v.sort_by_key(|(a, r, _)| (*a, *r));
    v.into_iter().map(|(_, _, p)| p).collect()
}

#[test]
fn spec_bar260_段3_七家入列_六公民相对序不变() {
    // 判据③的机械半边：第七公民**只追加在声明表末尾**，前六家的相对次序
    // 与栈位阶语义一字不动。变异抽检：把 Web 插进 PANELS 中间、或改前六家
    // 顺序、或把垫底位阶常量写死（不回随长度派生），本钉必红。
    assert_eq!(stage::PANELS.len(), 7, "七家入列");
    assert_eq!(
        &stage::PANELS[..6],
        &[
            Panel::Ai,
            Panel::Config,
            Panel::FileTree,
            Panel::Parser,
            Panel::Reader,
            Panel::Mail
        ],
        "前六家声明序不许动（Web 只许追加在末尾）"
    );
    assert_eq!(stage::PANELS[6], Panel::Web, "第七位是全局页");
    // 多组栈/活性下：把 Web 从结果里摘掉，必须与独立参照完全一致
    let cases: [(&[Panel], [bool; 7]); 6] = [
        (&[], [false, false, false, false, false, false, false]),
        (
            &[Panel::Ai],
            [true, false, false, false, false, false, false],
        ),
        (
            &[Panel::Config, Panel::Ai],
            [false, true, false, false, false, false, false],
        ),
        (
            &[Panel::Parser, Panel::Reader, Panel::Web],
            [false, false, false, true, false, false, false],
        ),
        (
            &[Panel::FileTree, Panel::Mail],
            [false, false, false, false, false, true, false],
        ),
        (
            &[Panel::Web, Panel::Ai],
            [false, false, false, false, false, false, true],
        ),
    ];
    for (stack, active) in cases {
        let got: Vec<Panel> = stage::panel_z_order(stack, active)
            .iter()
            .copied()
            .filter(|p| *p != Panel::Web)
            .collect();
        let want = six_citizens_reference(stack, active[..6].try_into().unwrap());
        assert_eq!(got, want, "stack={stack:?} 六公民相对序被 Web 入列扰动了");
    }
}

#[test]
fn spec_bar260_段3_动者在上_web同律() {
    // 判据③的行为半边：第七家吃与六公民同一套「动者在上」——活性时压顶；
    // 都静止时跟栈序。变异抽检：Web 活性不进排序键 / Web 永垫底，本钉必红。
    // ① 全局页推进中（active）压过栈里的解析页
    let order = stage::panel_z_order(
        &[Panel::Parser, Panel::Web],
        [false, false, false, false, false, false, true],
    );
    assert_eq!(order[6], Panel::Web, "推进中的全局页必须压顶");
    // ② 双静止：跟栈序（Web 在栈位 1，压在解析页之上）
    let order = stage::panel_z_order(
        &[Panel::Parser, Panel::Web],
        [false, false, false, false, false, false, false],
    );
    assert_eq!(
        order.to_vec()[5..7].to_vec(),
        vec![Panel::Parser, Panel::Web],
        "双静止跟栈序（Web 在栈顶）"
    );
    // ③ 全局页不在栈且不活跃：垫底组最上（声明序末位），不压栈内任何家
    let order = stage::panel_z_order(
        &[Panel::Mail],
        [false, false, false, false, false, false, false],
    );
    assert_eq!(order[6], Panel::Mail, "Web 不参与时栈顶仍是信箱页");
    assert_eq!(order[5], Panel::Web, "Web 在垫底组最上（声明序末位）");
}

#[test]
fn spec_bar260_段3_被压不越盖层() {
    // 判据③落地点（源码锚）：顶条只在全局页最顶时画——被别家盖住时不许
    // 越过盖层。变异抽检：把门槛去掉（无条件画），本钉必红。
    let root = env!("CARGO_MANIFEST_DIR");
    let app =
        std::fs::read_to_string(format!("{root}/src/android_app.rs")).expect("读 android_app.rs");
    assert!(
        app.contains("let web_on_top = z_order[6] == crate::ai_presence::Panel::Web;"),
        "顶条绘制必须吃 z 序最顶读数（web_on_top）"
    );
    // BAR-276（2026-10-20）：门槛由 web_on_top 收紧为 web_bar_on（＝ web_on_top
    // ∧ top/covered 是 Panel::Web），两渲染腿同判。本钉原字面「if web_on_top」
    // 随契约变更改写，意图不变：**门槛必须在**，不许无条件画（被压不越盖层）。
    assert!(
        app.contains("if web_bar_on &&"),
        "顶条绘制必须由门槛守着（web_bar_on＝web_on_top∧在栈，不许无条件画）"
    );
    // 楼1460 起：判据搬进产品函数 web_page::bar_visible（单源）；此处咬「由它派生」，
    // 不再咬内联表达式——内联即第二份副本，与真值表退化同源。
    assert!(
        app.contains("crate::ui::web_page::bar_visible("),
        "门槛必须由产品函数 bar_visible 派生（web_on_top 是它的首项）"
    );
    // 顶序索引必须随七家走（六家时代的 z_order[5] 已不是最顶）
    assert!(
        !app.contains("let ai_fully_covered = match z_order[5]"),
        "最顶索引必须随公民数走（七家下是 [6]）"
    );
    let gate = std::fs::read_to_string(format!("{root}/src/gate.rs")).expect("读 gate.rs");
    assert!(
        gate.contains("Panel::Web =>"),
        "值守倒帧的 z 循环必须真有 Web 臂（不是循环外硬画）"
    );
    assert!(
        !gate.contains("// BAR-260 全局页顶部原生条：在栈即常驻（置顶直画"),
        "循环外无条件直画必须删净"
    );
}
