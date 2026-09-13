//! settings.rs — servers.json / terminal.json 解析核（设置页 v1 数据模型）。
//!
//! 契约真相源：docs/active/设置页.md §2.5 数据模型 + §2.4 切换行为决策。
//! providers.json 同规：JSON 宽容缺省、纯逻辑零 IO（文件读取在调用方）。
//! hotkey→字节映射**不另造轮子**——直接调 keymap::map_text，与快捷键行
//! 实际产出的字节同源（宪法 §六 禁手抄：两处映射各写一份必漂移）。

use crate::keymap;

/// 修饰键（宪法 §六 组合键录入控件：无修饰键 = 短线「—」，JSON 写 "-"）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mod {
    Ctrl,
    Alt,
    Shift,
}

/// 组合键：修饰键可选（None = 「—」）+ 键名（单字符）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub modifier: Option<Mod>,
    pub key: String,
}

impl Hotkey {
    /// 组合键 → 终端字节（与 keymap::map_text 同一把尺：
    /// Ctrl+]=\x1d、Ctrl+C=\x03、Alt+X=ESC x、无修饰=原样）
    pub fn bytes(&self) -> Vec<u8> {
        let (ctrl, alt, shift) = match self.modifier {
            Some(Mod::Ctrl) => (true, false, false),
            Some(Mod::Alt) => (false, true, false),
            Some(Mod::Shift) => (false, false, true),
            None => (false, false, false),
        };
        keymap::map_text(ctrl, alt, shift, &self.key).into_bytes()
    }

    /// 展示串（会话切换横幅等）：Ctrl+] / Alt+x / Shift+a / 裸键名
    pub fn display(&self) -> String {
        let m = match self.modifier {
            Some(Mod::Ctrl) => "Ctrl+",
            Some(Mod::Alt) => "Alt+",
            Some(Mod::Shift) => "Shift+",
            None => "",
        };
        format!("{m}{}", self.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SshFields {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: String,
    /// 空 = 密钥登录（设置页 §2.2）
    pub password: String,
}

/// 通联端口（工程参数，JSON 可设 UI 不摆；每设备 10 口段，设置页 §2.5）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelPorts {
    pub local_port: u16,
    pub remote_port: u16,
}

impl Default for TunnelPorts {
    fn default() -> Self {
        TunnelPorts {
            local_port: 9021,
            remote_port: 9022,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub ssh: SshFields,
    pub tunnel: TunnelPorts,
    pub ws_url: String,
    pub command: Option<String>,
    /// 每服务器直达切换键（§2.4：不入轮换环，各自绑定）；未绑 = None
    pub hotkey: Option<Hotkey>,
}

impl ServerEntry {
    /// 按 id 或 name 匹配（providers.find 同款：双字段都试，无静默回退）
    pub fn find<'a>(servers: &'a [ServerEntry], key: &str) -> Option<&'a ServerEntry> {
        servers.iter().find(|s| s.id == key || s.name == key)
    }
}

/// 解析 servers.json（顶层数组，条目字段宽容缺省）
pub fn parse_servers(json: &str) -> Result<Vec<ServerEntry>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("servers.json 不是合法 JSON: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "servers.json 顶层必须是数组".to_string())?;
    let mut out = Vec::new();
    for item in arr {
        let s = |k: &str| {
            item.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let ssh_v = item.get("ssh").cloned().unwrap_or(serde_json::Value::Null);
        let fs = |k: &str| {
            ssh_v
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let port = ssh_v.get("port").and_then(|v| v.as_u64()).unwrap_or(22) as u16;
        let tun_v = item
            .get("tunnel")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let tun_d = TunnelPorts::default();
        let tunnel = TunnelPorts {
            local_port: tun_v
                .get("localPort")
                .and_then(|v| v.as_u64())
                .unwrap_or(tun_d.local_port as u64) as u16,
            remote_port: tun_v
                .get("remotePort")
                .and_then(|v| v.as_u64())
                .unwrap_or(tun_d.remote_port as u64) as u16,
        };
        let command = item
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        out.push(ServerEntry {
            id: s("id"),
            name: s("name"),
            ssh: SshFields {
                host: fs("host"),
                port,
                user: fs("user"),
                key_path: fs("keyPath"),
                password: fs("password"),
            },
            tunnel,
            ws_url: s("wsUrl"),
            command,
            hotkey: item.get("hotkey").and_then(parse_hotkey_value),
        });
    }
    Ok(out)
}

fn parse_hotkey_value(v: &serde_json::Value) -> Option<Hotkey> {
    let key = v.get("key").and_then(|k| k.as_str())?.to_string();
    let modifier = match v.get("mod").and_then(|m| m.as_str()) {
        Some("ctrl") => Some(Mod::Ctrl),
        Some("alt") => Some(Mod::Alt),
        Some("shift") => Some(Mod::Shift),
        // "-"（宪法短线）与缺省/未知值 = 无修饰键
        _ => None,
    };
    Some(Hotkey { modifier, key })
}

/// 默认会话：本地 / 某台服务器（值为服务器 id 或 name）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultSession {
    Local,
    Server(String),
}

/// terminal.json（全局项）：默认会话 + 全局切换键
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalConfig {
    pub default_session: DefaultSession,
    pub switch_hotkey: Hotkey,
}

impl Default for TerminalConfig {
    /// 现状行为锚（行为零变化承诺）：本地起步 + Ctrl-]
    fn default() -> Self {
        TerminalConfig {
            default_session: DefaultSession::Local,
            switch_hotkey: Hotkey {
                modifier: Some(Mod::Ctrl),
                key: "]".into(),
            },
        }
    }
}

/// 解析 terminal.json（顶层对象，字段宽容缺省 → 现状锚）
pub fn parse_terminal(json: &str) -> Result<TerminalConfig, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("terminal.json 不是合法 JSON: {e}"))?;
    if !v.is_object() {
        return Err("terminal.json 顶层必须是对象".to_string());
    }
    let d = TerminalConfig::default();
    let default_session = match v.get("defaultSession").and_then(|s| s.as_str()) {
        None | Some("") | Some("local") => DefaultSession::Local,
        Some(id) => DefaultSession::Server(id.to_string()),
    };
    let switch_hotkey = v
        .get("switchHotkey")
        .and_then(parse_hotkey_value)
        .unwrap_or(d.switch_hotkey);
    Ok(TerminalConfig {
        default_session,
        switch_hotkey,
    })
}
