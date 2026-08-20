//! plugins/conn_provider_ws.rs — kfmv4 ws 协议连接 provider（规格书 §3 第一批）
//!
//! 设计页：`/root/kfmv4/experiments/dsh-na/na/connection-provider.md`（v0.1，
//! 评审五条裁决落地）。契约考题：tests/conn_provider_spec.rs（考题 5-9）。
//!
//! 职责边界：apply 只注册「连接工厂」服务（`dyn TermFactory`），瞬时返回、
//! **不真连接**（v1.1 瞬时返回契约）；真连接发生在工厂被调用时，由工厂内部
//! 开线程。插件不存任何会话——TermHandle 归调用方持有，跨插件生命周期存活。

use std::sync::Arc;

use crate::base::{Ctx, Plugin, ServiceKey};
use crate::conn::{ConnConfig, Spawner, TermFactory, WsTermFactory, ws_spawner};

/// 插件名 = 启动配置表条目 id（PluginEntry.id）
pub const PLUGIN_NAME: &str = "conn-provider-ws";

pub struct ConnProviderWs {
    /// transport 注入缝（评审裁决 4）：生产 = ws_spawner()，测试 = 假 transport
    spawner: Spawner,
}

impl ConnProviderWs {
    /// 生产构造：真实 ws transport
    pub fn new() -> Self {
        Self::with_spawner(ws_spawner())
    }

    /// 注入 transport（契约考题用假 transport；零网络依赖判卷）
    pub fn with_spawner(spawner: Spawner) -> Self {
        ConnProviderWs { spawner }
    }
}

impl Default for ConnProviderWs {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ConnProviderWs {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::of::<dyn TermFactory>()]
    }

    /// 无 inject（设计页 §3：第一个插件不依赖任何服务键）
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        // 配置延迟解析（§4.1）：启动配置表条目 → 默认连接参数；无条目走现状默认
        let default = ctx
            .config::<ConnConfig>()
            .map(|c| (*c).clone())
            .unwrap_or_default();
        let factory: Arc<dyn TermFactory> =
            Arc::new(WsTermFactory::new(default, self.spawner.clone()));
        // 单一来源纪律：同名二次 provide = AlreadyProvided → apply Err →
        // 钉死 Failed，不传染先到者（考题 9）
        let undo = ctx
            .provide::<dyn TermFactory>(factory)
            .map_err(|e| format!("注册连接工厂失败: {e:?}"))?;
        // 卸载三相之 dispose：摘除注册表条目（忘挂 = 泄漏，观察等价考题判红）
        ctx.effect(undo);
        Ok(())
    }
}
