//! tests/protocol_spec.rs — A 档考题：terminal-pty ws 协议编解码（2026-08-13 方法论首案）
//!
//! 协议真相源：kfmv4/src/server/ws-server.ts handleMessage + terminal-pty.ts。
//! 封套：{type, payload, timestamp}（kfmv4 ws-channel.ts WsMessage）。
//!
//! 纪律（AGENTS.md 方法论 A 档）：先验证红（桩 unimplemented!），答案生成到绿，
//! 绿后做变异抽检（故意改坏答案看考题抓不抓得住）。
//! 答案只允许碰 src/protocol.rs，本文件是考题，生成器不许改。

use kfm_na::protocol::{ClientMsg, ServerMsg};

// ========== 编码（C→S）：类型名与字段名钉死（服务端 camelCase 口径） ==========

/// 解析编码产物为 (type, payload, timestamp)，公共检查：timestamp 是 >0 的 u64。
fn encode_parts(msg: ClientMsg) -> (String, serde_json::Value, u64) {
    let s = kfm_na::protocol::encode_client(&msg);
    let v: serde_json::Value = serde_json::from_str(&s).expect("编码产物必须是合法 JSON");
    let ty = v["type"].as_str().expect("type 必须是字符串").to_string();
    let ts = v["timestamp"].as_u64().expect("timestamp 必须是 u64");
    assert!(ts > 0, "timestamp 必须 >0");
    (ty, v["payload"].clone(), ts)
}

#[test]
fn spec_encode_open_minimal() {
    let (ty, p, _) = encode_parts(ClientMsg::Open {
        cwd: None,
        command: None,
        tag: None,
    });
    assert_eq!(ty, "terminal-open");
    assert_eq!(
        p,
        serde_json::json!({}),
        "全 None 时 payload 必须是空对象，不得带 null 字段"
    );
}

#[test]
fn spec_encode_open_full() {
    let (ty, p, _) = encode_parts(ClientMsg::Open {
        cwd: Some("/root".into()),
        command: Some("tmux attach -t main".into()),
        tag: Some("c1".into()),
    });
    assert_eq!(ty, "terminal-open");
    assert_eq!(
        p,
        serde_json::json!({"cwd": "/root", "command": "tmux attach -t main", "tag": "c1"})
    );
}

#[test]
fn spec_encode_input_camel_case_and_escapes() {
    let (ty, p, _) = encode_parts(ClientMsg::Input {
        session_id: "s-1".into(),
        input: "\u{1b}[1;5D中文\n".into(),
    });
    assert_eq!(ty, "terminal-input");
    // 钉 camelCase：服务端读 p.sessionId（ws-server.ts:190），写成 session_id 就是哑弹
    assert_eq!(p["sessionId"], "s-1", "字段必须是 camelCase sessionId");
    assert!(p.get("session_id").is_none(), "不得出现 snake_case 字段");
    assert_eq!(
        p["input"], "\u{1b}[1;5D中文\n",
        "ANSI/中文/换行经 JSON 往返不得变"
    );
}

#[test]
fn spec_encode_resize_numeric() {
    let (ty, p, _) = encode_parts(ClientMsg::Resize {
        session_id: "s-1".into(),
        cols: 120,
        rows: 40,
    });
    assert_eq!(ty, "terminal-resize");
    assert_eq!(
        p,
        serde_json::json!({"sessionId": "s-1", "cols": 120, "rows": 40})
    );
    assert!(
        p["cols"].is_u64() && p["rows"].is_u64(),
        "cols/rows 必须是数字不是字符串"
    );
}

#[test]
fn spec_encode_close() {
    let (ty, p, _) = encode_parts(ClientMsg::Close {
        session_id: "s-1".into(),
    });
    assert_eq!(ty, "terminal-close");
    assert_eq!(p, serde_json::json!({"sessionId": "s-1"}));
}

// ========== 解码（S→C） ==========

#[test]
fn spec_decode_opened_with_and_without_tag() {
    let m = kfm_na::protocol::decode_server(
        r#"{"type":"terminal-opened","payload":{"sessionId":"s-1","tag":"c1"},"timestamp":1}"#,
    )
    .expect("合法 opened 必须解出");
    match m {
        ServerMsg::Opened { session_id, tag } => {
            assert_eq!(session_id, "s-1");
            assert_eq!(tag.as_deref(), Some("c1"));
        }
        other => panic!("期望 Opened，实得 {other:?}"),
    }
    let m2 = kfm_na::protocol::decode_server(
        r#"{"type":"terminal-opened","payload":{"sessionId":"s-2"},"timestamp":1}"#,
    )
    .expect("tag 缺省必须容忍（ws-server.ts:182 tag 可能为 undefined）");
    match m2 {
        ServerMsg::Opened { session_id, tag } => {
            assert_eq!(session_id, "s-2");
            assert_eq!(tag, None);
        }
        other => panic!("期望 Opened，实得 {other:?}"),
    }
}

#[test]
fn spec_decode_output_preserves_bytes() {
    let data = "\u{1b}[38;5;204m茉莉\u{1b}[0m 输出\r\n第二行";
    let wire = serde_json::json!({"type":"terminal-output","payload":{"sessionId":"s-1","data":data},"timestamp":1}).to_string();
    match kfm_na::protocol::decode_server(&wire).expect("合法 output 必须解出") {
        ServerMsg::Output {
            session_id,
            data: d,
        } => {
            assert_eq!(session_id, "s-1");
            assert_eq!(d, data, "终端字节流一个字符都不许变（含 ANSI/中文/\\r\\n）");
        }
        other => panic!("期望 Output，实得 {other:?}"),
    }
}

#[test]
fn spec_decode_exit_codes() {
    for code in [0, 1, 137] {
        let wire = serde_json::json!({"type":"terminal-exit","payload":{"sessionId":"s-1","code":code},"timestamp":1}).to_string();
        match kfm_na::protocol::decode_server(&wire).expect("合法 exit 必须解出") {
            ServerMsg::Exit {
                session_id,
                code: c,
            } => {
                assert_eq!(session_id, "s-1");
                assert_eq!(c, code);
            }
            other => panic!("期望 Exit，实得 {other:?}"),
        }
    }
}

#[test]
fn spec_decode_error_and_ping() {
    match kfm_na::protocol::decode_server(
        r#"{"type":"error","payload":{"message":"PTY spawn failed: x"},"timestamp":1}"#,
    )
    .expect("合法 error 必须解出")
    {
        ServerMsg::Error { message } => assert_eq!(message, "PTY spawn failed: x"),
        other => panic!("期望 Error，实得 {other:?}"),
    }
    // 服务端 30s 心跳会发应用层 ping（payload null，ws-server.ts:142）——必须认识它
    match kfm_na::protocol::decode_server(r#"{"type":"ping","payload":null,"timestamp":1}"#)
        .expect("ping 必须解出")
    {
        ServerMsg::Ping => {}
        other => panic!("期望 Ping，实得 {other:?}"),
    }
}

#[test]
fn spec_decode_unknown_type_forward_compat() {
    // ack/snapshot/tmux-* 等本客户端不关心的类型：归 Unknown，不许 Err 不许 panic
    match kfm_na::protocol::decode_server(
        r#"{"type":"ack","payload":{"received":"snapshot"},"timestamp":1}"#,
    )
    .expect("未知类型必须落 Unknown（前向兼容）")
    {
        ServerMsg::Unknown { type_name } => assert_eq!(type_name, "ack"),
        other => panic!("期望 Unknown，实得 {other:?}"),
    }
}

#[test]
fn spec_decode_rejects_malformed() {
    assert!(
        kfm_na::protocol::decode_server("not json").is_err(),
        "非 JSON 必须 Err 不得 panic"
    );
    assert!(
        kfm_na::protocol::decode_server("{}").is_err(),
        "缺 type 必须 Err"
    );
    assert!(
        kfm_na::protocol::decode_server(
            r#"{"type":"terminal-output","payload":{"data":"x"},"timestamp":1}"#
        )
        .is_err(),
        "output 缺 sessionId 必须 Err"
    );
    assert!(
        kfm_na::protocol::decode_server(
            r#"{"type":"terminal-output","payload":{"sessionId":"s"},"timestamp":1}"#
        )
        .is_err(),
        "output 缺 data 必须 Err"
    );
    assert!(
        kfm_na::protocol::decode_server(
            r#"{"type":"terminal-exit","payload":{"sessionId":"s","code":"0"},"timestamp":1}"#
        )
        .is_err(),
        "code 为字符串必须 Err（严格类型）"
    );
}

#[test]
fn spec_decode_tolerates_extra_fields() {
    // 服务端将来加字段不得击垮旧客户端（前向兼容第二腿）
    let m = kfm_na::protocol::decode_server(
        r#"{"type":"terminal-opened","payload":{"sessionId":"s-1","tag":null,"futureField":42},"timestamp":1,"extra":true}"#,
    )
    .expect("多余字段必须容忍");
    match m {
        ServerMsg::Opened { session_id, tag } => {
            assert_eq!(session_id, "s-1");
            assert_eq!(tag, None, "显式 null 的 tag 等同缺省");
        }
        other => panic!("期望 Opened，实得 {other:?}"),
    }
}
