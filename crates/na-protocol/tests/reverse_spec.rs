//! crates/na-protocol/tests/reverse_spec.rs — A 档考题：协议反向腿
//! （S→C 编码 encode_server / C→S 解码 decode_client，na-server 服务端侧）
//!
//! 协议真相源：kfmv4/src/server/ws-server.ts handleMessage + ws-channel.ts。
//! 正向腿（encode_client/decode_server）的考题在 tests/protocol_spec.rs（根仓），
//! 本考题只钉反向腿；两条腿同吃一個封套格式 {type, payload, timestamp}。
//!
//! 纪律（AGENTS.md 方法论 A 档）：先验证红（桩 unimplemented!），答案生成到绿，
//! 绿后做变异抽检。答案只允许碰 crates/na-protocol/src/lib.rs，本文件是考题。

use na_protocol::{ClientMsg, ServerMsg};

// ========== 编码（S→C）：类型名与 camelCase 字段钉死 ==========

/// 解析编码产物为 (type, payload)，公共检查：timestamp 是 >0 的 u64。
fn enc_parts(msg: &ServerMsg) -> (String, serde_json::Value) {
    let s = na_protocol::encode_server(msg);
    let v: serde_json::Value = serde_json::from_str(&s).expect("编码产物必须是合法 JSON");
    let ty = v["type"].as_str().expect("type 必须是字符串").to_string();
    let ts = v["timestamp"].as_u64().expect("timestamp 必须是 u64");
    assert!(ts > 0, "timestamp 必须 >0");
    (ty, v["payload"].clone())
}

#[test]
fn spec_enc_opened_with_tag() {
    let (ty, p) = enc_parts(&ServerMsg::Opened {
        session_id: "s1".into(),
        tag: Some("remote".into()),
    });
    assert_eq!(ty, "terminal-opened");
    assert_eq!(p["sessionId"], "s1");
    assert_eq!(p["tag"], "remote");
}

/// tag=None 必须整键消失（JSON.stringify({tag:undefined}) 语义；
/// decode_server 侧缺省/显式 null 都归 None，钉编码侧取「整键消失」）
#[test]
fn spec_enc_opened_tag_none_key_absent() {
    let (_, p) = enc_parts(&ServerMsg::Opened {
        session_id: "s1".into(),
        tag: None,
    });
    assert_eq!(p["sessionId"], "s1");
    assert!(
        p.get("tag").is_none(),
        "tag=None 时 payload 不许出现 tag 键"
    );
}

#[test]
fn spec_enc_output() {
    let (ty, p) = enc_parts(&ServerMsg::Output {
        session_id: "s1".into(),
        data: "hi\r\n".into(),
    });
    assert_eq!(ty, "terminal-output");
    assert_eq!(p["sessionId"], "s1");
    assert_eq!(p["data"], "hi\r\n");
}

#[test]
fn spec_enc_exit() {
    let (ty, p) = enc_parts(&ServerMsg::Exit {
        session_id: "s1".into(),
        code: 137,
    });
    assert_eq!(ty, "terminal-exit");
    assert_eq!(p["sessionId"], "s1");
    assert_eq!(p["code"], 137);
}

#[test]
fn spec_enc_error() {
    let (ty, p) = enc_parts(&ServerMsg::Error {
        message: "炸".into(),
    });
    assert_eq!(ty, "error");
    assert_eq!(p["message"], "炸");
}

/// Ping 的 payload 必须是 null（ws-server 30s 应用层心跳口径）
#[test]
fn spec_enc_ping_payload_null() {
    let (ty, p) = enc_parts(&ServerMsg::Ping);
    assert_eq!(ty, "ping");
    assert!(p.is_null(), "ping 的 payload 必须是 null");
}

/// Unknown 只允许出现在解码侧（前向兼容收纳），编码侧不许产 Unknown
/// ——编译期就没有这个构造路径（ServerMsg::Unknown 存在但 encode_server
/// 必须显式拒绝，防止服务端把吞不掉的消息又原样吐回去）
#[test]
fn spec_enc_unknown_rejected() {
    let r = std::panic::catch_unwind(|| {
        na_protocol::encode_server(&ServerMsg::Unknown {
            type_name: "tmux-result".into(),
        })
    });
    assert!(
        r.is_err(),
        "encode_server(Unknown) 必须 panic（编码侧不许产 Unknown）"
    );
}

// ========== 解码（C→S）：字段缺失即错，未知类型即错 ==========

fn dec(raw: &str) -> Result<ClientMsg, na_protocol::ProtocolError> {
    na_protocol::decode_client(raw)
}

/// 构造合法封套串
fn envelope(ty: &str, payload: serde_json::Value) -> String {
    serde_json::json!({"type": ty, "payload": payload, "timestamp": 1}).to_string()
}

#[test]
fn spec_dec_open_minimal() {
    let m = dec(&envelope("terminal-open", serde_json::json!({}))).expect("全缺省 open 合法");
    assert_eq!(
        m,
        ClientMsg::Open {
            cwd: None,
            command: None,
            tag: None,
        }
    );
}

#[test]
fn spec_dec_open_full() {
    let m = dec(&envelope(
        "terminal-open",
        serde_json::json!({"cwd": "/root", "command": "tmux attach", "tag": "remote"}),
    ))
    .expect("全字段 open 合法");
    assert_eq!(
        m,
        ClientMsg::Open {
            cwd: Some("/root".into()),
            command: Some("tmux attach".into()),
            tag: Some("remote".into()),
        }
    );
}

#[test]
fn spec_dec_input() {
    let m = dec(&envelope(
        "terminal-input",
        serde_json::json!({"sessionId": "s1", "input": "ls\n"}),
    ))
    .expect("input 合法");
    assert_eq!(
        m,
        ClientMsg::Input {
            session_id: "s1".into(),
            input: "ls\n".into(),
        }
    );
}

#[test]
fn spec_dec_input_missing_input_err() {
    let r = dec(&envelope(
        "terminal-input",
        serde_json::json!({"sessionId": "s1"}),
    ));
    assert!(r.is_err(), "input 缺 input 字段必须报错");
}

#[test]
fn spec_dec_resize() {
    let m = dec(&envelope(
        "terminal-resize",
        serde_json::json!({"sessionId": "s1", "cols": 120, "rows": 40}),
    ))
    .expect("resize 合法");
    assert_eq!(
        m,
        ClientMsg::Resize {
            session_id: "s1".into(),
            cols: 120,
            rows: 40,
        }
    );
}

#[test]
fn spec_dec_resize_missing_rows_err() {
    let r = dec(&envelope(
        "terminal-resize",
        serde_json::json!({"sessionId": "s1", "cols": 120}),
    ));
    assert!(r.is_err(), "resize 缺 rows 必须报错");
}

#[test]
fn spec_dec_close() {
    let m = dec(&envelope(
        "terminal-close",
        serde_json::json!({"sessionId": "s1"}),
    ))
    .expect("close 合法");
    assert_eq!(
        m,
        ClientMsg::Close {
            session_id: "s1".into(),
        }
    );
}

#[test]
fn spec_dec_close_missing_session_err() {
    let r = dec(&envelope("terminal-close", serde_json::json!({})));
    assert!(r.is_err(), "close 缺 sessionId 必须报错");
}

/// 服务端收消息没有前向兼容义务——未知类型就是协议错误
/// （与 decode_server 的 Unknown 收纳不对称，钉死这个不对称）
#[test]
fn spec_dec_unknown_type_err() {
    let r = dec(&envelope("tmux-cmd", serde_json::json!({})));
    assert!(r.is_err(), "decode_client 遇未知类型必须报错");
}

#[test]
fn spec_dec_invalid_json_err() {
    assert!(dec("not json").is_err());
}

#[test]
fn spec_dec_missing_type_err() {
    assert!(dec("{\"payload\":{}}").is_err());
}

// ========== 往返一致性：两条腿互判 ==========

/// 正向编码 → 反向解码必须回原值（na 发的，na-server 必须读得懂）
#[test]
fn spec_roundtrip_client() {
    let cases = [
        ClientMsg::Open {
            cwd: Some("/x".into()),
            command: None,
            tag: Some("t".into()),
        },
        ClientMsg::Open {
            cwd: None,
            command: None,
            tag: None,
        },
        ClientMsg::Input {
            session_id: "s".into(),
            input: "a".into(),
        },
        ClientMsg::Resize {
            session_id: "s".into(),
            cols: 1,
            rows: 2,
        },
        ClientMsg::Close {
            session_id: "s".into(),
        },
    ];
    for c in cases {
        let raw = na_protocol::encode_client(&c);
        assert_eq!(dec(&raw).expect("往返必须成功"), c, "往返失真: {raw}");
    }
}

/// 反向编码 → 正向解码必须回原值（na-server 发的，na 必须读得懂）
#[test]
fn spec_roundtrip_server() {
    let cases = [
        ServerMsg::Opened {
            session_id: "s".into(),
            tag: Some("t".into()),
        },
        ServerMsg::Opened {
            session_id: "s".into(),
            tag: None,
        },
        ServerMsg::Output {
            session_id: "s".into(),
            data: "d".into(),
        },
        ServerMsg::Exit {
            session_id: "s".into(),
            code: 0,
        },
        ServerMsg::Error {
            message: "m".into(),
        },
        ServerMsg::Ping,
    ];
    for c in cases {
        let raw = na_protocol::encode_server(&c);
        assert_eq!(
            na_protocol::decode_server(&raw).expect("往返必须成功"),
            c,
            "往返失真: {raw}"
        );
    }
}
