//! ctx.rs — 公告栏（规格书 §4.2）：内核服务类型化字段 + 插件服务 registry
//!
//! - 内核服务是 Ctx 的类型化字段（`events` / `term` 占位）
//! - 插件服务 registry：`(RealmId, ServiceKey) → Arc<T>`。按 trait 取回、
//!   不 import 具体类型——存储侧把整个 `Arc<T>` 再包一层 Arc 擦除成
//!   `Arc<dyn Any + Send + Sync>`，取回时 downcast 到 `Arc<T>` 还原
//!   （等价于 downcast-rs 同款「注册时定死具体类型」模式，`Arc<dyn Any>`
//!   本就不能直接 downcast 成 trait object）
//! - get 错误两分：DeclaredButInactive（已声明未激活）/ Undeclared（未声明）
//! - 单一来源纪律：同 realm 同名二次 provide = 错误，绝不覆盖

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use super::effect::{Disposer, EffectStack};
use super::event::{Events, ListenerEntry};
use super::fiber::{BaseWarning, Fiber, PluginEntry};

/// isolate 作用域标签（§4.3：realm = 按服务键的作用域表）
pub type RealmId = u64;
/// 根作用域（应用级）
pub const ROOT_REALM: RealmId = 0;

/// 服务键：按 trait 寻址（`ServiceKey::of::<dyn Trait>()`）
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServiceKey {
    id: TypeId,
    name: &'static str,
}

impl ServiceKey {
    pub fn of<T: ?Sized + 'static>() -> Self {
        ServiceKey {
            id: TypeId::of::<T>(),
            name: std::any::type_name::<T>(),
        }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl std::fmt::Debug for ServiceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ServiceKey({})", self.name)
    }
}

impl std::fmt::Display for ServiceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// get 错误两分（§4.2 修订 10）：已声明未激活 vs 未声明
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetError {
    DeclaredButInactive(ServiceKey),
    Undeclared(ServiceKey),
}

/// 单一来源纪律（§4.2 修订 4）：同名二次 provide = 错误，绝不覆盖
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvideError {
    AlreadyProvided(ServiceKey),
}

/// 内核服务占位（§4.2 内核服务类型化字段；接真终端属阶段 3）
#[derive(Clone, Copy, Debug, Default)]
pub struct Term;

/// 效果的归属：根 ctx / 某 fiber / 某子 ctx
#[derive(Clone, Copy)]
pub(crate) enum Owner {
    Root,
    Fiber(&'static str),
    Child(u64),
}

/// 服务表条目。`value` 实为 `Arc<T>`（把 trait object 的 Arc 整体再包一层
/// Arc 擦除），`instance` 是提供者实例 id（epoch 签名的原料）
pub(crate) struct ServiceEntry {
    pub(crate) value: Arc<dyn Any + Send + Sync>,
    pub(crate) instance: u64,
    /// 卸载三相之「停供」标记：新 get 失败，存量 Arc 持有者读到的绑定值不变
    pub(crate) stopping: bool,
}

/// 基座全部可变状态的唯一居所（§4.5：共享可变状态必须挂在服务键/基座后面）
pub(crate) struct Core {
    pub(crate) services: HashMap<(RealmId, ServiceKey), ServiceEntry>,
    /// 已声明服务键全集（所有已注册插件的 provides ∪ deps）——错误两分的依据
    pub(crate) declared: HashSet<ServiceKey>,
    pub(crate) fibers: HashMap<&'static str, Fiber>,
    /// 装载顺序（refresh 按它遍历，保证确定性）
    pub(crate) order: Vec<&'static str>,
    pub(crate) events: HashMap<TypeId, Vec<ListenerEntry>>,
    /// 启动配置表（load 时按 id 取走对应条目）
    pub(crate) config: Vec<PluginEntry>,
    pub(crate) next_instance: u64,
    pub(crate) next_listener: u64,
    pub(crate) next_child: u64,
    /// 子 ctx 效果栈（撤销条贴父栈 = 父栈里有一条 dispose 它的逆元）
    pub(crate) child_stacks: HashMap<u64, EffectStack>,
    /// 根 ctx 注册的效果（应用级）
    pub(crate) root_stack: EffectStack,
    pub(crate) warnings: Vec<BaseWarning>,
}

impl Core {
    pub(crate) fn new(config: Vec<PluginEntry>) -> Self {
        Core {
            services: HashMap::new(),
            declared: HashSet::new(),
            fibers: HashMap::new(),
            order: Vec::new(),
            events: HashMap::new(),
            config,
            next_instance: 0,
            next_listener: 0,
            next_child: 0,
            child_stacks: HashMap::new(),
            root_stack: EffectStack::new(),
            warnings: Vec::new(),
        }
    }
}

/// 插件眼中的上下文：内核服务是类型化字段，插件服务走 provide/get
#[derive(Clone)]
pub struct Ctx {
    /// 内核服务：事件总线
    pub events: Events,
    /// 内核服务：终端（占位）
    pub term: Term,
    pub(crate) core: Arc<Mutex<Core>>,
    pub(crate) realm: RealmId,
    pub(crate) owner: Owner,
}

impl Ctx {
    pub(crate) fn new(core: Arc<Mutex<Core>>, realm: RealmId, owner: Owner) -> Self {
        Ctx {
            events: Events::new(core.clone(), realm),
            term: Term,
            core,
            realm,
            owner,
        }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().expect("base core 锁被毒化")
    }

    /// 注册插件服务；返回撤销条（插件义务：`ctx.effect(undo)` 注册进效果栈，
    /// 忘回滚由观察等价考题判红——§4.3：disposer 正确性 = 作者义务 + 测试强制）
    pub fn provide<T: ?Sized + Send + Sync + 'static>(
        &self,
        svc: Arc<T>,
    ) -> Result<Disposer, ProvideError> {
        let key = ServiceKey::of::<T>();
        let mut c = self.lock();
        if c.services.contains_key(&(self.realm, key)) {
            return Err(ProvideError::AlreadyProvided(key));
        }
        c.declared.insert(key);
        c.next_instance += 1;
        let instance = c.next_instance;
        c.services.insert(
            (self.realm, key),
            ServiceEntry {
                value: Arc::new(svc),
                instance,
                stopping: false,
            },
        );
        if let Owner::Fiber(name) = self.owner
            && let Some(f) = c.fibers.get_mut(name)
        {
            f.provided.push(key);
        }
        let core = Arc::clone(&self.core);
        let realm = self.realm;
        Ok(Box::new(move || {
            let mut c = core.lock().expect("base core 锁被毒化");
            // 实例比对：绑定若已被更换（epoch 新实例），旧逆元不许误删新绑定
            if let Some(e) = c.services.get(&(realm, key))
                && e.instance == instance
            {
                c.services.remove(&(realm, key));
            }
        }))
    }

    /// 按 trait 取回服务（不 import 具体类型）
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, GetError> {
        let key = ServiceKey::of::<T>();
        let c = self.lock();
        if let Some(e) = c.services.get(&(self.realm, key))
            && !e.stopping
            && let Ok(a) = e.value.clone().downcast::<Arc<T>>()
        {
            return Ok((*a).clone());
        }
        if c.declared.contains(&key) {
            Err(GetError::DeclaredButInactive(key))
        } else {
            Err(GetError::Undeclared(key))
        }
    }

    /// 把撤销条挂进当前 fiber（或子 ctx / 根）的效果栈
    pub fn effect(&self, d: Disposer) {
        let mut c = self.lock();
        match self.owner {
            Owner::Root => c.root_stack.push(d),
            Owner::Fiber(name) => {
                if let Some(f) = c.fibers.get_mut(name) {
                    f.stack.push(d);
                }
            }
            Owner::Child(id) => {
                if let Some(s) = c.child_stacks.get_mut(&id) {
                    s.push(d);
                }
            }
        }
    }

    /// 读本插件的已解析配置（激活时才解析，§4.1 配置延迟解析）
    pub fn config<C: Send + Sync + 'static>(&self) -> Option<Arc<C>> {
        let c = self.lock();
        let Owner::Fiber(name) = self.owner else {
            return None;
        };
        c.fibers
            .get(name)?
            .config_value
            .as_ref()?
            .clone()
            .downcast::<C>()
            .ok()
    }

    /// 派生子 ctx（独立 realm）；其撤销条自动贴进父栈（树形级联）
    pub fn fork(&self, realm: RealmId) -> Ctx {
        let id = {
            let mut c = self.lock();
            c.next_child += 1;
            let id = c.next_child;
            c.child_stacks.insert(id, EffectStack::new());
            id
        };
        // 父栈上贴一条「dispose 子栈」的逆元 → 父卸载时子效果级联回滚
        let core = Arc::clone(&self.core);
        self.effect(Box::new(move || {
            let stack = core
                .lock()
                .expect("base core 锁被毒化")
                .child_stacks
                .remove(&id);
            if let Some(mut s) = stack {
                s.dispose();
            }
        }));
        Ctx::new(Arc::clone(&self.core), realm, Owner::Child(id))
    }

    /// 翻转某插件的目标态（取消边界的同步形态，考题 16）
    pub fn set_plugin_target(&self, name: &str, target: bool) {
        let mut c = self.lock();
        if let Some(f) = c.fibers.get_mut(name) {
            f.target = target;
        }
    }

    pub fn realm(&self) -> RealmId {
        self.realm
    }
}
