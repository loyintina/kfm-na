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

/// 会话层后端（2026-09-20 na-server 立项，docs/active/na-server.md §五）：
/// Kfmv4 = 隧道指 kfmv4 8021（现状锚）；NaServer = 隧道指 na-server 9021，
/// 且由 na 主体拉起链负责它在服务器上的生死。
/// **默认 Kfmv4 = 行为零变化锚**——redroid 判绿后按用户拍板翻默认。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    #[default]
    Kfmv4,
    NaServer,
}

impl Backend {
    /// servers.json 的 "backend" 字段：只认 "na-server"，其余（含缺省/
    /// 未知值）一律 Kfmv4——未知值落回现状锚，不许静默进新世界
    pub fn parse(v: Option<&serde_json::Value>) -> Self {
        match v.and_then(|v| v.as_str()) {
            Some("na-server") => Backend::NaServer,
            _ => Backend::Kfmv4,
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
    /// 会话层后端（缺省 Kfmv4 现状锚）
    pub backend: Backend,
    /// QUIC 隧道腿（设计 docs/active/quic隧道.md；缺省关——§七问题 1
    /// 公网 UDP 口裁决前只代码就位）
    pub quic: QuicFields,
}

/// QUIC 腿服务器 UDP 口（单一源——na-server NA_QUIC_BIND 部署对齐它）。
/// 2026-09-23 用户拍板双口、避约定俗成段：
/// - 62633 = 正连数据路（本常量；数据面 QUIC 桥）
/// - 62694 = 反连推送路（M4 把 9022 QUIC 化时用，先立常量占位）
pub const QUIC_DEFAULT_PORT: u16 = 62633;
pub const QUIC_REVERSE_PORT: u16 = 62694;

/// QUIC 腿配置（servers.json "quic" 段）：enable 缺省 false（未裁决不开）、
/// port 缺省 62633、pin = 服务器证书 DER 的 SHA-256 hex（服务器证 pinning）、
/// psk = 预共享密钥 hex（客户端证 HMAC 挑战，设计 §四；两证齐全才准开腿）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuicFields {
    pub enable: bool,
    pub port: u16,
    pub pin: String,
    pub psk: String,
}

impl Default for QuicFields {
    fn default() -> Self {
        Self {
            enable: false,
            port: QUIC_DEFAULT_PORT,
            pin: String::new(),
            psk: String::new(),
        }
    }
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
        let quic_v = item.get("quic").cloned().unwrap_or(serde_json::Value::Null);
        let quic_d = QuicFields::default();
        let quic = QuicFields {
            enable: quic_v
                .get("enable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            port: quic_v
                .get("port")
                .and_then(|v| v.as_u64())
                .unwrap_or(quic_d.port as u64) as u16,
            pin: quic_v
                .get("pin")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            psk: quic_v
                .get("psk")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        };
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
            backend: Backend::parse(item.get("backend")),
            quic,
        });
    }
    Ok(out)
}

/// terminal.json 落盘序列化（设置页 v1 二版：默认服务器下拉换选写盘）。
/// 与 parse_terminal 同一份字段口径（defaultSession/switchHotkey{mod,key}；
/// 无修饰键 = 「-」宪法短线）
pub fn terminal_to_json(t: &TerminalConfig) -> String {
    let ds = match &t.default_session {
        DefaultSession::Local => "local".to_string(),
        DefaultSession::Server(id) => id.clone(),
    };
    let modifier = match t.switch_hotkey.modifier {
        Some(Mod::Ctrl) => "ctrl",
        Some(Mod::Alt) => "alt",
        Some(Mod::Shift) => "shift",
        None => "-",
    };
    let esc = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    format!(
        "{{\"defaultSession\":{},\"switchHotkey\":{{\"mod\":{},\"key\":{}}},\"pixelScroll\":{}}}",
        esc(&ds),
        esc(modifier),
        esc(&t.switch_hotkey.key),
        t.pixel_scroll
    )
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

impl DefaultSession {
    /// 默认会话 → 会话槽名（设置页换选立即联动的纯核，2026-09-21 用户
    /// 拍板：换选 = 活跃会话同步翻到对应槽。两会话拓扑下「翻到对应槽」
    /// = 不同名才 toggle——映射错 = 联动翻错边）
    pub fn session_name(&self) -> &'static str {
        match self {
            DefaultSession::Local => "local",
            DefaultSession::Server(_) => "remote",
        }
    }
}

/// terminal.json（全局项）：默认会话 + 全局切换键 + 像素级滚动开关
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalConfig {
    pub default_session: DefaultSession,
    pub switch_hotkey: Hotkey,
    /// 像素级滚动（2026-09-24 用户拍板「滚动的像素级」）：false = 旧行级
    /// 滚动保底（默认——防新机制意外卡死无退路，设置页终端设置可切回）
    pub pixel_scroll: bool,
}

impl Default for TerminalConfig {
    /// 现状行为锚（行为零变化承诺）：本地起步 + Ctrl-] + 行级滚动
    fn default() -> Self {
        TerminalConfig {
            default_session: DefaultSession::Local,
            switch_hotkey: Hotkey {
                modifier: Some(Mod::Ctrl),
                key: "]".into(),
            },
            pixel_scroll: false,
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
    let pixel_scroll = v
        .get("pixelScroll")
        .and_then(|b| b.as_bool())
        .unwrap_or(d.pixel_scroll);
    Ok(TerminalConfig {
        default_session,
        switch_hotkey,
        pixel_scroll,
    })
}
