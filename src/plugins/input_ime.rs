//! plugins/input_ime.rs — 输入/IME 域插件（规格书 §3 第一批最后一域）
//!
//! 设计页：`/root/kfmv4/experiments/dsh-na/na/input-ime.md`（v0.1，方案 A
//! 批准）。契约考题：tests/input_ime_spec.rs（考题 4-8）。
//!
//! 形态（规格书 v1.2 §4.2 判别准则）：两个服务都是 Sync 内部可变 →
//! **共享实例直挂** registry，无工厂。
//! - `input.modifiers`：`Arc<keybar::ModifierState>`（具体类型直挂，原子态
//!   无需 trait 擦除）——进程静态 MODS 的归属化（§4.5 共享态纪律）
//! - `ime.insets`：`Arc<dyn ImeInsets>`——构造注入（生产 = JniInsets，
//!   考题 = 假实现；AndroidApp 句柄是运行时对象不走配置表，评审裁决 4）
//!
//! 不进的：ime_queue（JNI 桥端点，B 档胶水同 report.rs，评审裁决 3）、
//! keymap/keybar 布局命中（纯函数不构成实现差异点）。

use std::sync::Arc;

use crate::base::{Ctx, Plugin, ServiceKey};
use crate::insets::ImeInsets;
use crate::keybar::ModifierState;

/// 插件名 = 启动配置表条目 id（v1 零配置，条目可省——设计页 §5）
pub const PLUGIN_NAME: &str = "input-ime";

pub struct InputIme {
    insets: Arc<dyn ImeInsets>,
}

impl InputIme {
    /// 构造注入键盘来源（生产 = JniInsets::new(android_app)，考题 = 假实现）
    pub fn new(insets: Arc<dyn ImeInsets>) -> Self {
        InputIme { insets }
    }
}

impl Plugin for InputIme {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn provides(&self) -> Vec<ServiceKey> {
        vec![
            ServiceKey::of::<ModifierState>(),
            ServiceKey::of::<dyn ImeInsets>(),
        ]
    }

    /// 无 inject（设计页 §3：修饰键×app_cursor 的翻译是调用方编排，不是插件依赖）
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        // 单一来源纪律：任一键冲突 → apply Err → 钉死 Failed（考题 8）
        let undo_mods = ctx
            .provide::<ModifierState>(Arc::new(ModifierState::new()))
            .map_err(|e| format!("注册修饰键服务失败: {e:?}"))?;
        ctx.effect(undo_mods);
        let undo_insets = ctx
            .provide::<dyn ImeInsets>(self.insets.clone())
            .map_err(|e| format!("注册键盘来源失败: {e:?}"))?;
        ctx.effect(undo_insets);
        Ok(())
    }
}
