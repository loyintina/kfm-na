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
