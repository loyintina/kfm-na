//! plugins/conn_provider_local.rs — 本地 PTY 连接 provider(L1)
//!
//! 设计页:`/root/kfmv4/experiments/dsh-na/na/multi-end-layering.md` §3。
//! 契约考题:tests/local_pty_spec.rs(考题 4-5)。
//!
//! 职责边界与 conn-provider-ws 同款:apply 只注册「本地会话工厂」服务
//! (`LocalPtyFactory` newtype 键——基座单一来源纪律下与 `dyn TermFactory`
//! 双键并存),瞬时返回、不真 spawn;真 spawn 发生在工厂被调用时。
//! 插件不存任何会话——TermHandle 归调用方持有,跨插件生命周期存活。

use std::sync::Arc;

use crate::base::{Ctx, Plugin, ServiceKey};
use crate::conn::{ConnConfig, Spawner};
use crate::local_pty::{LocalPtyFactory, local_pty_spawner};

/// 插件名 = 启动配置表条目 id(PluginEntry.id)
pub const PLUGIN_NAME: &str = "conn-provider-local";

pub struct ConnProviderLocal {
    /// transport 注入缝(与 ws 同款):生产 = local_pty_spawner(),
    /// 测试 = 假 transport(零 fork 判卷注册行为)
    spawner: Spawner,
}

impl ConnProviderLocal {
    /// 生产构造:真实本地 PTY transport
    pub fn new() -> Self {
        Self::with_spawner(local_pty_spawner())
    }

    /// 注入 transport(契约考题用假 transport)
    pub fn with_spawner(spawner: Spawner) -> Self {
        ConnProviderLocal { spawner }
    }
}

impl Default for ConnProviderLocal {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ConnProviderLocal {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::of::<LocalPtyFactory>()]
    }

    /// 无 inject(第一个本地 transport 插件,不依赖任何服务键)
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        let default = ctx
            .config::<ConnConfig>()
            .map(|c| (*c).clone())
            .unwrap_or_default();
        let factory: Arc<LocalPtyFactory> =
            Arc::new(LocalPtyFactory::new(default, self.spawner.clone()));
        let undo = ctx
            .provide::<LocalPtyFactory>(factory)
            .map_err(|e| format!("注册本地会话工厂失败: {e:?}"))?;
        ctx.effect(undo);
        Ok(())
    }
}
