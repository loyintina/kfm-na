//! base/ — 插件基座最小核心（规格书 v1.1 §4 落地；契约考题 tests/base_spec.rs）
//!
//! - [`ctx`]：公告栏——内核服务类型化字段 + 插件服务 registry（错误两分）
//! - [`effect`]：可逆效果栈（`Disposer = Box<dyn FnOnce() + Send>`，LIFO + take-once）
//! - [`fiber`]：生命周期五态机 + 依赖引擎（epoch / notify isolate / 卸载三相 / 失败钉死）
//! - [`event`]：事件三派发（Emit / Serial 顺序短路 / Waterfall；Parallel 缓建）
//!
//! v1.1 全同步（评审裁决 1/2）：不引 tokio 进 base；apply/unload 瞬时返回契约
//! （§4.3），慢活插件自开线程。async 契约（BoxFuture disposer / async unload）
//! 接口预留，真出现「卸载必须等」的效果时复用 conn.rs 线程模式，不手搓 executor。

pub mod ctx;
pub mod effect;
pub mod event;
pub mod fiber;

pub use ctx::{Ctx, GetError, ProvideError, ROOT_REALM, RealmId, ServiceKey, Term};
pub use effect::{Disposer, EffectStack};
pub use event::{Dispatch, Event, Events};
pub use fiber::{Base, BaseWarning, FiberState, Idle, LoadError, Plugin, PluginEntry};
