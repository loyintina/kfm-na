//! effect.rs — 可逆效果栈（规格书 §4.3：DisposableList 语义）
//!
//! - `Disposer = Box<dyn FnOnce() + Send>`：v1.1 全同步裁决（信箱评审裁决 2），
//!   FnOnce 类型级保单次调用
//! - LIFO 逆序 unwind（Theo 16：同插件内 LIFO 无条件安全）
//! - take-once 幂等（PAGE 56：dispose 触发两次 = 在没有应用产生的状态上跑逆元，
//!   必须至多一次）；同步版竞争消失，`disposed` 标志即可
//!
//! disposer 错误类型定死（评审附带发现）：v1 效果全是注册表式「获取」类，
//! 逆元不失败，故 `FnOnce()`；「失败 → Failed 终态」只挂在 apply 的
//! `Result<(), String>` 上（见 fiber.rs）。

/// 撤销条：运行一次即把对应效果从世界上摘除
pub type Disposer = Box<dyn FnOnce() + Send>;

/// 一个 fiber（或子 ctx）累积的效果栈
#[derive(Default)]
pub struct EffectStack {
    items: Vec<Disposer>,
    disposed: bool,
}

impl EffectStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: Disposer) {
        // 活性闸(G2)配套:经 Ctx 入口时死后注册已不可能到达(入口先查活性);
        // 直捅栈的 push-after-dispose 同样是结构性 bug——panic 显形,不静默丢弃
        assert!(
            !self.disposed,
            "INACTIVE_ACCESS: 效果栈已 dispose,push 拒绝"
        );
        self.items.push(d);
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// take-once + LIFO：第二次调用是 no-op
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        while let Some(d) = self.items.pop() {
            d();
        }
    }
}
