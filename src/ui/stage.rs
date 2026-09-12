//! stage.rs — 图层合成器状态机（渲染成本模型的基础设施，ui-base §八）。
//!
//! 契约一句话：**动画只能是 placement/alpha 变换（零光栅零上传），内容
//! 变化才允许重烘焙**。本模块只管「这一帧这层要不要重画」的判定（纯
//! 逻辑，A 档考题 tests/stage_spec.rs 钉住）；GL 侧的烘焙与 placement
//! 合成在 gles_present::ChromeLayer，槽位记账在 android_app::LayerSigs。
//!
//! sig 纪律：sig 必须列全该槽 paint 读过的每一个输入——漏一个输入 =
//! 陈旧像素（鬼影），比慢更严重。本结构不猜：调用方造 sig，这里只比
//! 相等（Option 记最近一份，None = 没烘焙过必重绘）。
//!
//! 消费者登记（提取式生长，D3）：AI 面板/快捷键行/上层 chrome 三槽
//! （2026-09-07 随 AI 面板动画立项落地）；后续菜单/工具卡/对话框入册
//! 即白拿「动画零光栅」。

/// 置脏判定器：`feed(新 sig)` 返回 true = 变了要重烘焙 / false = 照用
/// 现有烘焙物。`invalidate()` 供 GL 上下文重建后全失效（烘焙物已死）。
#[derive(Debug)]
pub struct DirtyGuard<S: PartialEq> {
    last: Option<S>,
}

impl<S: PartialEq> Default for DirtyGuard<S> {
    fn default() -> Self {
        Self { last: None }
    }
}

impl<S: PartialEq> DirtyGuard<S> {
    pub fn new() -> Self {
        Self::default()
    }
    /// 喂新 sig：与最近一份不等（或从未喂过）= true 要重烘焙
    pub fn feed(&mut self, sig: S) -> bool {
        let changed = self.last.as_ref() != Some(&sig);
        self.last = Some(sig);
        changed
    }
    /// 全失效（GLES 重建/resize 后烘焙物不存在了，下次 feed 必重绘）
    pub fn invalidate(&mut self) {
        self.last = None;
    }
}

/// 每帧槽位可见性（纯逻辑单源，BAR-070 回归钉）：GLES 图层化首版
/// 漏设上层槽 visible（默认 false 恒不画）→ 输入栏/光球/放大镜集体
/// 隐身（2026-09-07 用户实看）。槽位可见性判定从这一处出——
/// 上层 chrome（输入栏/光球/放大镜）是常驻层，任何状态都可见；
/// na-shot（值守 CPU 路径）看不见这类槽位病，判卷人=用户眼睛。
/// 返回 [键行, AI面板, 配置页, 文件树页, 解析页, 上层, 终端卡片壳]
/// （2026-09-10 面板栈 §五B 第四槽、09-11 三公民第五槽、09-12 四公民
/// 解析页槽：被覆盖的面板仍 visible=true——placement 不动，遮盖撤走
/// 随推移滑回；09-11 第六槽终端卡：与键行同规——四面板都没靠泊才可见，
/// 基座壳永不动画）
pub fn slot_visibility(
    grid_keybar: bool,
    panel_visible: bool,
    cfg_visible: bool,
    ft_visible: bool,
    pt_visible: bool,
) -> [bool; 7] {
    [
        grid_keybar,
        panel_visible,
        cfg_visible,
        ft_visible,
        pt_visible,
        true,
        grid_keybar,
    ]
}

/// 四面板 z 序裁决（BAR-083「动者在上」的四公民泛化，§五B 2026-09-12
/// 三缘语义）。旧二面板版（panel_z_cfg_on_top）的洞与它修掉的洞同构：
/// 撤顶面板瞬栈顶翻成底下的不透明面板 → 退场动画在背后播完 = 用户见
/// 瞬消。规则：**不动者按栈序（底→顶），动者（缝活跃/拖拽锁定）压到
/// 一切不动者之上；多动者之间仍按栈序**。不在栈的面板垫最底（屏外
/// 不可见，次序无关但要确定——按 PANELS 声明序）。安全论证同 BAR-083：
/// 动画结束时动者必在端点（靠泊=栈顶本身 / 屏外=不可见），z 序回落
/// 栈序那一帧被画的一方要么本来就是顶要么不可见——零像素跳变。
/// 入参 active 与 PANELS 同序对齐；返回底→顶次序（合成器按序画）。
/// 红线：本函数的活性读数只许进 z 序，**不许进 target/presence**——
/// 那是 BAR-084 的回粘回路（见 panel_target_and_draw）
pub const PANELS: [crate::ai_presence::Panel; 4] = [
    crate::ai_presence::Panel::Ai,
    crate::ai_presence::Panel::Config,
    crate::ai_presence::Panel::FileTree,
    crate::ai_presence::Panel::Parser,
];

pub fn panel_z_order(
    stack: &[crate::ai_presence::Panel],
    active: [bool; 4],
) -> [crate::ai_presence::Panel; 4] {
    // 栈位阶：在栈 = 位置下标（0 底）；不在栈 = -4+声明序（确定的垫底序）
    let rank = |p: crate::ai_presence::Panel| -> i32 {
        stack
            .iter()
            .position(|&x| x == p)
            .map(|i| i as i32)
            .unwrap_or_else(|| -4 + PANELS.iter().position(|&x| x == p).unwrap_or(0) as i32)
    };
    let mut order = PANELS;
    // 稳定排序键：（活性, 栈位阶）升序 = 底→顶；同组内栈序不动
    order.sort_by_key(|&p| {
        (
            active[PANELS.iter().position(|&x| x == p).unwrap_or(0)] as i32,
            rank(p),
        )
    });
    order
}

/// 面板目标偏移与绘制可见性（纯逻辑单源，BAR-084）：**target 只问栈**
/// （在栈 = 靠泊 0 / 不在栈 = 屏外 offscreen），**draw 才看活性**（在栈
/// 或缝动画/拖拽锁定中——退场动画必须画完）。BAR-084 病灶：BAR-083 把
/// z 序的活性读数泄漏进 presence→target 回路——退场中的面板被算成
/// present → target 翻回靠泊 → 采样器掉头回粘，出栈的面板视觉上永远
/// 停在靠泊位（2026-09-11 redroid 实锤：stats 栈空、截图配置页满屏，
/// 状态与画面两张皮）。offscreen 带符号（右缘家 +w：配置/解析 /
/// 文件树家 -w / AI 家 -h）。返回 (target, draw)
pub fn panel_target_and_draw(in_stack: bool, active: bool, offscreen: f32) -> (f32, bool) {
    (if in_stack { 0.0 } else { offscreen }, in_stack || active)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 首喂必重绘（冷启动：没有任何烘焙物）
    #[test]
    fn first_feed_repaints() {
        let mut g: DirtyGuard<u32> = DirtyGuard::new();
        assert!(g.feed(7));
    }

    // 同 sig 复喂不重绘——动画帧零光栅零上传的契约本体
    #[test]
    fn same_sig_reuses_bake() {
        let mut g: DirtyGuard<u32> = DirtyGuard::new();
        assert!(g.feed(7));
        assert!(!g.feed(7));
        assert!(!g.feed(7));
    }

    // sig 变了必重绘；变回旧值仍要重绘（只记最近一份，不做历史比对）
    #[test]
    fn changed_sig_repaints_and_back() {
        let mut g: DirtyGuard<u32> = DirtyGuard::new();
        assert!(g.feed(7));
        assert!(g.feed(9));
        assert!(g.feed(7));
        assert!(!g.feed(7));
    }

    // invalidate（GLES 重建）后必重绘，哪怕 sig 没变
    #[test]
    fn invalidate_forces_repaint() {
        let mut g: DirtyGuard<u32> = DirtyGuard::new();
        assert!(g.feed(7));
        assert!(!g.feed(7));
        g.invalidate();
        assert!(g.feed(7));
    }

    // sig 是结构化值：深度比较（Option/元组/字符串逐字段），不是指针
    #[test]
    fn deep_compare_not_identity() {
        let mut g: DirtyGuard<Option<(bool, String, u32)>> = DirtyGuard::new();
        let a = Some((true, "abc".to_string(), 3));
        let b = Some((true, "abc".to_string(), 3));
        assert!(g.feed(a.clone()));
        assert!(!g.feed(b));
        assert!(g.feed(Some((true, "abd".to_string(), 3))));
    }
}
