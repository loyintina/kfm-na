//! event.rs — 事件三派发（v1；规格书 §4.3 + 评审裁决 3）
//!
//! 派发模式即公开契约：每个事件在 `Event::DISPATCH` 上标注派发模式，
//! 用错姿势监听/派发 = 立即 panic。v1 实现：Emit（同步观察）/ Serial
//! （顺序短路，cordis serial+bail 合一）/ Waterfall（委托链，不调 next
//! 即否决）。Parallel 缓建（同步基座下不可达）。

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex, MutexGuard};

use super::ctx::{Core, Owner, RealmId, gate};
use super::effect::Disposer;

/// 派发模式（v1 三派发；Parallel 缓建）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// 同步观察：注册序逐个调用，无返回值
    Emit,
    /// 顺序短路：注册序逐个调用，首个 Err 短路并透传
    Serial,
    /// 委托链：监听器收 (payload, next)，不调 next 即否决
    Waterfall,
}

/// 事件契约：名字 + 派发模式 + 载荷类型
pub trait Event: Send + Sync + 'static {
    const NAME: &'static str;
    const DISPATCH: Dispatch;
    type Data: Send + 'static;
}

/// 监听器条目。`f` 实为 `Arc<具体监听签名>`（哪种签名由事件派发模式决定，
/// 注册侧已断言模式匹配，派发侧 downcast 是形式核对）
pub(crate) struct ListenerEntry {
    pub(crate) id: u64,
    pub(crate) realm: RealmId,
    pub(crate) f: Arc<dyn Any + Send + Sync>,
}

/// 三种派发的监听器签名（擦除后进 ListenerEntry.f，派发时 downcast 还原）
type EmitListener<D> = Arc<dyn Fn(&D) + Send + Sync>;
type SerialListener<D> = Arc<dyn Fn(&D) -> Result<(), String> + Send + Sync>;
type WaterfallListener<D> = Arc<dyn Fn(D, &dyn Fn(D) -> D) -> D + Send + Sync>;

/// 事件总线句柄（随 Ctx 派发，realm 隔离;带 owner——活性闸同 Ctx,
/// 死 fiber 发射/监听事件同样是死后访问,评审裁决 1:同闸不收窄)
#[derive(Clone)]
pub struct Events {
    pub(crate) core: Arc<Mutex<Core>>,
    pub(crate) realm: RealmId,
    pub(crate) owner: Owner,
}

impl Events {
    pub(crate) fn new(core: Arc<Mutex<Core>>, realm: RealmId, owner: Owner) -> Self {
        Events { core, realm, owner }
    }

    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().expect("base core 锁被毒化")
    }

    /// 登记监听器并返回摘除条（摘下即「观察不到」，观察等价判据的一部分）
    fn add_listener<E: Event>(&self, op: &str, f: Arc<dyn Any + Send + Sync>) -> Disposer {
        let id = {
            gate(&self.core, &self.owner, op);
            let mut c = self.lock();
            c.next_listener += 1;
            let id = c.next_listener;
            c.events
                .entry(TypeId::of::<E>())
                .or_default()
                .push(ListenerEntry {
                    id,
                    realm: self.realm,
                    f,
                });
            id
        };
        let core = Arc::clone(&self.core);
        Box::new(move || {
            let mut c = core.lock().expect("base core 锁被毒化");
            let empty = if let Some(list) = c.events.get_mut(&TypeId::of::<E>()) {
                list.retain(|l| l.id != id);
                list.is_empty()
            } else {
                false
            };
            if empty {
                c.events.remove(&TypeId::of::<E>());
            }
        })
    }

    /// 快照本 realm 的监听器（快照后放锁再调用：监听器可以自由回调总线）
    fn snapshot<E: Event>(&self) -> Vec<Arc<dyn Any + Send + Sync>> {
        let c = self.lock();
        c.events
            .get(&TypeId::of::<E>())
            .map(|list| {
                list.iter()
                    .filter(|l| l.realm == self.realm)
                    .map(|l| l.f.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 监听 Emit 事件；返回摘除条
    pub fn on_emit<E: Event>(&self, f: impl Fn(&E::Data) + Send + Sync + 'static) -> Disposer {
        assert!(
            E::DISPATCH == Dispatch::Emit,
            "派发模式即公开契约：{} 标注 {:?}，不能用 on_emit 监听",
            E::NAME,
            E::DISPATCH
        );
        let f: EmitListener<E::Data> = Arc::new(f);
        self.add_listener::<E>("on_emit", Arc::new(f))
    }

    /// 同步观察派发：返回时全部监听器已跑完
    pub fn emit<E: Event>(&self, data: &E::Data) {
        assert!(
            E::DISPATCH == Dispatch::Emit,
            "派发模式即公开契约：{} 标注 {:?}，不能用 emit 派发",
            E::NAME,
            E::DISPATCH
        );
        gate(&self.core, &self.owner, "emit");
        for f in self.snapshot::<E>() {
            let f = f
                .downcast::<EmitListener<E::Data>>()
                .expect("Emit 监听签名不符");
            f(data);
        }
    }

    /// 监听 Serial 事件；返回摘除条
    pub fn on_serial<E: Event>(
        &self,
        f: impl Fn(&E::Data) -> Result<(), String> + Send + Sync + 'static,
    ) -> Disposer {
        assert!(
            E::DISPATCH == Dispatch::Serial,
            "派发模式即公开契约：{} 标注 {:?}，不能用 on_serial 监听",
            E::NAME,
            E::DISPATCH
        );
        let f: SerialListener<E::Data> = Arc::new(f);
        self.add_listener::<E>("on_serial", Arc::new(f))
    }

    /// 顺序短路派发（serial+bail 合一）：首个 Err 短路并透传
    pub fn serial<E: Event>(&self, data: &E::Data) -> Result<(), String> {
        assert!(
            E::DISPATCH == Dispatch::Serial,
            "派发模式即公开契约：{} 标注 {:?}，不能用 serial 派发",
            E::NAME,
            E::DISPATCH
        );
        gate(&self.core, &self.owner, "serial");
        for f in self.snapshot::<E>() {
            let f = f
                .downcast::<SerialListener<E::Data>>()
                .expect("Serial 监听签名不符");
            f(data)?;
        }
        Ok(())
    }

    /// 监听 Waterfall 事件（委托链）；返回摘除条
    pub fn on_waterfall<E: Event>(
        &self,
        f: impl Fn(E::Data, &dyn Fn(E::Data) -> E::Data) -> E::Data + Send + Sync + 'static,
    ) -> Disposer {
        assert!(
            E::DISPATCH == Dispatch::Waterfall,
            "派发模式即公开契约：{} 标注 {:?}，不能用 on_waterfall 监听",
            E::NAME,
            E::DISPATCH
        );
        let f: WaterfallListener<E::Data> = Arc::new(f);
        self.add_listener::<E>("on_waterfall", Arc::new(f))
    }

    /// 委托链派发：注册序外层优先，next 一路向内；不调 next 即否决
    pub fn waterfall<E: Event>(&self, data: E::Data) -> E::Data {
        assert!(
            E::DISPATCH == Dispatch::Waterfall,
            "派发模式即公开契约：{} 标注 {:?}，不能用 waterfall 派发",
            E::NAME,
            E::DISPATCH
        );
        gate(&self.core, &self.owner, "waterfall");
        let listeners = self.snapshot::<E>();
        waterfall_run::<E>(&listeners, 0, data)
    }

    /// 观察等价判据用：本 realm 该事件的监听器数
    pub fn listener_count<E: Event>(&self) -> usize {
        let c = self.lock();
        c.events
            .get(&TypeId::of::<E>())
            .map(|list| list.iter().filter(|l| l.realm == self.realm).count())
            .unwrap_or(0)
    }
}

/// 委托链的递归求值：第 idx 棒收到 (data, next)，next 即「从 idx+1 续跑」
fn waterfall_run<E: Event>(
    listeners: &[Arc<dyn Any + Send + Sync>],
    idx: usize,
    data: E::Data,
) -> E::Data {
    let Some(f) = listeners.get(idx) else {
        return data;
    };
    let f = f
        .clone()
        .downcast::<WaterfallListener<E::Data>>()
        .expect("Waterfall 监听签名不符");
    f(data, &|d| waterfall_run::<E>(listeners, idx + 1, d))
}
