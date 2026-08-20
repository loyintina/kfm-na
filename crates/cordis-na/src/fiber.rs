//! fiber.rs — 生命周期状态机 + 依赖引擎（规格书 §4.3/§4.4）
//!
//! 形式态五个（v1.1 裁决 5）：Inactive(Clean) / Loading / Active / Unloading /
//! Inactive(Failed)；PENDING 不是独立态，是 Inactive(Clean) 下 target≠⊥ 的
//! 条件态；v1 无 DISPOSED 态（编译期插件无运行时移除）。
//!
//! 依赖引擎三件套：
//! - epoch = 依赖提供者实例 id 串接签名；签名变 → 依赖者重载
//! - notify 只到同 realm fiber（isolate 过滤，refresh 按 realm 作用域）
//! - 卸载三相：停供（stopping 标记，存量绑定不变）→ 依赖者排空 → LIFO disposers
//!
//! 失败语义（§4.4）：apply 失败 → 回滚已累积逆元 → 钉死 Failed，永不自动
//! 重试，不传染兄弟；恢复只能靠 reload 等显式编排动作。
//!
//! 取消边界（§4.4，v1.1 同步形态）：apply 完成后比对 target，变了则回滚
//! 已注册效果、落 Inactive(Clean)，不进 Active。

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::ctx::{Core, Ctx, Owner, ROOT_REALM, RealmId, ServiceKey};
use super::effect::EffectStack;

/// 失败/干净的 Inactive 细分（论文 ⊥/ξ）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Idle {
    Clean,
    Failed(String),
}

/// fiber 生命周期五态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FiberState {
    Inactive(Idle),
    Loading,
    Active,
    Unloading,
}

/// 插件形态（§4.3）：身份 + 依赖声明 + apply
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    /// 本插件 provide 的服务键（依赖环拓扑检测用）
    fn provides(&self) -> Vec<ServiceKey> {
        vec![]
    }
    /// 声明的依赖（inject）
    fn deps(&self) -> Vec<ServiceKey> {
        vec![]
    }
    /// 安装：注册服务/监听/效果。瞬时返回契约（§4.3）：慢活自开线程
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String>;
}

/// 配置解析器：产出类型擦除的 config 值（延迟到激活时求值）
pub type ConfigParser = Box<dyn Fn() -> Arc<dyn Any + Send + Sync> + Send>;

/// 启动配置表条目（§4.1：{id, config, disabled}，启动读一次）
pub struct PluginEntry {
    pub id: &'static str,
    pub disabled: bool,
    pub config: Option<ConfigParser>,
}

/// 基座报警（瞬时返回契约的判卷产物，考题 17）
#[derive(Debug, Clone)]
pub enum BaseWarning {
    SlowApply {
        plugin: &'static str,
        elapsed: Duration,
    },
}

/// load 错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    DuplicatePlugin(&'static str),
    CycleDetected(Vec<&'static str>),
}

/// 一个插件实例的生命周期载体
pub(crate) struct Fiber {
    pub(crate) plugin: Arc<dyn Plugin>,
    /// 目标态：配置 disabled / set_plugin_target → false（⊥）
    pub(crate) target: bool,
    pub(crate) state: FiberState,
    pub(crate) realm: RealmId,
    pub(crate) stack: EffectStack,
    /// 本 fiber 提供且尚未摘除的服务键（停供/排空的对象）
    pub(crate) provided: Vec<ServiceKey>,
    /// 激活时记录的依赖提供者实例签名（epoch）
    pub(crate) epoch: Vec<(ServiceKey, u64)>,
    pub(crate) config_parse: Option<ConfigParser>,
    /// 解析过的 config 缓存（启动读一次：重载不重复解析）
    pub(crate) config_value: Option<Arc<dyn Any + Send + Sync>>,
    /// 激活代数：每次 activate 递增;Ctx 活性闸按 (name, generation) 判活,
    /// reload 换代后旧句柄永死(考题 23)
    pub(crate) generation: u64,
}

/// refresh 单步动作
enum Act {
    Activate(&'static str),
    Unload(&'static str),
    /// epoch 签名变了：卸载后 target 仍真，下轮循环自动重激活
    Reload(&'static str),
}

/// 插件基座：fibers + registry + 事件 + 配置的总装配。
/// 全同步（v1.1）：一切生命周期转换在调用方线程上瞬时完成。
pub struct Base {
    core: Arc<Mutex<Core>>,
    /// 瞬时返回契约的 apply 预算——**机制归内核、政策归 harness**(G5 归层,
    /// 评审裁决 3):cordis-na 默认关闭,harness 显式开启并自带预算值
    apply_budget: Option<Duration>,
}

impl Base {
    /// 配置表启动读一次（§4.1；改配置重启生效，热调和留 v1+）
    pub fn new(config: Vec<PluginEntry>) -> Self {
        Base {
            core: Arc::new(Mutex::new(Core::new(config))),
            apply_budget: None,
        }
    }

    /// 开启瞬时返回契约检查并给预算(harness 政策;kfm-na 现值 50ms)
    pub fn with_apply_budget(mut self, budget: Duration) -> Self {
        self.apply_budget = Some(budget);
        self
    }

    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().expect("base core 锁被毒化")
    }

    /// 根 realm 的公告栏
    pub fn ctx(&self) -> Ctx {
        self.ctx_in(ROOT_REALM)
    }

    pub fn ctx_in(&self, realm: RealmId) -> Ctx {
        Ctx::new(Arc::clone(&self.core), realm, Owner::Root)
    }

    pub fn load<P: Plugin + 'static>(&self, plugin: P) -> Result<(), LoadError> {
        self.load_in(plugin, ROOT_REALM)
    }

    /// 注册插件：登记 fiber → 依赖环拓扑检测（注册期报错，不静默挂起）→ refresh
    pub fn load_in<P: Plugin + 'static>(&self, plugin: P, realm: RealmId) -> Result<(), LoadError> {
        let name = plugin.name();
        {
            let mut c = self.lock();
            if c.fibers.contains_key(name) {
                return Err(LoadError::DuplicatePlugin(name));
            }
            let (disabled, parse) = match c.config.iter_mut().find(|e| e.id == name) {
                Some(e) => (e.disabled, e.config.take()),
                None => (false, None),
            };
            for k in plugin.provides().iter().chain(plugin.deps().iter()) {
                c.declared.insert(*k);
            }
            c.fibers.insert(
                name,
                Fiber {
                    plugin: Arc::new(plugin),
                    target: !disabled,
                    state: FiberState::Inactive(Idle::Clean),
                    realm,
                    stack: EffectStack::new(),
                    provided: Vec::new(),
                    epoch: Vec::new(),
                    config_parse: parse,
                    config_value: None,
                    generation: 0,
                },
            );
            c.order.push(name);
            if let Some(cycle) = detect_cycle(&c, realm) {
                // 成环 = 明确报错，新插件不留存（不许静默双挂起）
                c.fibers.remove(name);
                c.order.retain(|n| *n != name);
                return Err(LoadError::CycleDetected(cycle));
            }
        }
        self.refresh(realm);
        Ok(())
    }

    /// 显式卸载（target → ⊥；三相：停供 → 依赖者排空 → LIFO disposers）
    pub fn unload(&self, name: &str) {
        let Some(key) = self.resolve(name) else {
            return;
        };
        let realm = {
            let mut c = self.lock();
            let f = c.fibers.get_mut(key).expect("fiber 必须存在");
            f.target = false;
            f.realm
        };
        self.unload_fiber(key);
        self.refresh(realm);
    }

    /// 显式重载（失败 fiber 的唯一恢复通道，§4.4）
    pub fn reload(&self, name: &str) {
        let Some(key) = self.resolve(name) else {
            return;
        };
        let (was_active, realm) = {
            let mut c = self.lock();
            let f = c.fibers.get_mut(key).expect("fiber 必须存在");
            f.target = true;
            if matches!(f.state, FiberState::Inactive(Idle::Failed(_))) {
                // 显式编排动作：解除钉死，回到 Clean 重新走 L-Begin
                f.state = FiberState::Inactive(Idle::Clean);
            }
            (f.state == FiberState::Active, f.realm)
        };
        if was_active {
            self.unload_fiber(key);
        }
        self.refresh(realm);
    }

    /// 按名字找回注册时的 'static 键
    fn resolve(&self, name: &str) -> Option<&'static str> {
        self.lock().order.iter().copied().find(|n| *n == name)
    }

    pub fn state(&self, name: &str) -> Option<FiberState> {
        self.lock().fibers.get(name).map(|f| f.state.clone())
    }

    /// PENDING = Inactive(Clean) 且 target≠⊥ 且依赖未齐
    pub fn is_pending(&self, name: &str) -> bool {
        let c = self.lock();
        let Some(f) = c.fibers.get(name) else {
            return false;
        };
        f.state == FiberState::Inactive(Idle::Clean)
            && f.target
            && !deps_satisfied(&c, f.realm, &f.plugin.deps())
    }

    /// 观察等价判据用：本 realm 服务表条目数
    pub fn service_count(&self, realm: RealmId) -> usize {
        self.lock()
            .services
            .keys()
            .filter(|(r, _)| *r == realm)
            .count()
    }

    pub fn warnings(&self) -> Vec<BaseWarning> {
        self.lock().warnings.clone()
    }

    /// notify：服务变更后调和本 realm 的 fiber 到 target（isolate 过滤 =
    /// 只扫同 realm fiber）。不动点循环，每轮必有状态迁移，64 轮兜底。
    fn refresh(&self, realm: RealmId) {
        for _ in 0..64 {
            let act = {
                let c = self.lock();
                let mut found = None;
                for &name in &c.order {
                    let f = &c.fibers[&name];
                    if f.realm != realm {
                        continue;
                    }
                    let deps = f.plugin.deps();
                    match f.state {
                        FiberState::Inactive(Idle::Clean)
                            if f.target && deps_satisfied(&c, realm, &deps) =>
                        {
                            found = Some(Act::Activate(name));
                            break;
                        }
                        FiberState::Active => {
                            if !deps_satisfied(&c, realm, &deps) {
                                found = Some(Act::Unload(name));
                                break;
                            }
                            if epoch_changed(&c, realm, &deps, &f.epoch) {
                                found = Some(Act::Reload(name));
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                found
            };
            match act {
                None => break,
                Some(Act::Activate(name)) => self.activate(name),
                Some(Act::Unload(name)) | Some(Act::Reload(name)) => self.unload_fiber(name),
            }
        }
    }

    /// 激活：Loading → （配置延迟解析）→ apply → 比对 target → Active。
    /// 全程不持锁跑插件代码（插件可自由回调 ctx）。
    fn activate(&self, name: &'static str) {
        let (plugin, ctx) = {
            let mut c = self.lock();
            let f = c.fibers.get_mut(name).expect("fiber 必须存在");
            f.state = FiberState::Loading;
            f.generation += 1;
            let generation = f.generation;
            (
                f.plugin.clone(),
                Ctx::new(
                    Arc::clone(&self.core),
                    f.realm,
                    Owner::Fiber(name, generation),
                ),
            )
        };
        // 配置延迟解析（§4.1，fiber.ts:740）：依赖就绪后、apply 前求值一次；
        // 未激活的插件永远走不到这里
        let parse = {
            let mut c = self.lock();
            c.fibers.get_mut(name).and_then(|f| f.config_parse.take())
        };
        if let Some(p) = parse {
            let v = p();
            self.lock()
                .fibers
                .get_mut(name)
                .expect("fiber 必须存在")
                .config_value = Some(v);
        }

        let mut ctx = ctx;
        let start = Instant::now();
        let result = plugin.apply(&mut ctx);
        let elapsed = start.elapsed();
        // 瞬时返回契约（§4.3 v1.1）：超预算记报警不记失败（理由见考题 17);
        // 预算检查默认关(G5 归层),harness 显式开启
        if let Some(budget) = self.apply_budget
            && elapsed > budget
        {
            self.lock().warnings.push(BaseWarning::SlowApply {
                plugin: name,
                elapsed,
            });
        }

        match result {
            Ok(()) => {
                let target_still = self.lock().fibers[name].target;
                if target_still {
                    let mut c = self.lock();
                    let (realm, deps) = {
                        let f = &c.fibers[name];
                        (f.realm, f.plugin.deps())
                    };
                    let epoch = current_epoch(&c, realm, &deps);
                    let f = c.fibers.get_mut(name).expect("fiber 必须存在");
                    f.epoch = epoch;
                    f.state = FiberState::Active;
                } else {
                    // 取消边界（§4.4 同步形态）：apply 完成后比对 target，
                    // 已变 → 回滚已注册效果、落 Clean，不进 Active
                    let mut stack = take_stack(&self.core, name);
                    stack.dispose();
                    self.lock()
                        .fibers
                        .get_mut(name)
                        .expect("fiber 必须存在")
                        .state = FiberState::Inactive(Idle::Clean);
                }
            }
            Err(e) => {
                // 失败（§4.4）：先恢复已累积的逆元，再钉死 Failed
                let mut stack = take_stack(&self.core, name);
                stack.dispose();
                self.lock()
                    .fibers
                    .get_mut(name)
                    .expect("fiber 必须存在")
                    .state = FiberState::Inactive(Idle::Failed(e));
            }
        }
    }

    /// 卸载三相（§4.3 修订 3）：停供 → 依赖者排空 → LIFO disposers。
    /// 非 Active 的 fiber 无效果可拆，直接返回（幂等）。
    fn unload_fiber(&self, name: &'static str) {
        // 相一 · 停供：标记 Unloading，服务键撤出可见性（stopping），
        // 存量 Arc 持有者读到的绑定值不变（Theo 63）
        let (provided, realm) = {
            let mut c = self.lock();
            let Some(f) = c.fibers.get_mut(name) else {
                return;
            };
            if f.state != FiberState::Active {
                return;
            }
            f.state = FiberState::Unloading;
            let provided = f.provided.clone();
            let realm = f.realm;
            for k in &provided {
                if let Some(e) = c.services.get_mut(&(realm, *k)) {
                    e.stopping = true;
                }
            }
            (provided, realm)
        };
        // 相二 · 依赖者排空：撤完所有（传递闭包）依赖者才轮到自己
        loop {
            let dependent = {
                let c = self.lock();
                c.order.iter().copied().find(|n| {
                    let f = &c.fibers[n];
                    f.realm == realm
                        && f.state == FiberState::Active
                        && f.plugin.deps().iter().any(|k| provided.contains(k))
                })
            };
            match dependent {
                Some(d) => self.unload_fiber(d),
                None => break,
            }
        }
        // 相三 · LIFO disposers：逆序跑累积器（插件注册的逆元在此摘除服务/
        // 监听器；忘摘 = 泄漏，由观察等价考题判红，基座不兜底）
        let mut stack = take_stack(&self.core, name);
        stack.dispose();
        self.lock()
            .fibers
            .get_mut(name)
            .expect("fiber 必须存在")
            .state = FiberState::Inactive(Idle::Clean);
    }
}

/// 把 fiber 的效果栈整栈取出（take-once：重复卸载拿到的是空栈）
fn take_stack(core: &Arc<Mutex<Core>>, name: &'static str) -> EffectStack {
    let mut c = core.lock().expect("base core 锁被毒化");
    std::mem::take(&mut c.fibers.get_mut(name).expect("fiber 必须存在").stack)
}

/// 依赖满足判定：同 realm 内有非停供条目
fn deps_satisfied(c: &Core, realm: RealmId, deps: &[ServiceKey]) -> bool {
    deps.iter()
        .all(|k| c.services.get(&(realm, *k)).is_some_and(|e| !e.stopping))
}

/// 当前依赖提供者实例签名（epoch 原料；按服务键名排序取定值）
fn current_epoch(c: &Core, realm: RealmId, deps: &[ServiceKey]) -> Vec<(ServiceKey, u64)> {
    let mut epoch: Vec<(ServiceKey, u64)> = deps
        .iter()
        .filter_map(|k| c.services.get(&(realm, *k)).map(|e| (*k, e.instance)))
        .collect();
    epoch.sort_by_key(|(k, _)| k.name());
    epoch
}

/// epoch 比对：签名变 → 依赖者重载。
/// 注（实施精度）：单一来源纪律下实例更换必经「停供-摘除」间隙，通常先被
/// 「依赖不满足 → Unload」捕获；本分支是签名机制的直接落地，作为防御层保留
/// （如未来 broker 模式原地换绑时它就是唯一判据）
fn epoch_changed(
    c: &Core,
    realm: RealmId,
    deps: &[ServiceKey],
    recorded: &[(ServiceKey, u64)],
) -> bool {
    current_epoch(c, realm, deps) != recorded
}

/// 依赖环拓扑检测（§4.3 修订 5，注册期）：沿「插件 → 其依赖的提供者」边
/// 三色 DFS；环上的插件清单随 CycleDetected 返回
fn detect_cycle(c: &Core, realm: RealmId) -> Option<Vec<&'static str>> {
    let mut provider_of: HashMap<ServiceKey, &'static str> = HashMap::new();
    for &name in &c.order {
        let f = &c.fibers[&name];
        if f.realm != realm {
            continue;
        }
        for k in f.plugin.provides() {
            provider_of.insert(k, name);
        }
    }
    let edges: HashMap<&'static str, Vec<&'static str>> = c
        .order
        .iter()
        .copied()
        .filter(|n| c.fibers[n].realm == realm)
        .map(|n| {
            let deps = c.fibers[&n].plugin.deps();
            (
                n,
                deps.iter()
                    .filter_map(|k| provider_of.get(k).copied())
                    .collect(),
            )
        })
        .collect();

    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit(
        node: &'static str,
        edges: &HashMap<&'static str, Vec<&'static str>>,
        marks: &mut HashMap<&'static str, Mark>,
        stack: &mut Vec<&'static str>,
    ) -> Option<Vec<&'static str>> {
        match marks.get(node) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => {
                let pos = stack.iter().position(|n| *n == node).expect("在栈上");
                return Some(stack[pos..].to_vec());
            }
            None => {}
        }
        marks.insert(node, Mark::Visiting);
        stack.push(node);
        if let Some(nexts) = edges.get(node) {
            for &m in nexts {
                if let Some(cyc) = visit(m, edges, marks, stack) {
                    return Some(cyc);
                }
            }
        }
        stack.pop();
        marks.insert(node, Mark::Done);
        None
    }

    let mut marks = HashMap::new();
    let mut stack = Vec::new();
    for &name in &c.order {
        if c.fibers[&name].realm != realm || marks.contains_key(&name) {
            continue;
        }
        if let Some(cyc) = visit(name, &edges, &mut marks, &mut stack) {
            return Some(cyc);
        }
    }
    None
}
