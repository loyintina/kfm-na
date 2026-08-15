//! base_spec.rs — 插件基座最小核心考题（A 档契约层，17 道，答案 src/base/）
//!
//! 契约来源：plugin-architecture-spec.md v1.1 §4 全部条款 + 跨线信箱
//! （kfmv4 docs/ledger/agent-inbox/）评审裁决六条（全同步 / serial+bail 合一 /
//! PENDING 条件态 / 瞬时返回契约）。
//!
//! 分层说明（评审裁决 4-④）：规格书 §5 是四层测试体系，本文件只含第一层
//! 「契约测试」。第二层「互操作组合矩阵」（多插件同挂边界，按 inject 依赖图
//! 生成）**另立考题文件**，不在本文件。

use kfm_na::base::{
    Base, BaseWarning, Dispatch, Event, FiberState, GetError, Idle, LoadError, Plugin, PluginEntry,
    ProvideError, ROOT_REALM, ServiceKey,
};
use kfm_na::base::{Ctx, EffectStack};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ================= 假服务与假插件脚手架 =================

/// 假服务一号：计数器（实例 id 用于钉「换提供者实例」）
trait Counter: Send + Sync {
    fn id(&self) -> u64;
    fn bump(&self);
    fn count(&self) -> u64;
}
struct CounterImpl {
    id: u64,
    n: AtomicU64,
}
impl Counter for CounterImpl {
    fn id(&self) -> u64 {
        self.id
    }
    fn bump(&self) {
        self.n.fetch_add(1, Ordering::SeqCst);
    }
    fn count(&self) -> u64 {
        self.n.load(Ordering::SeqCst)
    }
}
fn counter_key() -> ServiceKey {
    ServiceKey::of::<dyn Counter>()
}

/// 假服务二号：种子（依赖环 / epoch 链的上一级）
trait Seed: Send + Sync {
    fn id(&self) -> u64;
}
struct SeedImpl(u64);
impl Seed for SeedImpl {
    fn id(&self) -> u64 {
        self.0
    }
}
fn seed_key() -> ServiceKey {
    ServiceKey::of::<dyn Seed>()
}

/// 假事件三枚：三种派发各一
struct Ping;
impl Event for Ping {
    const NAME: &'static str = "ping";
    const DISPATCH: Dispatch = Dispatch::Emit;
    type Data = u64;
}
struct Guard;
impl Event for Guard {
    const NAME: &'static str = "guard";
    const DISPATCH: Dispatch = Dispatch::Serial;
    type Data = String;
}
struct Pipe;
impl Event for Pipe {
    const NAME: &'static str = "pipe";
    const DISPATCH: Dispatch = Dispatch::Waterfall;
    type Data = Vec<String>;
}

/// 万能假插件：provides/deps/apply 全由构造者注入
type ApplyFn = Box<dyn Fn(&mut Ctx) -> Result<(), String> + Send + Sync>;
struct FakePlugin {
    name: &'static str,
    provides: Vec<ServiceKey>,
    deps: Vec<ServiceKey>,
    apply: ApplyFn,
}
impl Plugin for FakePlugin {
    fn name(&self) -> &'static str {
        self.name
    }
    fn provides(&self) -> Vec<ServiceKey> {
        self.provides.clone()
    }
    fn deps(&self) -> Vec<ServiceKey> {
        self.deps.clone()
    }
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        (self.apply)(ctx)
    }
}
fn fake(
    name: &'static str,
    provides: Vec<ServiceKey>,
    deps: Vec<ServiceKey>,
    apply: impl Fn(&mut Ctx) -> Result<(), String> + Send + Sync + 'static,
) -> FakePlugin {
    FakePlugin {
        name,
        provides,
        deps,
        apply: Box::new(apply),
    }
}

/// 标准计数器提供者：provide 的撤销条按纪律注册进效果栈。
/// （「忘回滚」是考题 2 变异样本的病灶，不是常态写法）
fn provider_plugin(name: &'static str, ids: Arc<AtomicU64>) -> FakePlugin {
    fake(name, vec![counter_key()], vec![], move |ctx| {
        let id = ids.fetch_add(1, Ordering::SeqCst) + 1;
        let svc: Arc<dyn Counter> = Arc::new(CounterImpl {
            id,
            n: AtomicU64::new(0),
        });
        let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
        ctx.effect(undo);
        Ok(())
    })
}

/// 标准计数器消费者：apply 里 get 一次并记录实例 id
fn counter_consumer(name: &'static str, seen: Arc<Mutex<Vec<u64>>>) -> FakePlugin {
    fake(name, vec![], vec![counter_key()], move |ctx| {
        let c = ctx.get::<dyn Counter>().map_err(|e| format!("{e:?}"))?;
        seen.lock().unwrap().push(c.id());
        Ok(())
    })
}

/// 依赖者（带撤销记录）：dep Counter，注册一条记录型效果
fn dependent_with_log(
    name: &'static str,
    log: &Arc<Mutex<Vec<&'static str>>>,
    tag: &'static str,
) -> FakePlugin {
    let log = log.clone();
    fake(name, vec![], vec![counter_key()], move |ctx| {
        ctx.get::<dyn Counter>().map_err(|e| format!("{e:?}"))?;
        let log = log.clone();
        ctx.effect(Box::new(move || log.lock().unwrap().push(tag)));
        Ok(())
    })
}

/// 观察等价快照（规格书 §5 修订 8）：卸载后经由 ctx 可观察的行为
/// 与加载前不可区分——按值比较服务表/监听表内容，不比句柄 id
fn snapshot(base: &Base) -> (usize, usize) {
    (
        base.service_count(ROOT_REALM),
        base.ctx().events.listener_count::<Ping>(),
    )
}

// ================= 考题 1：注册-卸载回滚（观察等价判据）=================
// 契约（§5 修订 8 + §4.1 承诺边界）：卸载后服务表/监听表与加载前逐值相等，
// get 回落到 DeclaredButInactive，监听器不再触发。【变异抽检对象】
#[test]
fn spec_base_01_注册卸载回滚_观察等价() {
    let base = Base::new(vec![]);
    let before = snapshot(&base);

    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let counter: Arc<dyn Counter> = Arc::new(CounterImpl {
        id: 1,
        n: AtomicU64::new(0),
    });
    let p = {
        let counter = counter.clone();
        let log = log.clone();
        fake("p", vec![counter_key()], vec![], move |ctx| {
            // 效果一：注册服务（获取类，可回滚）
            let undo = ctx.provide(counter.clone()).map_err(|e| format!("{e:?}"))?;
            ctx.effect(undo);
            // 效果二：注册监听器（获取类，可回滚）
            let c = counter.clone();
            let off = ctx.events.on_emit::<Ping>(move |&_| c.bump());
            ctx.effect(off);
            // 效果三：裸效果
            let log = log.clone();
            ctx.effect(Box::new(move || log.lock().unwrap().push("e3-undo")));
            Ok(())
        })
    };
    base.load(p).unwrap();
    assert_eq!(base.state("p"), Some(FiberState::Active));
    assert_eq!(base.ctx().get::<dyn Counter>().unwrap().id(), 1);
    base.ctx().events.emit::<Ping>(&7);
    assert_eq!(counter.count(), 1, "加载后监听器必须生效");

    base.unload("p");
    assert_eq!(base.state("p"), Some(FiberState::Inactive(Idle::Clean)));
    assert_eq!(
        snapshot(&base),
        before,
        "观察等价：卸载后服务表/监听表必须与加载前不可区分"
    );
    assert!(matches!(
        base.ctx().get::<dyn Counter>(),
        Err(GetError::DeclaredButInactive(_))
    ));
    base.ctx().events.emit::<Ping>(&1);
    assert_eq!(counter.count(), 1, "卸载后监听器必须已摘除");
    assert!(
        log.lock().unwrap().contains(&"e3-undo"),
        "裸效果的逆元必须已运行"
    );
}

// ================= 考题 2：忘回滚变异（判卷必须咬人）=================
// 契约（§4.3：disposer 正确性 = 插件作者义务 + 测试强制）：基座无法运行时
// 验证 witness，「忘回滚 → 观察等价判据红」是唯一执行点。本题自带变异样本：
// 同一个判卷器，对守规矩插件判过、对忘回滚插件必须判挂。
#[test]
fn spec_base_02_忘回滚变异_判卷咬人() {
    // 正样本：守规矩插件 → 判卷通过
    let base_ok = Base::new(vec![]);
    let before_ok = snapshot(&base_ok);
    base_ok
        .load(provider_plugin("good", Arc::new(AtomicU64::new(0))))
        .unwrap();
    base_ok.unload("good");
    assert_eq!(snapshot(&base_ok), before_ok, "守规矩插件必须判过");

    // 变异样本：忘回滚插件——provide 的撤销条拿到手直接丢
    let base_bad = Base::new(vec![]);
    let before_bad = snapshot(&base_bad);
    let leaky = fake("leaky", vec![counter_key()], vec![], |ctx| {
        let svc: Arc<dyn Counter> = Arc::new(CounterImpl {
            id: 9,
            n: AtomicU64::new(0),
        });
        let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
        drop(undo); // 病灶：撤销条没注册进效果栈，卸载时无人摘除服务
        let off = ctx.events.on_emit::<Ping>(|_| {});
        drop(off); // 病灶同上：监听器也不摘
        Ok(())
    });
    base_bad.load(leaky).unwrap();
    base_bad.unload("leaky");
    assert_ne!(
        snapshot(&base_bad),
        before_bad,
        "判卷必须咬住忘回滚：卸载后残留必须可观察"
    );
    assert!(
        base_bad.service_count(ROOT_REALM) == 1,
        "泄漏的服务条目仍占着注册表——这正是判据要抓的残迹\
         （停供相使新 get 已失败，但条目没摘除 = 忘回滚的铁证）"
    );
}

// ================= 考题 3：LIFO 逆序 =================
// 契约（§4.3 可逆效果 + Theo 16）：卸载逆序 unwind，最新注册的先回滚。
// 【变异抽检对象】
#[test]
fn spec_base_03_效果栈_lifo逆序() {
    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let mut stack = EffectStack::new();
    for tag in ["first", "second", "third"] {
        let log = log.clone();
        stack.push(Box::new(move || log.lock().unwrap().push(tag)));
    }
    stack.dispose();
    assert_eq!(
        *log.lock().unwrap(),
        vec!["third", "second", "first"],
        "必须 LIFO 逆序回滚"
    );
}

// ================= 考题 4：PENDING 合法态 =================
// 契约（§4.3 激活语义 + v1.1 措辞裁决 5）：依赖未齐 = Inactive(Clean) 下
// target≠⊥ 的条件态，不是错误、不报错；服务上线 → 依赖者自动激活。
#[test]
fn spec_base_04_pending合法态_上线自动激活() {
    let base = Base::new(vec![]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    base.load(counter_consumer("c", seen.clone())).unwrap();
    assert_eq!(
        base.state("c"),
        Some(FiberState::Inactive(Idle::Clean)),
        "依赖缺失是合法挂起，不许报错、不许 Failed"
    );
    assert!(
        base.is_pending("c"),
        "PENDING = Inactive(Clean) 且 target≠⊥"
    );
    assert!(seen.lock().unwrap().is_empty(), "挂起期 apply 不许运行");

    base.load(provider_plugin("p", Arc::new(AtomicU64::new(0))))
        .unwrap();
    assert_eq!(
        base.state("c"),
        Some(FiberState::Active),
        "依赖上线必须自动激活"
    );
    assert_eq!(*seen.lock().unwrap(), vec![1]);
}

// ================= 考题 5：epoch 重载 =================
// 契约（§4.3）：epoch = 依赖提供者实例 id 串接签名；换提供者实例 →
// 依赖者自动重载。链 D(Seed)→P(Counter)→C：D 换实例级联到 C。
// 【变异抽检对象】
#[test]
fn spec_base_05_epoch签名_换提供者实例触发依赖者重载() {
    let base = Base::new(vec![]);
    let d_ids = Arc::new(AtomicU64::new(0));
    let p_ids = Arc::new(AtomicU64::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seed_seen = Arc::new(Mutex::new(Vec::new()));

    let d = {
        let d_ids = d_ids.clone();
        fake("d", vec![seed_key()], vec![], move |ctx| {
            let id = d_ids.fetch_add(1, Ordering::SeqCst) + 1;
            let svc: Arc<dyn Seed> = Arc::new(SeedImpl(id));
            let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
            ctx.effect(undo);
            Ok(())
        })
    };
    let p = {
        let p_ids = p_ids.clone();
        let seed_seen = seed_seen.clone();
        fake("p", vec![counter_key()], vec![seed_key()], move |ctx| {
            let seed = ctx.get::<dyn Seed>().map_err(|e| format!("{e:?}"))?;
            seed_seen.lock().unwrap().push(seed.id());
            let id = p_ids.fetch_add(1, Ordering::SeqCst) + 1;
            let svc: Arc<dyn Counter> = Arc::new(CounterImpl {
                id,
                n: AtomicU64::new(0),
            });
            let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
            ctx.effect(undo);
            Ok(())
        })
    };
    base.load(d).unwrap();
    base.load(p).unwrap();
    base.load(counter_consumer("c", seen.clone())).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![1],
        "C 首次激活读到 P 的第一个实例"
    );

    base.reload("d");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![1, 2],
        "提供者实例更换必须级联重载到二级依赖者，且读到新实例"
    );
    assert_eq!(
        *seed_seen.lock().unwrap(),
        vec![1, 2],
        "epoch 签名变：P 重载后读到的必须是 D 的新实例"
    );
    assert_eq!(p_ids.load(Ordering::SeqCst), 2, "P 必须被重载一次");
    assert_eq!(base.state("c"), Some(FiberState::Active));
}

// ================= 考题 6：卸载拓扑序（三相）=================
// 契约（§4.3 卸载协议三相）：停供 → 依赖者排空 → LIFO disposers。
// 提供者的逆元必须在所有依赖者的逆元之后运行。【变异抽检对象】
#[test]
fn spec_base_06_卸载拓扑序_依赖者先撤完() {
    let base = Base::new(vec![]);
    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let ids = Arc::new(AtomicU64::new(0));
    let p = {
        let log = log.clone();
        fake("p", vec![counter_key()], vec![], move |ctx| {
            let id = ids.fetch_add(1, Ordering::SeqCst) + 1;
            let svc: Arc<dyn Counter> = Arc::new(CounterImpl {
                id,
                n: AtomicU64::new(0),
            });
            let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
            ctx.effect(undo);
            let log = log.clone();
            ctx.effect(Box::new(move || log.lock().unwrap().push("P.d")));
            Ok(())
        })
    };
    base.load(p).unwrap();
    base.load(dependent_with_log("a", &log, "A.d")).unwrap();
    base.load(dependent_with_log("b", &log, "B.d")).unwrap();

    base.unload("p");
    let log = log.lock().unwrap();
    let pos = |t: &str| log.iter().position(|x| *x == t).expect("逆元必须运行");
    assert!(
        pos("A.d") < pos("P.d") && pos("B.d") < pos("P.d"),
        "依赖者必须先于提供者撤完：{log:?}"
    );
    drop(log);
    assert_eq!(
        base.state("a"),
        Some(FiberState::Inactive(Idle::Clean)),
        "被波及的依赖者落在 Clean（target 仍真，等服务回来），不是 Failed"
    );
}

// ================= 考题 7：失败隔离 =================
// 契约（§4.4 单插件隔离）：apply 失败 → 已累积逆元全部回滚后钉 Failed；
// 失败记在 fiber 上，不传播父级，兄弟照常跑，依赖者合法挂起。
#[test]
fn spec_base_07_失败隔离_回滚已累积逆元() {
    let base = Base::new(vec![]);
    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let f = {
        let log = log.clone();
        fake("f", vec![counter_key()], vec![], move |ctx| {
            let log = log.clone();
            ctx.effect(Box::new(move || log.lock().unwrap().push("f.e1-undo")));
            let svc: Arc<dyn Counter> = Arc::new(CounterImpl {
                id: 1,
                n: AtomicU64::new(0),
            });
            let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
            ctx.effect(undo);
            Err("boom".to_string())
        })
    };
    base.load(f).unwrap();
    // 兄弟：与 f 无依赖关系的独立插件
    base.load(fake("sibling", vec![], vec![], |_ctx| Ok(())))
        .unwrap();
    // 依赖失败者的插件
    let seen = Arc::new(Mutex::new(Vec::new()));
    base.load(counter_consumer("c", seen)).unwrap();

    match base.state("f") {
        Some(FiberState::Inactive(Idle::Failed(msg))) => {
            assert!(msg.contains("boom"), "失败原因必须记进 fiber：{msg}")
        }
        other => panic!("失败必须钉死 Failed 终态，实际 {other:?}"),
    }
    assert_eq!(
        base.state("sibling"),
        Some(FiberState::Active),
        "失败不传染：兄弟照常跑"
    );
    assert!(
        log.lock().unwrap().contains(&"f.e1-undo"),
        "失败前已累积的逆元必须全部回滚"
    );
    assert!(matches!(
        base.ctx().get::<dyn Counter>(),
        Err(GetError::DeclaredButInactive(_)),
    ));
    assert_eq!(
        base.state("c"),
        Some(FiberState::Inactive(Idle::Clean)),
        "失败不传染：依赖者合法挂起，不是 Failed"
    );
}

// ================= 考题 8：失败不重试 =================
// 契约（§4.4 永久 Failed）：L-Begin 以 Inactive(⊥) 为前提，失败 fiber
// 永不自动重试；恢复只能靠显式编排动作（reload）。
#[test]
fn spec_base_08_失败钉死_永不自动重试() {
    let base = Base::new(vec![]);
    let apply_count = Arc::new(AtomicU64::new(0));
    let f = {
        let apply_count = apply_count.clone();
        fake("f", vec![], vec![], move |_ctx| {
            apply_count.fetch_add(1, Ordering::SeqCst);
            Err("boom".to_string())
        })
    };
    base.load(f).unwrap();
    assert_eq!(apply_count.load(Ordering::SeqCst), 1);

    // 搅动基座：加载/卸载/重载无关插件，触发多轮 refresh
    base.load(provider_plugin("x", Arc::new(AtomicU64::new(0))))
        .unwrap();
    base.unload("x");
    base.load(provider_plugin("y", Arc::new(AtomicU64::new(0))))
        .unwrap();
    base.reload("y");
    assert_eq!(
        apply_count.load(Ordering::SeqCst),
        1,
        "环境扰动不许触发自动重试"
    );
    assert!(matches!(
        base.state("f"),
        Some(FiberState::Inactive(Idle::Failed(_)))
    ));

    // 显式恢复是允许的（配置变更等编排动作）
    base.reload("f");
    assert_eq!(
        apply_count.load(Ordering::SeqCst),
        2,
        "显式 reload 必须重试"
    );
    assert!(
        matches!(base.state("f"), Some(FiberState::Inactive(Idle::Failed(_)))),
        "重试再败仍钉 Failed"
    );
}

// ================= 考题 9：dispose 幂等（take-once）=================
// 契约（§4.3 dispose 幂等）：dispose 触发两次 = 在没有应用产生的状态上跑
// 逆元，必须至多一次。附：子 ctx 撤销条贴父栈（树形级联）。
#[test]
fn spec_base_09_dispose幂等_takeonce与树形级联() {
    // 栈级：dispose 两次，逆元只跑一次
    let count = Arc::new(AtomicU64::new(0));
    let mut stack = EffectStack::new();
    {
        let count = count.clone();
        stack.push(Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
    }
    stack.dispose();
    stack.dispose();
    assert_eq!(count.load(Ordering::SeqCst), 1, "take-once：至多一次");
    assert!(stack.is_disposed());

    // fiber 级：unload 两次幂等；且子 ctx 的效果级联进父栈
    let base = Base::new(vec![]);
    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let p = {
        let log = log.clone();
        fake("p", vec![], vec![], move |ctx| {
            let l = log.clone();
            ctx.effect(Box::new(move || l.lock().unwrap().push("p.d")));
            // 子 ctx（独立 realm 作用域）：其撤销条自动贴进父栈
            let child = ctx.fork(9);
            let l = log.clone();
            child.effect(Box::new(move || l.lock().unwrap().push("child.d")));
            Ok(())
        })
    };
    base.load(p).unwrap();
    base.unload("p");
    base.unload("p");
    let log = log.lock().unwrap();
    assert_eq!(
        log.iter().filter(|t| **t == "p.d").count(),
        1,
        "重复 unload 不许重跑逆元"
    );
    assert!(
        log.contains(&"child.d"),
        "子 ctx 的逆元必须随父纤维级联运行：{log:?}"
    );
}

// ================= 考题 10：依赖环注册期报错 =================
// 契约（§4.3 依赖环条款）：环 = 双方永久 Inactive 从声明即可预测——
// 基座在注册期拓扑检测，环 → 明确报错，不许静默双挂起。
#[test]
fn spec_base_10_依赖环_注册期报错() {
    let base = Base::new(vec![]);
    let a = fake("a", vec![counter_key()], vec![seed_key()], |_ctx| Ok(()));
    let b = fake("b", vec![seed_key()], vec![counter_key()], |_ctx| Ok(()));
    base.load(a).unwrap();
    match base.load(b) {
        Err(LoadError::CycleDetected(path)) => {
            assert!(
                path.contains(&"a") && path.contains(&"b"),
                "报错必须点名环上的插件：{path:?}"
            );
        }
        other => panic!("环必须注册期报错，实际 {other:?}"),
    }
    assert_eq!(base.state("b"), None, "成环插件不许残留（静默挂起）");
    assert!(base.is_pending("a"), "先到的 a 仍是合法挂起");
}

// ================= 考题 11：单一来源 =================
// 契约（§4.2 修订 4）：同名服务二次 provide = 错误，registry 拒绝，
// 绝不覆盖，原绑定不动。
#[test]
fn spec_base_11_单一来源_二次provide拒绝() {
    let base = Base::new(vec![]);
    let ctx = base.ctx();
    let c1: Arc<dyn Counter> = Arc::new(CounterImpl {
        id: 1,
        n: AtomicU64::new(0),
    });
    let _undo = ctx.provide(c1).unwrap();
    let c2: Arc<dyn Counter> = Arc::new(CounterImpl {
        id: 2,
        n: AtomicU64::new(0),
    });
    assert_eq!(
        ctx.provide(c2).err(),
        Some(ProvideError::AlreadyProvided(counter_key())),
        "二次 provide 必须拒绝"
    );
    assert_eq!(
        ctx.get::<dyn Counter>().unwrap().id(),
        1,
        "原绑定必须原样不动（绝不覆盖）"
    );
}

// ================= 考题 12：isolate 过滤 =================
// 契约（§4.3 作用域）：realm 是按服务键的作用域表；notify 带 isolate
// 过滤——会话 A 的服务上下线不惊动会话 B；跨 realm 服务不可见。
#[test]
fn spec_base_12_isolate过滤_跨realm不通知() {
    const A: u64 = 1;
    const B: u64 = 2;
    let base = Base::new(vec![]);
    let seen_a = Arc::new(Mutex::new(Vec::new()));
    let seen_b = Arc::new(Mutex::new(Vec::new()));
    base.load_in(provider_plugin("p", Arc::new(AtomicU64::new(0))), A)
        .unwrap();
    base.load_in(counter_consumer("ca", seen_a.clone()), A)
        .unwrap();
    base.load_in(counter_consumer("cb", seen_b.clone()), B)
        .unwrap();

    assert_eq!(*seen_a.lock().unwrap(), vec![1], "同 realm 依赖者正常激活");
    assert!(
        seen_b.lock().unwrap().is_empty(),
        "realm B 看不见 realm A 的服务，合法挂起"
    );
    assert!(matches!(
        base.ctx_in(B).get::<dyn Counter>(),
        Err(GetError::DeclaredButInactive(_)),
    ));

    base.unload("p");
    assert_eq!(
        base.state("ca"),
        Some(FiberState::Inactive(Idle::Clean)),
        "同 realm 依赖者被波及卸载"
    );
    assert!(
        seen_b.lock().unwrap().is_empty() && base.is_pending("cb"),
        "跨 realm 零通知零扰动：B 的 fiber 纹丝不动"
    );
}

// ================= 考题 13：配置延迟解析 =================
// 契约（§4.1 配置延迟解析，cordis fiber.ts:740）：config 求值延迟到依赖
// 就绪之后；未激活的插件不解析配置；disabled 插件启动不装载。
#[test]
fn spec_base_13_配置延迟解析_未激活不求值() {
    struct Cfg {
        value: u64,
    }
    let parse_count = Arc::new(AtomicU64::new(0));
    let disabled_parse = Arc::new(AtomicU64::new(0));
    let config = vec![
        PluginEntry {
            id: "c",
            disabled: false,
            config: Some({
                let parse_count = parse_count.clone();
                Box::new(move || {
                    parse_count.fetch_add(1, Ordering::SeqCst);
                    Arc::new(Cfg { value: 42 }) as Arc<dyn std::any::Any + Send + Sync>
                })
            }),
        },
        PluginEntry {
            id: "d",
            disabled: true,
            config: Some({
                let disabled_parse = disabled_parse.clone();
                Box::new(move || {
                    disabled_parse.fetch_add(1, Ordering::SeqCst);
                    Arc::new(Cfg { value: 0 }) as Arc<dyn std::any::Any + Send + Sync>
                })
            }),
        },
    ];
    let base = Base::new(config);

    let used = Arc::new(Mutex::new(Vec::new()));
    let c = {
        let used = used.clone();
        fake("c", vec![], vec![counter_key()], move |ctx| {
            let cfg = ctx.config::<Cfg>().expect("激活时 config 必须已解析");
            used.lock().unwrap().push(cfg.value);
            Ok(())
        })
    };
    base.load(c).unwrap();
    assert!(base.is_pending("c"));
    assert_eq!(
        parse_count.load(Ordering::SeqCst),
        0,
        "依赖未就绪（未激活）不许解析 config"
    );

    base.load(provider_plugin("p", Arc::new(AtomicU64::new(0))))
        .unwrap();
    assert_eq!(base.state("c"), Some(FiberState::Active));
    assert_eq!(parse_count.load(Ordering::SeqCst), 1, "激活时解析一次");
    assert_eq!(*used.lock().unwrap(), vec![42], "apply 里读到的就是解析值");

    base.reload("c");
    assert_eq!(
        parse_count.load(Ordering::SeqCst),
        1,
        "配置启动读一次：重载不重复解析"
    );

    // disabled 位：启动不装载，config 更不许求值
    base.load(fake("d", vec![], vec![], |_ctx| Ok(()))).unwrap();
    assert_eq!(base.state("d"), Some(FiberState::Inactive(Idle::Clean)));
    assert!(!base.is_pending("d"), "disabled = target ⊥，不是 PENDING");
    assert_eq!(disabled_parse.load(Ordering::SeqCst), 0);
}

// ================= 考题 14：事件派发顺序 =================
// 契约（§4.3 事件派发 + 裁决 3）：派发模式即公开契约。v1 三派发：
// Emit 同步观察 / Serial 顺序短路（serial+bail 合一）/ Waterfall 委托链
// （不调 next 即否决，链序可判卷）。Parallel 缓建。
#[test]
fn spec_base_14_事件派发_顺序短路与waterfall序() {
    let base = Base::new(vec![]);
    let events = base.ctx().events;

    // Emit：同步观察，注册序，emit 返回时已全部跑完
    let elog = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    for tag in ["e1", "e2"] {
        let elog = elog.clone();
        let _off = events.on_emit::<Ping>(move |&_| elog.lock().unwrap().push(tag));
        std::mem::forget(_off); // 本考题不测摘除（考题 1 已钉），保持监听存活
    }
    events.emit::<Ping>(&1);
    assert_eq!(
        *elog.lock().unwrap(),
        vec!["e1", "e2"],
        "Emit 必须按注册序同步观察"
    );

    // Serial：顺序短路——第二棒返 Err，第三棒不许跑
    let slog = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    {
        let slog = slog.clone();
        let _o1 = events.on_serial::<Guard>(move |_d| {
            slog.lock().unwrap().push("S1");
            Ok(())
        });
        std::mem::forget(_o1);
    }
    {
        let slog = slog.clone();
        let _o2 = events.on_serial::<Guard>(move |_d| {
            slog.lock().unwrap().push("S2");
            Err("拦截".to_string())
        });
        std::mem::forget(_o2);
    }
    {
        let slog = slog.clone();
        let _o3 = events.on_serial::<Guard>(move |_d| {
            slog.lock().unwrap().push("S3");
            Ok(())
        });
        std::mem::forget(_o3);
    }
    let r = events.serial::<Guard>(&"payload".to_string());
    assert_eq!(r, Err("拦截".to_string()), "短路错误必须透传");
    assert_eq!(
        *slog.lock().unwrap(),
        vec!["S1", "S2"],
        "短路：S2 之后不许再跑"
    );

    // Waterfall：委托链——环绕语义（in/out 夹序），不调 next 即否决
    let _w1 = events.on_waterfall::<Pipe>(|mut v, next| {
        v.push("L1-in".to_string());
        let mut r = next(v);
        r.push("L1-out".to_string());
        r
    });
    std::mem::forget(_w1);
    let _w2 = events.on_waterfall::<Pipe>(|mut v, next| {
        v.push("L2-in".to_string());
        let mut r = next(v);
        r.push("L2-out".to_string());
        r
    });
    std::mem::forget(_w2);
    // 否决者：不调 next，链在此截断
    let _w3 = events.on_waterfall::<Pipe>(|mut v, _next| {
        v.push("L3-veto".to_string());
        v
    });
    std::mem::forget(_w3);
    let out = events.waterfall::<Pipe>(vec![]);
    assert_eq!(
        out,
        vec!["L1-in", "L2-in", "L3-veto", "L2-out", "L1-out"],
        "委托链必须环绕嵌套，且不调 next 的监听器截断后续"
    );
}

/// 派发模式即公开契约：用错的派发姿势监听 = 立即 panic（模式错配）
#[test]
#[should_panic(expected = "派发模式")]
fn spec_base_14b_事件模式错配_监听拒绝() {
    let base = Base::new(vec![]);
    let _off = base.ctx().events.on_emit::<Guard>(|_| {});
}

// ================= 考题 15：错误两分 =================
// 契约（§4.2 修订 10）：DeclaredButInactive（已声明未激活）vs
// Undeclared（未声明）——v1 运行期错误两分（编译期宏是实施期任务）。
#[test]
fn spec_base_15_get错误两分() {
    let base = Base::new(vec![]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    base.load(counter_consumer("c", seen)).unwrap(); // 声明了 Counter 但未激活
    assert_eq!(
        base.ctx().get::<dyn Counter>().err(),
        Some(GetError::DeclaredButInactive(counter_key())),
        "已声明未激活"
    );
    assert_eq!(
        base.ctx().get::<dyn Seed>().err(),
        Some(GetError::Undeclared(seed_key())),
        "从未声明"
    );
}

// ================= 考题 16：取消边界 =================
// 契约（§4.4 取消点 = 效果边界）：v1.1 全同步化后，apply 无半 await 状态，
// 取消点自动落在效果边界——本题钉的是同步化后的形态：**apply 完成后比对
// target，不变才激活**；若 apply 期间 target 被翻转为 ⊥，则回滚已注册
// 效果、落在 Inactive(Clean)，不进 Active。
#[test]
fn spec_base_16_取消边界_apply完成后比对target() {
    let base = Base::new(vec![]);
    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let p = {
        let log = log.clone();
        fake("selfcanceller", vec![counter_key()], vec![], move |ctx| {
            let svc: Arc<dyn Counter> = Arc::new(CounterImpl {
                id: 1,
                n: AtomicU64::new(0),
            });
            let undo = ctx.provide(svc).map_err(|e| format!("{e:?}"))?;
            ctx.effect(undo);
            let log = log.clone();
            ctx.effect(Box::new(move || log.lock().unwrap().push("sc-undo")));
            // apply 运行期间自己的 target 被翻转为 ⊥（v1 里等价于
            // 「in-flight apply 落地后 target 已变」的同步形态）
            ctx.set_plugin_target("selfcanceller", false);
            Ok(())
        })
    };
    base.load(p).unwrap();
    assert_eq!(
        base.state("selfcanceller"),
        Some(FiberState::Inactive(Idle::Clean)),
        "target 已变 → 回滚后落 Clean，不许进 Active"
    );
    assert!(
        log.lock().unwrap().contains(&"sc-undo"),
        "apply 期间注册的效果必须全部回滚"
    );
    assert!(matches!(
        base.ctx().get::<dyn Counter>(),
        Err(GetError::DeclaredButInactive(_)),
    ));
}

// ================= 考题 17：瞬时返回契约 =================
// 契约（§4.3 v1.1 瞬时返回契约）：生命周期转换跑在事件循环上，apply/unload
// 必须瞬时返回；慢活插件自开线程。本题形态设计（自决，理由如下）：
// **超阈值记报警（SlowApply），不记失败**。理由：① 同步基座无法中断
// in-flight apply，阻塞已发生，能做的是让它可观察（冻 UI 的来源可定位，
// 上层如 report.rs 可据此处置）；② Failed 终态属于 apply 返回 Err 的
// 情形（§4.4），「慢」不等于「错」（冷启动调度抖动），两语义不混；
// ③ 报警带 elapsed 证据，阈值可配（默认 50ms）。
#[test]
fn spec_base_17_瞬时返回契约_阻塞apply报警() {
    let base = Base::new(vec![]).with_apply_budget(Duration::from_millis(1));
    let slow = fake("slow", vec![], vec![], |_ctx| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(10) {
            std::hint::spin_loop();
        }
        Ok(())
    });
    base.load(slow).unwrap();
    assert_eq!(
        base.state("slow"),
        Some(FiberState::Active),
        "报警不记失败（理由见本题头注释）"
    );
    let warnings = base.warnings();
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            BaseWarning::SlowApply { plugin, elapsed }
                if *plugin == "slow" && *elapsed >= Duration::from_millis(1)
        )),
        "阻塞 apply 必须被检测并报警（带 elapsed 证据）：{warnings:?}"
    );

    // 对照组：正常插件在默认预算下不产生报警
    let base2 = Base::new(vec![]);
    base2
        .load(provider_plugin("fast", Arc::new(AtomicU64::new(0))))
        .unwrap();
    assert!(base2.warnings().is_empty(), "瞬时 apply 不许误报");
}
