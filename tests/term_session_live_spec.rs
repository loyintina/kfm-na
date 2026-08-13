//! term_session_live_spec.rs — 常驻会话驱动 B 档钉（live）：真连本机 kfmv4 8021
//!
//! 默认 #[ignore]——依赖运行中的 kfmv4 服务器。
//! 手动跑：cargo test --test term_session_live_spec -- --ignored
//!
//! 判卷：spawn_terminal_session 双向泵全链——
//! Opened 前发的 input 被缓存、绑定后补发（shell 回显印记）、
//! resize 不炸、exit 指令终结会话且退出码正确、事件顺序合法。

use std::sync::mpsc;
use std::time::{Duration, Instant};

use kfm_na::conn::{TermCmd, spawn_terminal_session};
use kfm_na::session::SessionEvent;

#[test]
#[ignore]
fn live_常驻会话_缓存补发与退出() {
    let (ev_tx, ev_rx) = mpsc::channel::<SessionEvent>();
    let outbound = spawn_terminal_session("ws://127.0.0.1:8021/ws", None, move |ev| {
        ev_tx.send(ev).unwrap();
    });

    // Opened 之前就发 input——必须被缓存，绑定后补发（本钉的核心判卷点）
    outbound
        .send(TermCmd::Input("echo KFM-NA-SESSION-OK\n".into()))
        .unwrap();
    outbound
        .send(TermCmd::Resize { cols: 90, rows: 30 })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut opened = false;
    let mut outputs = String::new();
    let mut exit_code = None;
    while Instant::now() < deadline && exit_code.is_none() {
        let Ok(ev) = ev_rx.recv_timeout(Duration::from_secs(2)) else {
            continue;
        };
        match ev {
            SessionEvent::Opened { session_id } => {
                assert!(!session_id.is_empty());
                opened = true;
                // opened 后再发 resize（Live 态直通路径）
                outbound
                    .send(TermCmd::Resize { cols: 80, rows: 24 })
                    .unwrap();
            }
            SessionEvent::Output { data } => {
                outputs.push_str(&data);
                if opened && outputs.contains("KFM-NA-SESSION-OK") && exit_code.is_none() {
                    // 印记回了——缓存补发链走通；收尾
                    outbound.send(TermCmd::Input("exit 7\n".into())).unwrap();
                }
            }
            SessionEvent::Exited { code } => exit_code = Some(code),
            SessionEvent::Failed { message } => panic!("会话失败: {message}"),
        }
    }
    assert!(opened, "必须收到 Opened");
    assert!(
        outputs.contains("KFM-NA-SESSION-OK"),
        "Opened 前缓存的 input 必须补发并回显，实际输出: {outputs:.200}"
    );
    assert_eq!(exit_code, Some(7), "exit 7 的退出码必须原样回来");
}
