//! conn_provider_spec.rs — 连接 provider 插件契约考题（A 档）
//!
//! 约束对象：`src/plugins/conn_provider_ws.rs` + `src/conn.rs` 工厂层
//! （`ConnConfig`/`TermHandle`/`TermFactory`/`Spawner`）。
//! 依据：设计页 `/root/kfmv4/experiments/dsh-na/na/connection-provider.md` §8
//! 考题 5-9 + 评审回信裁决（假 transport 注入、unload 不断连、reload 钉旧句柄）。
//!
//! 判卷维度：注册成功 / 事件桥收敛 / 卸载回滚（观察等价 + 句柄存活）/
//! reload 换新工厂 / 注册冲突失败隔离。
//!
//! 假 transport 纪律（评审裁决 4）：不开真实 ws、不起 tokio 多线程；
//! 假 spawner 用 std::thread + mpsc 跨线程喂 SessionEvent——与 conn.rs 现状
//! 同构（真实路径事件也来自另一线程的 mpsc），判卷稳定零网络依赖。
//! 真实 ws 路径归 live 题 + C 档实拍管，本文件不碰。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kfm_na::base::Ctx;
use kfm_na::base::{Base, FiberState, GetError, Plugin, PluginEntry, ServiceKey};
use kfm_na::conn::{ConnConfig, Spawner, TermCmd, TermFactory, TermHandle};
use kfm_na::plugins::conn_provider_ws::ConnProviderWs;
use kfm_na::session::SessionEvent;

/// 假 transport：记录每次 spawn 的配置；开线程回显——收到 Input 原文回 Output，
/// 收到 Close 收工。跨线程 mpsc 交付，与真实 ws 路径同构。
fn fake_spawner(log: Arc<Mutex<Vec<ConnConfig>>>) -> Spawner {
    Arc::new(move |cfg: ConnConfig| {
        log.lock().expect("log 锁").push(cfg);
        let (out_tx, out_rx) = mpsc::channel::<TermCmd>();
        let (ev_tx, ev_rx) = mpsc::channel::<SessionEvent>();
        std::thread::spawn(move || {
            if ev_tx
                .send(SessionEvent::Opened {
                    session_id: "fake-session".into(),
                })
                .is_err()
            {
                return;
            }
            while let Ok(cmd) = out_rx.recv() {
                match cmd {
                    TermCmd::Input(s) => {
                        if ev_tx.send(SessionEvent::Output { data: s }).is_err() {
                            break;
                        }
                    }
                    TermCmd::Close => break,
                    TermCmd::Resize { .. } => {}
                }
            }
        });
        TermHandle {
            outbound: out_tx,
            events: ev_rx,
        }
    })
}

fn entry_with(url: &str) -> PluginEntry {
    let url = url.to_string();
    PluginEntry {
        id: "conn-provider-ws",
        disabled: false,
        config: Some(Box::new(move || {
            Arc::new(ConnConfig {
                url: url.clone(),
                command: None,
            }) as Arc<dyn std::any::Any + Send + Sync>
        })),
    }
}

fn recv_timeout(h: &TermHandle) -> SessionEvent {
    h.events
        .recv_timeout(Duration::from_secs(2))
        .expect("2s 内应收到事件")
}

/// 考题 5：注册成功——apply 后 ctx 可取回工厂；工厂配置来自启动配置表
/// （§4.1 配置延迟解析：PluginEntry.config → 工厂 default_config）
#[test]
fn spec_注册成功_工厂可取回且配置来自配置表() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let base = Base::new(vec![entry_with("ws://fake-host:9/ws")]);
    base.load(ConnProviderWs::with_spawner(fake_spawner(log.clone())))
        .expect("装载应成功");
    assert_eq!(
        base.state("conn-provider-ws"),
        Some(FiberState::Active),
        "apply 只注册不真连，应瞬时 Active"
    );

    let factory = base
        .ctx()
        .get::<dyn TermFactory>()
        .expect("注册表式服务键应可取回");
    assert_eq!(factory.default_config().url, "ws://fake-host:9/ws");

    let _handle = factory.spawn(&factory.default_config());
    let log = log.lock().expect("log 锁");
    assert_eq!(log.len(), 1, "spawn 应真正调用一次 transport");
    assert_eq!(log[0].url, "ws://fake-host:9/ws", "配置应传到 transport");
}

/// 考题 5b：无配置条目时走默认（url = 现状 8021 回环，行为零变化的锚）
#[test]
fn spec_无配置条目_默认即现状() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let base = Base::new(vec![]);
    base.load(ConnProviderWs::with_spawner(fake_spawner(log)))
        .expect("装载应成功");
    let factory = base.ctx().get::<dyn TermFactory>().expect("应可取回");
    assert_eq!(factory.default_config(), ConnConfig::default());
    assert_eq!(factory.default_config().url, "ws://127.0.0.1:8021/ws");
}

/// 考题 6：事件桥收敛——调用方不传 inbound 闭包，经 TermHandle.events 收事件；
/// outbound 发 Input 能收到回显 Output（双向通道都通）
#[test]
fn spec_事件桥收敛_句柄双向可用() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let base = Base::new(vec![entry_with("ws://fake/ws")]);
    base.load(ConnProviderWs::with_spawner(fake_spawner(log)))
        .unwrap();
    let factory = base.ctx().get::<dyn TermFactory>().unwrap();
    let handle = factory.spawn(&factory.default_config());

    assert_eq!(
        recv_timeout(&handle),
        SessionEvent::Opened {
            session_id: "fake-session".into()
        },
        "Opened 应经 events 通道到达（无需调用方建桥）"
    );
    handle
        .outbound
        .send(TermCmd::Input("echo hi\n".into()))
        .expect("outbound 应可发");
    assert_eq!(
        recv_timeout(&handle),
        SessionEvent::Output {
            data: "echo hi\n".into()
        },
        "Input 应触发回显 Output"
    );
}

/// 考题 7：卸载回滚（观察等价）+ 句柄存活——unload 后工厂取回失败，
/// 但卸载前已创建的 TermHandle 照常双向收发（连接不随插件死，评审裁决 2）
#[test]
fn spec_卸载后_工厂消失但句柄存活() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let base = Base::new(vec![entry_with("ws://fake/ws")]);
    base.load(ConnProviderWs::with_spawner(fake_spawner(log)))
        .unwrap();
    let factory = base.ctx().get::<dyn TermFactory>().unwrap();
    let handle = factory.spawn(&factory.default_config());
    assert_eq!(
        recv_timeout(&handle),
        SessionEvent::Opened {
            session_id: "fake-session".into()
        }
    );

    base.unload("conn-provider-ws");
    assert!(
        matches!(
            base.ctx().get::<dyn TermFactory>(),
            Err(GetError::DeclaredButInactive(_))
        ),
        "卸载后新调用方应取不到工厂（声明过但未激活）"
    );

    // 存量句柄：连接是调用方持有的长寿命状态，不随插件卸载死
    handle
        .outbound
        .send(TermCmd::Input("still alive\n".into()))
        .expect("卸载后 outbound 仍应可发");
    assert_eq!(
        recv_timeout(&handle),
        SessionEvent::Output {
            data: "still alive\n".into()
        },
        "卸载后已连会话不受影响"
    );
}

/// 考题 8：换 provider 实例（独占绑定语义）——reload 后新工厂可用、
/// 新 spawn 走新实例，旧 TermHandle 收发不受新工厂影响（评审裁决 3 实现注记）
#[test]
fn spec_reload_新工厂可用_旧句柄不受影响() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let base = Base::new(vec![entry_with("ws://fake/ws")]);
    base.load(ConnProviderWs::with_spawner(fake_spawner(log.clone())))
        .unwrap();
    let factory_v1 = base.ctx().get::<dyn TermFactory>().unwrap();
    let old_handle = factory_v1.spawn(&factory_v1.default_config());
    assert!(matches!(
        recv_timeout(&old_handle),
        SessionEvent::Opened { .. }
    ));

    base.reload("conn-provider-ws");
    assert_eq!(
        base.state("conn-provider-ws"),
        Some(FiberState::Active),
        "reload 后应重新 Active"
    );
    let factory_v2 = base.ctx().get::<dyn TermFactory>().unwrap();
    let new_handle = factory_v2.spawn(&factory_v2.default_config());
    assert!(matches!(
        recv_timeout(&new_handle),
        SessionEvent::Opened { .. }
    ));
    assert_eq!(
        log.lock().expect("log 锁").len(),
        2,
        "两次 spawn 各走一次 transport"
    );

    // 旧句柄的通道是裸 mpsc，不依赖工厂闭包里的任何可蒸发状态
    old_handle
        .outbound
        .send(TermCmd::Input("old still ok\n".into()))
        .unwrap();
    assert_eq!(
        recv_timeout(&old_handle),
        SessionEvent::Output {
            data: "old still ok\n".into()
        },
        "reload 后旧句柄收发不受新工厂影响"
    );
}

/// 考题 9：注册冲突——第二个同名服务键注册 → apply 返回 Err，插件 Failed，
/// 基座按 serial+bail 停该链（v1.1：失败钉死、不传染兄弟、先到者服务不变）
#[test]
fn spec_注册冲突_后者failed不传染() {
    struct Squatter;
    impl Plugin for Squatter {
        fn name(&self) -> &'static str {
            "conn-provider-squatter"
        }
        fn provides(&self) -> Vec<ServiceKey> {
            vec![ServiceKey::of::<dyn TermFactory>()]
        }
        fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
            let undo = ctx
                .provide::<dyn TermFactory>(Arc::new(DummyFactory))
                .map_err(|e| format!("占位者注册失败: {e:?}"))?;
            ctx.effect(undo);
            Ok(())
        }
    }
    struct DummyFactory;
    impl TermFactory for DummyFactory {
        fn default_config(&self) -> ConnConfig {
            ConnConfig::default()
        }
        fn spawn(&self, _config: &ConnConfig) -> TermHandle {
            panic!("占位者不该被调用")
        }
    }

    let log = Arc::new(Mutex::new(Vec::new()));
    let base = Base::new(vec![entry_with("ws://fake/ws")]);
    base.load(ConnProviderWs::with_spawner(fake_spawner(log)))
        .unwrap();
    base.load(Squatter).expect("load 本身不报错");

    assert!(
        matches!(
            base.state("conn-provider-squatter"),
            Some(FiberState::Inactive(kfm_na::base::Idle::Failed(_)))
        ),
        "冲突者应钉死 Failed"
    );
    assert_eq!(
        base.state("conn-provider-ws"),
        Some(FiberState::Active),
        "先到者不受传染"
    );
    // 服务仍是先到者的工厂（squatter 的 DummyFactory 一调就 panic）
    let factory = base.ctx().get::<dyn TermFactory>().unwrap();
    let handle = factory.spawn(&factory.default_config());
    assert!(matches!(recv_timeout(&handle), SessionEvent::Opened { .. }));
}
