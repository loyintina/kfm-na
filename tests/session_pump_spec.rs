//! tests/session_pump_spec.rs — 会话泵考题(2026-08-24 Output 数据面分家)
//!
//! 判的坑:挂起态事件循环不抽 Output,网格冻结——闸门读屏读到旧画面。
//! 分家后泵是唯一消费者:活跃方 Output 直接喂共享终端(谁调 pump 谁喂),
//! 待机方进 replay 缓存(切换时补屏),控制事件(Opened/Exited/Failed)
//! 一粒不动留给 UI 记健康账。全部 host 可判卷:mpsc 假通道 + Vec 假 sink。

use std::sync::mpsc::{Sender, channel};

use kfm_na::gate::SessionPump;
use kfm_na::session::SessionEvent;

/// 注册一条假会话,返回发送端
fn slot(pump: &mut SessionPump, name: &'static str) -> Sender<SessionEvent> {
    let (tx, rx) = channel();
    pump.register(name, rx);
    tx
}

fn out(s: &str) -> SessionEvent {
    SessionEvent::Output { data: s.into() }
}

/// 考题 1:活跃方 Output 直接进 sink,泵报「喂过」
#[test]
fn spec_pump_活跃输出进sink() {
    let mut pump = SessionPump::new();
    let local = slot(&mut pump, "local");
    local.send(out("hello")).unwrap();

    let mut fed: Vec<String> = Vec::new();
    let touched = pump.pump(
        "local",
        &mut |b| fed.push(String::from_utf8_lossy(b).into_owned()),
        &mut |_, _| {},
    );
    assert!(touched, "喂了活跃输出必须报 true");
    assert_eq!(fed, ["hello"]);
}

/// 考题 2:待机方 Output 不进 sink,进 replay 缓存(切换补屏的料)
#[test]
fn spec_pump_待机输出进缓存() {
    let mut pump = SessionPump::new();
    let _local = slot(&mut pump, "local");
    let remote = slot(&mut pump, "remote");
    remote.send(out("bg-1")).unwrap();
    remote.send(out("bg-2")).unwrap();

    let mut fed: Vec<String> = Vec::new();
    let touched = pump.pump(
        "local",
        &mut |b| fed.push(String::from_utf8_lossy(b).into_owned()),
        &mut |_, _| {},
    );
    assert!(!touched, "没喂活跃方不许报 true");
    assert!(fed.is_empty(), "待机输出一粒都不许进 sink");
    assert_eq!(pump.take_replay("remote"), ["bg-1", "bg-2"]);
}

/// 考题 3:活跃名翻面后路由跟随——原待机方改进 sink,原活跃方改进缓存
#[test]
fn spec_pump_翻面路由跟随() {
    let mut pump = SessionPump::new();
    let local = slot(&mut pump, "local");
    let remote = slot(&mut pump, "remote");

    let mut fed: Vec<String> = Vec::new();
    remote.send(out("now-active")).unwrap();
    local.send(out("now-standby")).unwrap();
    pump.pump(
        "remote",
        &mut |b| fed.push(String::from_utf8_lossy(b).into_owned()),
        &mut |_, _| {},
    );
    assert_eq!(fed, ["now-active"]);
    assert_eq!(pump.take_replay("local"), ["now-standby"]);
}

/// 考题 4:控制事件不进 sink 不进缓存,按名带进出控制队列(FIFO 保序)
#[test]
fn spec_pump_控制事件归控制队列() {
    let mut pump = SessionPump::new();
    let local = slot(&mut pump, "local");
    let remote = slot(&mut pump, "remote");
    local.send(out("x")).unwrap();
    local
        .send(SessionEvent::Opened {
            session_id: "local".into(),
        })
        .unwrap();
    remote.send(SessionEvent::Exited { code: 0 }).unwrap();

    let mut fed: Vec<String> = Vec::new();
    pump.pump(
        "local",
        &mut |b| fed.push(String::from_utf8_lossy(b).into_owned()),
        &mut |_, _| {},
    );
    assert_eq!(fed, ["x"]);
    assert!(
        pump.take_replay("remote").is_empty(),
        "控制事件不许进 replay"
    );

    let ctl = pump.take_control();
    assert_eq!(ctl.len(), 2);
    assert!(matches!(&ctl[0], ("local", SessionEvent::Opened { .. })));
    assert!(matches!(
        &ctl[1],
        ("remote", SessionEvent::Exited { code: 0 })
    ));
    assert!(pump.take_control().is_empty(), "取走即清");
}

/// 考题 5:replay 取走即清——补屏是一次性的,再取是空
#[test]
fn spec_pump_replay取走即清() {
    let mut pump = SessionPump::new();
    let remote = slot(&mut pump, "remote");
    remote.send(out("once")).unwrap();
    pump.pump("local", &mut |_| {}, &mut |_, _| {});
    assert_eq!(pump.take_replay("remote"), ["once"]);
    assert!(pump.take_replay("remote").is_empty());
}

/// 考题 6:replay 限量——爆量丢最旧,总量压在帽下(挂起期待机话痨
/// 不许把内存吃穿;现状 mpsc 无界积压本身就是暗雷,分家顺手排掉)
#[test]
fn spec_pump_replay限量丢最旧() {
    let mut pump = SessionPump::new();
    let remote = slot(&mut pump, "remote");
    let chunk = "x".repeat(64 * 1024); // 64KB 一块
    for _ in 0..8 {
        remote.send(out(&chunk)).unwrap(); // 共 512KB,帽 256KB
    }
    pump.pump("local", &mut |_| {}, &mut |_, _| {});
    let kept = pump.take_replay("remote");
    let total: usize = kept.iter().map(|s| s.len()).sum();
    assert!(total <= kfm_na::gate::REPLAY_CAP_BYTES, "缓存总量不许爆帽");
    assert_eq!(kept.len(), 4, "512KB 进 256KB 帽 = 只留最新 4 块");
}

/// 考题 7:同名 register = 断线重连换心脏——旧通道遗物一粒不收,
/// 该名 replay 一并清(旧 shell 遗物不喂新会话),新通道正常工作
#[test]
fn spec_pump_换心脏清遗物() {
    let mut pump = SessionPump::new();
    let old = slot(&mut pump, "remote");
    old.send(out("遗物")).unwrap();

    let (new_tx, new_rx) = channel();
    pump.register("remote", new_rx);
    new_tx.send(out("新会话")).unwrap();

    let mut fed: Vec<String> = Vec::new();
    pump.pump(
        "local",
        &mut |b| fed.push(String::from_utf8_lossy(b).into_owned()),
        &mut |_, _| {},
    );
    assert_eq!(pump.take_replay("remote"), ["新会话"], "旧通道遗物不许出现");

    // 旧发送端再发也没人收(通道已随旧 rx 一起 drop)
    assert!(
        old.send(out("ghost")).is_err() || {
            pump.pump("local", &mut |_| {}, &mut |_, _| {});
            pump.take_replay("remote").is_empty()
        }
    );
}

/// 考题 8:rec 见证回调全量收——活跃+待机都带名进 rec,路由分派
/// 本身不受影响(sink/replay 照旧)。飞行记录仪的接头钉死在这
#[test]
fn spec_pump_rec全量见证() {
    let mut pump = SessionPump::new();
    let local = slot(&mut pump, "local");
    let remote = slot(&mut pump, "remote");
    local.send(out("a")).unwrap();
    remote.send(out("b")).unwrap();

    let mut fed: Vec<String> = Vec::new();
    let mut rec: Vec<(String, String)> = Vec::new();
    pump.pump(
        "local",
        &mut |b| fed.push(String::from_utf8_lossy(b).into_owned()),
        &mut |n, b| rec.push((n.into(), String::from_utf8_lossy(b).into_owned())),
    );
    assert_eq!(fed, ["a"]);
    assert_eq!(
        rec,
        [
            ("local".to_string(), "a".to_string()),
            ("remote".to_string(), "b".to_string())
        ],
        "rec 必须全量带名见证,与路由无关"
    );
    assert_eq!(
        pump.take_replay("remote"),
        ["b"],
        "rec 分走的不许影响 replay"
    );
}
