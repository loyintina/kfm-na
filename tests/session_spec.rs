//! session_spec.rs — 终端会话状态机考题（A 档：约束 src/session.rs）
//!
//! 判卷维度：生命周期迁移、sessionId 归属过滤、出向消息门禁、终态静默。

use kfm_na::protocol::{ClientMsg, ServerMsg};
use kfm_na::session::{Session, SessionEvent, SessionState};

fn opened(id: &str) -> ServerMsg {
    ServerMsg::Opened {
        session_id: id.to_string(),
        tag: None,
    }
}

fn output(id: &str, data: &str) -> ServerMsg {
    ServerMsg::Output {
        session_id: id.to_string(),
        data: data.to_string(),
    }
}

#[test]
fn spec_初始为_opening() {
    let s = Session::new();
    assert_eq!(s.state(), &SessionState::Opening);
    assert_eq!(s.session_id(), None);
}

#[test]
fn spec_opened_绑定id_转live() {
    let mut s = Session::new();
    let ev = s.on_server(opened("pty-1"));
    assert_eq!(
        ev,
        Some(SessionEvent::Opened {
            session_id: "pty-1".into()
        })
    );
    assert_eq!(s.state(), &SessionState::Live);
    assert_eq!(s.session_id(), Some("pty-1"));
}

#[test]
fn spec_output_只认本会话id() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    // 别人的 output：忽略，状态不动
    assert_eq!(s.on_server(output("pty-2", "x")), None);
    assert_eq!(s.state(), &SessionState::Live);
    // 自己的 output：升事件
    assert_eq!(
        s.on_server(output("pty-1", "$ ")),
        Some(SessionEvent::Output { data: "$ ".into() })
    );
}

#[test]
fn spec_opened_前的_output_忽略() {
    let mut s = Session::new();
    assert_eq!(s.on_server(output("pty-1", "早产的帧")), None);
    assert_eq!(s.state(), &SessionState::Opening);
}

#[test]
fn spec_exit_记录退出码_终结() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    let ev = s.on_server(ServerMsg::Exit {
        session_id: "pty-1".into(),
        code: 0,
    });
    assert_eq!(ev, Some(SessionEvent::Exited { code: 0 }));
    assert_eq!(s.state(), &SessionState::Exited(0));
}

#[test]
fn spec_别人的_exit_不终结我() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    assert_eq!(
        s.on_server(ServerMsg::Exit {
            session_id: "pty-2".into(),
            code: 1
        }),
        None
    );
    assert_eq!(s.state(), &SessionState::Live);
}

#[test]
fn spec_error_转failed() {
    let mut s = Session::new();
    let ev = s.on_server(ServerMsg::Error {
        message: "PTY spawn failed".into(),
    });
    assert_eq!(
        ev,
        Some(SessionEvent::Failed {
            message: "PTY spawn failed".into()
        })
    );
    assert_eq!(s.state(), &SessionState::Failed("PTY spawn failed".into()));
}

#[test]
fn spec_终态后一律静默() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    s.on_server(ServerMsg::Exit {
        session_id: "pty-1".into(),
        code: 0,
    });
    // 迟到的 output / 第二个 exit / error 都不再产事件
    assert_eq!(s.on_server(output("pty-1", "迟到")), None);
    assert_eq!(
        s.on_server(ServerMsg::Exit {
            session_id: "pty-1".into(),
            code: 9
        }),
        None
    );
    assert_eq!(
        s.on_server(ServerMsg::Error {
            message: "补刀".into()
        }),
        None
    );
    assert_eq!(s.state(), &SessionState::Exited(0));
}

#[test]
fn spec_重复opened_容忍忽略() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    assert_eq!(s.on_server(opened("pty-1")), None);
    assert_eq!(s.session_id(), Some("pty-1")); // 首绑不覆盖
}

#[test]
fn spec_ping_unknown_不升事件() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    assert_eq!(s.on_server(ServerMsg::Ping), None);
    assert_eq!(
        s.on_server(ServerMsg::Unknown {
            type_name: "ack".into()
        }),
        None
    );
    assert_eq!(s.state(), &SessionState::Live);
}

#[test]
fn spec_出向消息_live前门禁() {
    let mut s = Session::new();
    // Opening：全部 None
    assert_eq!(s.input_msg("ls\n"), None);
    assert_eq!(s.resize_msg(80, 24), None);
    assert_eq!(s.close_msg(), None);
    s.on_server(opened("pty-1"));
    // Live：全部 Some 且带绑定 id
    assert_eq!(
        s.input_msg("ls\n"),
        Some(ClientMsg::Input {
            session_id: "pty-1".into(),
            input: "ls\n".into()
        })
    );
    assert_eq!(
        s.resize_msg(80, 24),
        Some(ClientMsg::Resize {
            session_id: "pty-1".into(),
            cols: 80,
            rows: 24
        })
    );
    assert_eq!(
        s.close_msg(),
        Some(ClientMsg::Close {
            session_id: "pty-1".into()
        })
    );
}

#[test]
fn spec_终态后出向也关门() {
    let mut s = Session::new();
    s.on_server(opened("pty-1"));
    s.on_server(ServerMsg::Exit {
        session_id: "pty-1".into(),
        code: 0,
    });
    assert_eq!(s.input_msg("x"), None);
    assert_eq!(s.resize_msg(1, 1), None);
    assert_eq!(s.close_msg(), None);
}

#[test]
fn spec_open_msg_带命令与全空() {
    let with_cmd = Session::open_msg(Some("tmux attach"));
    assert_eq!(
        with_cmd,
        ClientMsg::Open {
            cwd: None,
            command: Some("tmux attach".into()),
            tag: None,
        }
    );
    assert_eq!(
        Session::open_msg(None),
        ClientMsg::Open {
            cwd: None,
            command: None,
            tag: None,
        }
    );
}

// ---- 自动重孵时间闸（2026-09-11 redroid 瞬死案，BAR-080，A 档）----
// 约束 src/session.rs auto_respawn_due：云安卓 local 会话
// 「Opened 即 Failed」瞬死循环击穿旧「每剧集只自动重孵一次」语义
// （Opened 清牌 = 新剧集新第一次），死亡↔重孵每帧一轮实烧 2.5 核。
// 纯时间闸：首次立即放行，其后 ≥MIN_AUTO_RESPAWN_MS 才再放行。

use kfm_na::session::{MIN_AUTO_RESPAWN_MS, auto_respawn_due};

#[test]
fn spec_首次自动重孵立即放行() {
    assert!(auto_respawn_due(None, 0));
    assert!(auto_respawn_due(None, 999_999));
}

#[test]
fn spec_间隔内压住() {
    assert!(!auto_respawn_due(Some(1000), 1000));
    assert!(!auto_respawn_due(
        Some(1000),
        1000 + MIN_AUTO_RESPAWN_MS - 1
    ));
}

#[test]
fn spec_到点放行() {
    assert!(auto_respawn_due(Some(1000), 1000 + MIN_AUTO_RESPAWN_MS));
    assert!(auto_respawn_due(
        Some(1000),
        1000 + MIN_AUTO_RESPAWN_MS * 10
    ));
}

#[test]
fn spec_时钟回拨压住不炸() {
    // saturating_sub：回拨当 0 间隔 = 压住，不透支也不 panic
    assert!(!auto_respawn_due(Some(5000), 1000));
    assert!(!auto_respawn_due(Some(u64::MAX), 0));
}
