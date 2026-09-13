//! tests/settings_spec.rs — A 档考题：servers.json/terminal.json 解析核（src/settings.rs）
//!
//! 契约真相源：docs/active/设置页.md §2.5 数据模型 + §2.4 切换行为决策。
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::settings::{
    DefaultSession, Hotkey, Mod, ServerEntry, TerminalConfig, parse_servers, parse_terminal,
};

const SERVERS_SAMPLE: &str = r#"[
  {
    "id": "main",
    "name": "阿里云主服",
    "ssh": {
      "host": "8.145.46.182",
      "port": 22,
      "user": "root",
      "keyPath": "/data/data/dev.kfm.na/files/usr/etc/ssh/id_ed25519",
      "password": ""
    },
    "tunnel": { "localPort": 9021, "remotePort": 9022 },
    "wsUrl": "ws://127.0.0.1:9021/ws",
    "command": null,
    "hotkey": { "mod": "ctrl", "key": "]" }
  },
  {
    "id": "dev",
    "name": "开发机",
    "ssh": { "host": "192.168.1.10", "user": "loyin", "password": "s3cret" },
    "wsUrl": "ws://127.0.0.1:9031/ws"
  }
]"#;

// ---- servers.json ----

#[test]
fn servers_parse_full_fields() {
    let v = parse_servers(SERVERS_SAMPLE).unwrap();
    assert_eq!(v.len(), 2);
    let m = &v[0];
    assert_eq!(m.id, "main");
    assert_eq!(m.name, "阿里云主服");
    assert_eq!(m.ssh.host, "8.145.46.182");
    assert_eq!(m.ssh.port, 22);
    assert_eq!(m.ssh.user, "root");
    assert_eq!(
        m.ssh.key_path,
        "/data/data/dev.kfm.na/files/usr/etc/ssh/id_ed25519"
    );
    assert_eq!(m.ssh.password, "");
    assert_eq!(m.tunnel.local_port, 9021);
    assert_eq!(m.tunnel.remote_port, 9022);
    assert_eq!(m.ws_url, "ws://127.0.0.1:9021/ws");
    assert_eq!(m.command, None);
    let hk = m.hotkey.as_ref().unwrap();
    assert_eq!(hk.modifier, Some(Mod::Ctrl));
    assert_eq!(hk.key, "]");
}

#[test]
fn servers_lenient_defaults() {
    // 宽容缺省（providers.json 同规）：port 默认 22，tunnel 默认 9021/9022，
    // 缺 ssh/tunnel/hotkey/command 整块不炸
    let v = parse_servers(SERVERS_SAMPLE).unwrap();
    let d = &v[1];
    assert_eq!(d.ssh.port, 22);
    assert_eq!(d.ssh.key_path, "");
    assert_eq!(d.ssh.password, "s3cret");
    assert_eq!(d.tunnel.local_port, 9021);
    assert_eq!(d.tunnel.remote_port, 9022);
    assert!(d.hotkey.is_none());
    assert_eq!(d.command, None);
}

#[test]
fn servers_reject_bad_shape() {
    assert!(parse_servers("不是 json").is_err());
    assert!(parse_servers(r#"{"id":"x"}"#).is_err(), "顶层必须数组");
    assert!(parse_servers(r#"[]"#).unwrap().is_empty());
}

#[test]
fn servers_find_by_id_or_name() {
    let v = parse_servers(SERVERS_SAMPLE).unwrap();
    assert_eq!(
        ServerEntry::find(&v, "main").unwrap().ssh.host,
        "8.145.46.182"
    );
    assert_eq!(ServerEntry::find(&v, "开发机").unwrap().id, "dev");
    assert!(ServerEntry::find(&v, "不存在").is_none());
}

// ---- hotkey → 字节（与 keymap::map_text 同源语义）----

#[test]
fn hotkey_bytes_ctrl_bracket_is_0x1d() {
    // 现状锚：Ctrl-] 落成 \x1d（telnet 转义符惯例，android_app 拦截同款）
    let hk = Hotkey {
        modifier: Some(Mod::Ctrl),
        key: "]".into(),
    };
    assert_eq!(hk.bytes(), "\u{1d}".as_bytes());
}

#[test]
fn hotkey_bytes_ctrl_letter_lowercase_same_as_upper() {
    // c & 0x1f 大小写同值（keymap 条款：Ctrl+C=\x03 的命根）
    let lower = Hotkey {
        modifier: Some(Mod::Ctrl),
        key: "c".into(),
    };
    let upper = Hotkey {
        modifier: Some(Mod::Ctrl),
        key: "C".into(),
    };
    assert_eq!(lower.bytes(), b"\x03");
    assert_eq!(lower.bytes(), upper.bytes());
}

#[test]
fn hotkey_bytes_alt_is_esc_prefix() {
    let hk = Hotkey {
        modifier: Some(Mod::Alt),
        key: "x".into(),
    };
    assert_eq!(hk.bytes(), b"\x1bx");
}

#[test]
fn hotkey_bytes_no_modifier_plain() {
    // 「—」（无修饰键）：键名原样——壳层对全局切换键应拒用（会劫打字），
    // 解析核不裁判用途，只忠实映射
    let hk = Hotkey {
        modifier: None,
        key: "q".into(),
    };
    assert_eq!(hk.bytes(), b"q");
}

#[test]
fn hotkey_parse_dash_means_none() {
    // 宪法 §六：无控制键显示短线「—」，JSON 里写 "-"
    let t: TerminalConfig =
        parse_terminal(r#"{ "switchHotkey": { "mod": "-", "key": "q" } }"#).unwrap();
    assert_eq!(t.switch_hotkey.modifier, None);
    assert_eq!(t.switch_hotkey.key, "q");
}

// ---- terminal.json ----

#[test]
fn terminal_default_is_status_quo() {
    // 缺文件/缺字段 = 现状行为锚：本地起步 + Ctrl-]（行为零变化承诺）
    let d = TerminalConfig::default();
    assert_eq!(d.default_session, DefaultSession::Local);
    assert_eq!(d.switch_hotkey.bytes(), "\u{1d}".as_bytes());
    let parsed = parse_terminal(r#"{}"#).unwrap();
    assert_eq!(parsed, TerminalConfig::default());
}

#[test]
fn terminal_parse_full() {
    let t = parse_terminal(
        r#"{ "defaultSession": "main", "switchHotkey": { "mod": "ctrl", "key": "o" } }"#,
    )
    .unwrap();
    assert_eq!(t.default_session, DefaultSession::Server("main".into()));
    assert_eq!(t.switch_hotkey.bytes(), b"\x0f"); // Ctrl+O
}

#[test]
fn terminal_default_session_local_literal() {
    let t = parse_terminal(r#"{ "defaultSession": "local" }"#).unwrap();
    assert_eq!(t.default_session, DefaultSession::Local);
}

#[test]
fn terminal_reject_bad_shape() {
    assert!(parse_terminal("不是 json").is_err());
    assert!(parse_terminal(r#"[1,2]"#).is_err(), "顶层必须对象");
}
