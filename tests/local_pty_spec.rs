//! tests/local_pty_spec.rs — 本地 PTY transport 契约考题(L1,A 档 host 实跑)
//!
//! 设计页:`/root/kfmv4/experiments/dsh-na/na/multi-end-layering.md` §3 四条:
//! ①echo 往返 ②resize 传播(TIOCSWINSZ) ③子进程退出事件 ④与 ws 工厂
//! 同 trait 可替换(基座双键并存注册)。
//!
//! host 判卷:shell = /bin/sh(local_pty::default_shell 的 host 分支)。

use std::time::Duration;

use kfm_na::base::Base;
use kfm_na::conn::{ConnConfig, TermCmd, TermFactory};
use kfm_na::local_pty::{LocalPtyFactory, local_pty_spawner};
use kfm_na::session::SessionEvent;

const TIMEOUT: Duration = Duration::from_secs(5);

/// 收事件直到 pred 命中;超时即红(把已见事件带进断言消息,红了好归因)
fn recv_until(
    rx: &std::sync::mpsc::Receiver<SessionEvent>,
    what: &str,
    mut pred: impl FnMut(&SessionEvent) -> bool,
) {
    let deadline = std::time::Instant::now() + TIMEOUT;
    let mut seen = Vec::new();
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ev) => {
                let hit = pred(&ev);
                seen.push(format!("{ev:?}"));
                if hit {
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("等不到 {what}(已见: {seen:?})");
}

/// 考题 1:echo 往返——spawn 即 Opened,Input 字节到 shell,回显+结果回 Output
#[test]
fn spec_l1_echo往返() {
    let factory = LocalPtyFactory::new(ConnConfig::default(), local_pty_spawner());
    let h = factory.spawn(&factory.default_config());
    recv_until(&h.events, "Opened", |ev| {
        matches!(ev, SessionEvent::Opened { .. })
    });
    h.outbound
        .send(TermCmd::Input("echo kfm-l1-hi\n".into()))
        .unwrap();
    recv_until(
        &h.events,
        "echo 结果",
        |ev| matches!(ev, SessionEvent::Output { data } if data.contains("kfm-l1-hi")),
    );
}

/// 考题 2:resize 传播——TIOCSWINSZ 必须落到 PTY(stty 读的是 slave 的 winsize)
#[test]
fn spec_l1_resize传播() {
    let factory = LocalPtyFactory::new(ConnConfig::default(), local_pty_spawner());
    let h = factory.spawn(&factory.default_config());
    recv_until(&h.events, "Opened", |ev| {
        matches!(ev, SessionEvent::Opened { .. })
    });
    h.outbound
        .send(TermCmd::Resize {
            cols: 132,
            rows: 43,
        })
        .unwrap();
    // stty 打印需换行触发;读多拍直到拿到 "43 132"
    h.outbound
        .send(TermCmd::Input("stty size\n".into()))
        .unwrap();
    recv_until(
        &h.events,
        "stty size = 43 132",
        |ev| matches!(ev, SessionEvent::Output { data } if data.contains("43 132")),
    );
}

/// 考题 3:子进程退出——shell exit → Exited 事件(收尸成功,码 0)
#[test]
fn spec_l1_子进程退出事件() {
    let factory = LocalPtyFactory::new(ConnConfig::default(), local_pty_spawner());
    let h = factory.spawn(&factory.default_config());
    recv_until(&h.events, "Opened", |ev| {
        matches!(ev, SessionEvent::Opened { .. })
    });
    h.outbound.send(TermCmd::Input("exit\n".into())).unwrap();
    recv_until(&h.events, "Exited", |ev| {
        matches!(ev, SessionEvent::Exited { code: 0 })
    });
}

/// 考题 4:插件注册——conn-provider-local 进基座,LocalPtyFactory 键可取;
/// 与 ws 插件双键并存(单一来源纪律:同键二次 provide 才报错)
#[test]
fn spec_l1_插件注册_双工厂并存() {
    let base = Base::new(vec![]);
    base.load(
        kfm_na::plugins::conn_provider_ws::ConnProviderWs::with_spawner(
            // 假 transport:注册行为判卷不真连(与 conn_provider_spec 同款)
            std::sync::Arc::new(|_| panic!("考题不许真 spawn ws")),
        ),
    )
    .expect("ws 插件装载失败");
    base.load(
        kfm_na::plugins::conn_provider_local::ConnProviderLocal::with_spawner(std::sync::Arc::new(
            |_| panic!("考题不许真 spawn local"),
        )),
    )
    .expect("本地插件装载失败");
    // 双键并存:两个工厂都能取回
    base.ctx().get::<dyn TermFactory>().expect("ws 工厂应可取");
    base.ctx().get::<LocalPtyFactory>().expect("本地工厂应可取");
}

/// 考题 5(L3 挂勾):bootstrap 装好后 shell 换 $PREFIX/bin/bash,
/// env 带 PATH/LD_LIBRARY_PATH/PREFIX;没装则回落系统 sh(行为不变)
#[test]
fn spec_l3_shell_plan_bash优先() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr");
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    std::fs::write(prefix.join("bin/bash"), b"fake").unwrap();
    let plan = kfm_na::local_pty::shell_plan(&prefix);
    assert_eq!(plan.shell, prefix.join("bin/bash").to_string_lossy());
    assert!(
        plan.env_extra
            .iter()
            .any(|e| e == &format!("PATH={}/bin:/system/bin:/system/xbin", prefix.display()))
    );
    assert!(
        plan.env_extra
            .iter()
            .any(|e| e == &format!("LD_LIBRARY_PATH={}/lib", prefix.display()))
    );
    assert!(
        plan.env_extra
            .iter()
            .any(|e| e == &format!("PREFIX={}", prefix.display()))
    );
}

#[test]
fn spec_l3_shell_plan_无bash回落系统sh() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("usr"); // 不存在
    let plan = kfm_na::local_pty::shell_plan(&prefix);
    assert_eq!(plan.shell, kfm_na::local_pty::default_shell());
    assert!(plan.env_extra.is_empty());
}
