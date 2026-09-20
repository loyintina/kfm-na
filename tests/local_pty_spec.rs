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

/// 收 local_exec 的一次性结果(超时不来即红——执行线程死了要显形)
fn recv_exec(
    rx: &std::sync::mpsc::Receiver<Result<String, String>>,
    what: &str,
) -> Result<String, String> {
    rx.recv_timeout(TIMEOUT)
        .unwrap_or_else(|e| panic!("等不到 exec 结果 {what}: {e}"))
}

/// 考题 6(两轴契约第 6 步):local_exec echo 往返——`sh -c` 跑命令,
/// 全部输出进 Ok(host 判卷:shell = /bin/sh)
#[test]
fn spec_l6_exec_echo往返() {
    let rx = kfm_na::local_pty::local_exec("echo kfm-exec-hi".into());
    let out = recv_exec(&rx, "echo").expect("echo 不该失败");
    assert!(out.contains("kfm-exec-hi"), "输出缺回显: {out:?}");
}

/// 考题 7:stderr 同 PTY 合并收回 + 非零退出仍 Ok(ws 路契约同形——
/// tmux list 在无服务端时 rc=1 带报错文本,解析层把噪声滤成空表,
/// 传输层不替它判成败)
#[test]
fn spec_l6_exec_非零退出仍ok并收stderr() {
    let rx = kfm_na::local_pty::local_exec("echo kfm-exec-err >&2; exit 3".into());
    let out = recv_exec(&rx, "stderr").expect("非零退出不该算传输失败");
    assert!(out.contains("kfm-exec-err"), "stderr 未收回: {out:?}");
}

/// 考题 8:超时兜底——子进程挂死必杀必报 Err(裸阻塞 read 会把超时
/// 咬死,本钉防读环退化);超时可注版 1s 判卷(10s 正身等不起)
#[test]
fn spec_l6_exec_超时兜底() {
    let rx = kfm_na::local_pty::local_exec_with("sleep 30".to_string(), 1);
    let err = recv_exec(&rx, "超时").expect_err("挂死命令不该出 Ok");
    assert!(err.contains("执行超时"), "报错缺超时词: {err:?}");
}

/// 考题 9(两轴第 6 步②):ConnConfig.command = 命令行语义——平台
/// shell `-c` 跑命令行,跑完自己退出(本地相 attach 重孵的工序根:
/// `tmux new-session -A -s '名'` 走这条路进 PTY)。与 ws 侧「服务端
/// sh -c」同一契约
#[test]
fn spec_l6_command命令行语义() {
    let factory = LocalPtyFactory::new(
        ConnConfig {
            url: String::new(),
            command: Some("echo kfm-cmdline-hi".into()),
        },
        local_pty_spawner(),
    );
    let h = factory.spawn(&factory.default_config());
    recv_until(
        &h.events,
        "命令行输出",
        |ev| matches!(ev, SessionEvent::Output { data } if data.contains("kfm-cmdline-hi")),
    );
    // sh -c 跑完即退——Exited 必须到(argv 漏 -c 会变交互 shell 挂死)
    recv_until(&h.events, "Exited", |ev| {
        matches!(ev, SessionEvent::Exited { .. })
    });
}
