//! plugins/input_bar.rs — 全局输入栏插件（期 0 组件三，规格书
//! docs/active/ai-presence.md §二 常驻 chrome 一）。
//!
//! 形态判别同 ai_presence：状态核 Sync 内部可变 → 共享实例直挂 registry。
//! provides `Arc<InputBarState>`——输入栏绘制（壳层直读）、发送口装配
//! （壳层 install_sender）、探针观测/注入（gate 通道十一）的同源读数。
//!
//! 新插件上线纪律：带 disabled 开关，默认开、可一键关（启动配置表条目
//! id = PLUGIN_NAME，disabled 翻 true 即整插件不激活——回退第一层）。

use std::sync::Arc;

use crate::base::{Ctx, Plugin, ServiceKey};
use crate::input_bar::InputBarState;

/// 插件名 = 启动配置表条目 id（disabled 一键关按它寻址）
pub const PLUGIN_NAME: &str = "input-bar";

pub struct InputBar;

impl InputBar {
    pub fn new() -> Self {
        InputBar
    }
}

impl Default for InputBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for InputBar {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::of::<InputBarState>()]
    }

    /// 无 deps（几何由壳层喂 inset，与 ime.insets 解耦）；apply 只注册
    /// 共享实例，瞬时返回
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        let undo = ctx
            .provide::<InputBarState>(Arc::new(InputBarState::new()))
            .map_err(|e| format!("注册全局输入栏状态核失败: {e:?}"))?;
        ctx.effect(undo);
        Ok(())
    }
}
