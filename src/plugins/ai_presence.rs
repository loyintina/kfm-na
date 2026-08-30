//! plugins/ai_presence.rs — AI 外显插件（期 0 组件一，规格书
//! docs/active/ai-presence.md §三 provides 第一键）
//!
//! 形态（规格书 v1.2 §4.2 判别准则，同 input-ime 方案 A）：状态核是 Sync
//! 内部可变 → **共享实例直挂** registry，无工厂。provides
//! `Arc<AiPresenceState>`——光球绘制（壳层直读）、AI 遥控（服务调用）、
//! 探针观测（gate stats/通道十）的同源读数（D9：人走触摸、AI 走服务，
//! 同一状态核同一套考题）。
//!
//! 新插件上线纪律：带 disabled 开关，默认开、可一键关（启动配置表条目
//! id = PLUGIN_NAME，disabled 翻 true 即整插件不激活——回退第一层）。

use std::sync::Arc;

use crate::ai_presence::AiPresenceState;
use crate::base::{Ctx, Plugin, ServiceKey};

/// 插件名 = 启动配置表条目 id（disabled 一键关按它寻址）
pub const PLUGIN_NAME: &str = "ai-presence";

pub struct AiPresence;

impl AiPresence {
    pub fn new() -> Self {
        AiPresence
    }
}

impl Default for AiPresence {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AiPresence {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::of::<AiPresenceState>()]
    }

    /// 无 deps（组件一不碰终端工厂/键盘 inset 服务——inset 由壳层
    /// set_bounds 喂入，与 ime.insets 解耦）；apply 只注册共享实例，
    /// 瞬时返回
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        let undo = ctx
            .provide::<AiPresenceState>(Arc::new(AiPresenceState::new()))
            .map_err(|e| format!("注册 AI 外显状态核失败: {e:?}"))?;
        ctx.effect(undo);
        Ok(())
    }
}
