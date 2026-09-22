//! tests/session_router_spec.rs — 双会话输入路由考题(L1 裁决 4 附议,A 档)
//!
//! 判的坑:并存期输入发错会话(本地敲的命令进了远程,或反之)。
//! 假通道判卷:两个 mpsc 冒充两条会话,路由行为全在 host 可重现。

use std::sync::mpsc::channel;

use kfm_na::conn::TermCmd;
use kfm_na::session_router::SessionRouter;

fn fake_pair() -> (
    std::sync::mpsc::Sender<TermCmd>,
    std::sync::mpsc::Receiver<TermCmd>,
) {
    channel()
}

/// 考题 5a:默认路由——输入只进活跃会话,待机一粒不进
#[test]
fn spec_l1_路由_默认只进活跃() {
    let (tx_a, rx_a) = fake_pair();
    let (tx_b, rx_b) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    r.add_standby(tx_b, "remote").unwrap();
    r.send(TermCmd::Input("ls\n".into()));
    assert!(matches!(
        rx_a.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "ls\n"
    ));
    assert!(rx_b.try_recv().is_err(), "待机会话不该收到任何输入");
}

/// 考题 5b:切换后路由翻面——输入改进新活跃方;切回再翻回
#[test]
fn spec_l1_路由_切换翻面() {
    let (tx_a, rx_a) = fake_pair();
    let (tx_b, rx_b) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    r.add_standby(tx_b, "remote").unwrap();

    let (old, new) = r.switch().expect("有待机必须可切");
    assert_eq!((old, new), ("local", "remote"));
    r.send(TermCmd::Input("tmux\n".into()));
    assert!(matches!(
        rx_b.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "tmux\n"
    ));
    assert!(rx_a.try_recv().is_err(), "切换后旧活跃方不该再收输入");

    let (old2, new2) = r.switch().expect("再切回");
    assert_eq!((old2, new2), ("remote", "local"));
    r.send(TermCmd::Input("echo back\n".into()));
    assert!(matches!(
        rx_a.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "echo back\n"
    ));
}

/// 考题 5c:无待机切换 = 无操作——活跃方不动,输入不丢不错位
#[test]
fn spec_l1_路由_无待机切换无操作() {
    let (tx_a, rx_a) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    assert!(r.switch().is_none(), "无待机方时切换必须是无操作");
    r.send(TermCmd::Input("ls\n".into()));
    assert!(matches!(
        rx_a.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "ls\n"
    ));
    assert_eq!(r.active_name(), "local");
}

/// 考题 5d:待机槽只补一次——重复 add 拒绝(覆盖 = 会话通道丢失)
#[test]
fn spec_l1_路由_待机槽拒绝覆盖() {
    let (tx_a, _rx_a) = fake_pair();
    let (tx_b, _rx_b) = fake_pair();
    let (tx_c, _rx_c) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    r.add_standby(tx_b, "remote").unwrap();
    assert!(r.add_standby(tx_c, "remote2").is_err());
    assert!(r.has_standby());
}

/// 考题 5e:换心脏（断线重连）——replace_active 后输入进新通道,
/// 旧通道（僵尸会话）一粒不进;槽位名不动
#[test]
fn spec_l1_路由_活跃换心脏() {
    let (tx_a, rx_a) = fake_pair();
    let (tx_b, rx_b) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    r.replace_active(tx_b);
    r.send(TermCmd::Input("ls\n".into()));
    assert!(matches!(
        rx_b.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "ls\n"
    ));
    assert!(rx_a.try_recv().is_err(), "重连后旧通道不该再收输入");
    assert_eq!(r.active_name(), "local", "换心脏不许换槽位名");
}

/// 考题 5f:待机换心脏——replace_standby 后切换,输入进新通道
#[test]
fn spec_l1_路由_待机换心脏() {
    let (tx_a, rx_a) = fake_pair();
    let (tx_b, _rx_b) = fake_pair();
    let (tx_c, rx_c) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    r.add_standby(tx_b, "remote").unwrap();
    r.replace_standby(tx_c).unwrap();
    let (old, new) = r.switch().expect("有待机必须可切");
    assert_eq!((old, new), ("local", "remote"));
    r.send(TermCmd::Input("tmux attach\n".into()));
    assert!(matches!(
        rx_c.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "tmux attach\n"
    ));
    assert!(rx_a.try_recv().is_err(), "切换后旧活跃方不该再收输入");
}

/// 考题 5g:无待机槽 replace_standby = Err(装配错误不许静默)
#[test]
fn spec_l1_路由_无待机换心脏拒绝() {
    let (tx_a, _rx_a) = fake_pair();
    let (tx_b, _rx_b) = fake_pair();
    let mut r = SessionRouter::new(tx_a, "local");
    assert!(r.replace_standby(tx_b).is_err(), "无待机槽换心脏必须报错");
}

/// 考题 5h:send_checked 回执——活通道 true 且送达;对端(接收方已丢)
/// 死了报 false(僵尸通道不许静默吞,闸门判案靠它)
#[test]
fn spec_l1_路由_send_checked回执() {
    let (tx_a, rx_a) = fake_pair();
    let r = SessionRouter::new(tx_a, "local");
    assert!(r.send_checked(TermCmd::Input("ls\n".into())));
    assert!(matches!(
        rx_a.recv_timeout(std::time::Duration::from_millis(200)),
        Ok(TermCmd::Input(s)) if s == "ls\n"
    ));
    drop(rx_a);
    assert!(
        !r.send_checked(TermCmd::Input("x".into())),
        "对端死了必须报 false"
    );
}

/// 考题 BAR-124:抖动尺寸 = 行数减一且保底 1 行(0 行 pty 畸形不许造;
/// 列数纹丝不动——只抖行,列抖了 tmux 横排也重画,白送一次洪峰)
#[test]
fn spec_bar124_抖动尺寸_减一行保底() {
    use kfm_na::session_router::jog_resize;
    assert_eq!(jog_resize(80, 24), (80, 23));
    assert_eq!(jog_resize(80, 2), (80, 1));
    assert_eq!(jog_resize(80, 1), (80, 1), "1 行不许抖成 0 行");
    assert_eq!(jog_resize(80, 0), (80, 1), "0 行输入也要兜回 1 行");
}

#[test]
fn spec_bar132_远程会话重孵须隧道可用() {
    // 2026-09-22 用户「正常使用时突然掉线，然后反复跳重连」定案：
    // 隧道断着的 30 秒里会话重孵空转六次（每条都 `Connection refused`），
    // 那就是用户看到的「反复跳」。闸 = 远程会话必须隧道可用；本地会话
    // （本机 PTY）不吃这条；时间闸语义不变
    use kfm_na::session::auto_respawn_allowed;
    assert!(
        auto_respawn_allowed(true, true, true),
        "隧道可用 + 够钟 = 放行"
    );
    assert!(
        !auto_respawn_allowed(false, true, true),
        "远程会话 + 隧道不可用 = 不许重孵（空转即「反复跳」）"
    );
    assert!(
        auto_respawn_allowed(false, false, true),
        "本地会话与隧道无关，照放行"
    );
    assert!(
        !auto_respawn_allowed(true, true, false),
        "时间闸仍在（放行 ≠ 无视节流）"
    );
    assert!(!auto_respawn_allowed(true, false, false));
}
