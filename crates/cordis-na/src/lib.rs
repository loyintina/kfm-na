//! cordis-na — Cordis 语义的 Rust 重实现：插件基座最小核心
//! （规格书 v1.3 §4 落地；契约考题 tests/base_spec.rs；差距审计与移植路线图见
//! experiments/dsh-na/na/cordis-rs-gap-audit.md）
//!
//! 语义出处：论文《A Programming Paradigm for Spatiotemporal Composability》
//! + `cordis@4.0.0-rc.7`(MIT);与本体差异表见审计文档 §三 阶段 3。
//!
//! - [`ctx`]：公告栏——内核事件总线类型化字段 + 插件服务 registry（错误两分）
//! - [`effect`]：可逆效果栈（`Disposer = Box<dyn FnOnce() + Send>`，LIFO + take-once）
//! - [`fiber`]：生命周期五态机 + 依赖引擎（epoch / notify isolate / 卸载三相 / 失败钉死）
//! - [`event`]：事件三派发（Emit / Serial 顺序短路 / Waterfall；Parallel 缓建）
//!
//! v1.1 全同步（评审裁决 1/2；规格书 v1.3 钉为设计选择）：不引 tokio 进 base；
//! apply/unload 瞬时返回契约（§4.3），慢活插件自开线程。async 契约（BoxFuture
//! disposer / async unload）接口预留，真出现「卸载必须等」的效果时由 harness
//! 复用线程模式，不手搓 executor。

pub mod ctx;
pub mod effect;
pub mod event;
pub mod fiber;

pub use ctx::{Ctx, GetError, ProvideError, ROOT_REALM, RealmId, ServiceKey};
pub use effect::{Disposer, EffectStack};
pub use event::{Dispatch, Event, Events};
pub use fiber::{Base, BaseWarning, FiberState, Idle, LoadError, Plugin, PluginEntry};
