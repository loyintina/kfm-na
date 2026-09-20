//! svc_health.rs — na-server health 面轮询器（服务卡唯一数据源，
//! docs/active/na-server.md §三/§四）。
//!
//! 分层：上半 A 档纯逻辑（health JSON 解析/uptime·空闲格式化/会话行
//! 文案，tests/svc_card_spec.rs 钉死）；下半 B 档轮询胶水（TcpStream +
//! http1 手写客户端打隧道本地口——复用 direct-api-brain 的传输层，
//! 零新依赖）。
//!
//! 节拍纪律（设计定稿）：解析页可见 2s 一拍，不可见不轮——壳每帧喂
//! set_visible（与 parser_docked 同源）；后端非 na-server 不轮
//! （卡显示「kfmv4 托管」态）。快照全局注册（tunnel/nasup 同款），
//! 涂装直读免穿 App plumbing；epoch 变 → DIRTY 置位 → 壳脏帧重烘。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::settings::Backend;

// ---- A 档：数据形状与纯函数（考题先行钉死）----

/// health 面一条会话记录（字段与 na-server httpd 同一份契约）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthSession {
    pub id: String,
    pub cmd: String,
    pub cols: u32,
    pub rows: u32,
    pub alive: bool,
    pub idle_s: u64,
}

/// health 面解析结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthInfo {
    pub uptime_s: u64,
    pub sessions: Vec<HealthSession>,
}

/// health JSON → 数据形状（A 档纯函数）。宽容缺省：缺字段按零值，
/// 整体不是合法 JSON / 缺 uptime_s 才报错——契约主键缺失 = 对面不是
/// na-server（kfmv4 端口撞车等），必须显形不许静默当零会话
pub fn parse_health(json: &str) -> Result<HealthInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("health 不是合法 JSON: {e}"))?;
    let uptime_s = v
        .get("uptime_s")
        .and_then(|u| u.as_u64())
        .ok_or_else(|| "health 缺 uptime_s".to_string())?;
    let sessions = v
        .get("sessions")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|s| HealthSession {
            id: s
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string(),
            cmd: s
                .get("cmd")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            cols: s.get("cols").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            rows: s.get("rows").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            alive: s.get("alive").and_then(|x| x.as_bool()).unwrap_or(false),
            idle_s: s.get("idle_s").and_then(|x| x.as_u64()).unwrap_or(0),
        })
        .collect();
    Ok(HealthInfo { uptime_s, sessions })
}

/// 时长紧凑格式（A 档）：<60s = "Ns"，<1h = "Nm"，<1d = "Nh"，否则 "Nd"
pub fn fmt_duration(s: u64) -> String {
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86400)
    }
}

/// 会话行文案（A 档）：`s6 · 80×24 · 闲 3m`；死会话标死（alive=false
/// 的会话留在列表里是 health 面的信息，不许悄悄当活的画）
pub fn session_line(s: &HealthSession) -> String {
    let dead = if s.alive { "" } else { "（死）" };
    format!(
        "{} · {}×{} · 闲 {}{}",
        s.id,
        s.cols,
        s.rows,
        fmt_duration(s.idle_s),
        dead
    )
}

// ---- 数据面（UI 只读这道门）----

/// 轮询相位
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// 后端非 na-server（含未配置）——卡显示「kfmv4 托管」态
    Kfmv4,
    /// na-server 后端但还没轮到第一拍（页未开过/刚切后端）
    Pending,
    /// 轮询在途（首拍）
    Loading,
    /// 有健康数据
    Ready,
    /// 轮询失败（保留上一份数据——闪断不许清空卡面）
    Error(String),
}

/// 对外快照（服务卡数据源）
#[derive(Debug, Clone)]
pub struct HealthSnap {
    pub phase: Phase,
    pub info: Option<HealthInfo>,
    pub epoch: u64,
}

struct Inner {
    cfg_backend: Backend,
    cfg_port: u16,
    visible: bool,
    snap: HealthSnap,
}

static INNER: OnceLock<Arc<Mutex<Inner>>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);

fn inner() -> &'static Arc<Mutex<Inner>> {
    INNER.get_or_init(|| {
        Arc::new(Mutex::new(Inner {
            cfg_backend: Backend::Kfmv4,
            cfg_port: 9021,
            visible: false,
            snap: HealthSnap {
                phase: Phase::Kfmv4,
                info: None,
                epoch: 0,
            },
        }))
    })
}

fn bump(g: &mut Inner, phase: Phase, info: Option<HealthInfo>) {
    if g.snap.phase != phase || g.snap.info != info {
        g.snap.phase = phase;
        g.snap.info = info;
        g.snap.epoch += 1;
        DIRTY.store(true, Ordering::Relaxed);
    }
}

/// 配置（壳设置加载/重载时喂）：后端 + 隧道本地口。幂等——重复喂同值
/// 零动作；后端翻相时清数据（kfmv4 时代的会话行不许带进 na-server 相）
pub fn configure(backend: Backend, local_port: u16) {
    {
        let mut g = inner().lock().unwrap();
        if g.cfg_backend == backend && g.cfg_port == local_port {
            return;
        }
        g.cfg_backend = backend;
        g.cfg_port = local_port;
        let phase = if backend == Backend::NaServer {
            Phase::Pending
        } else {
            Phase::Kfmv4
        };
        bump(&mut g, phase, None);
    }
    ensure_poller();
}

/// 解析页可见性（壳每帧喂，与 parser_docked 同源）
pub fn set_visible(v: bool) {
    let mut g = inner().lock().unwrap();
    if g.visible != v {
        g.visible = v;
        // 页一开若还在 Pending（从未轮过）→ Loading（卡面给个在途相）
        if v && g.cfg_backend == Backend::NaServer && g.snap.phase == Phase::Pending {
            bump(&mut g, Phase::Loading, None);
        }
    }
}

/// 读当前快照（涂装每烘焙拍一张；锁短）
pub fn snap() -> HealthSnap {
    inner().lock().unwrap().snap.clone()
}

/// 壳脏帧消耗口：有变化取走 true（每帧一查，零成本）
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::Relaxed)
}

// ---- B 档：轮询胶水（判卷 = redroid 实录：卡面 vs curl health 对表）----

/// 页可见时的轮询节拍（A 档常量，设计定稿 2s）
pub const POLL_SECS: u64 = 2;
/// 单次 GET 总超时（连/读同档）
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);
/// body 上限（health 面也就几 KB，防爆内存）
const BODY_CAP: usize = 64 * 1024;

fn http_get_health(port: u16) -> Result<HealthInfo, String> {
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let st =
        TcpStream::connect_timeout(&addr, HTTP_TIMEOUT).map_err(|e| format!("连接失败: {e}"))?;
    st.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    st.set_write_timeout(Some(HTTP_TIMEOUT)).ok();
    let req = crate::http1::serialize_request(&crate::http1::Request {
        method: "GET".into(),
        path: "/api/na/health".into(),
        headers: vec![("Host".into(), "127.0.0.1".into())],
        body: Vec::new(),
    });
    let mut st = st;
    st.write_all(&req).map_err(|e| format!("写请求失败: {e}"))?;
    let mut io = crate::http1::BufIo::new(st);
    let head = io.read_head().map_err(|e| format!("读响应头失败: {e}"))?;
    if head.status != 200 {
        return Err(format!("HTTP {}", head.status));
    }
    let mut rd = io.body_reader(head.body_kind());
    let mut body = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match rd.read_body(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                body.extend_from_slice(&buf[..n]);
                if body.len() > BODY_CAP {
                    return Err("body 超限".into());
                }
            }
            Err(e) => return Err(format!("读 body 失败: {e}")),
        }
    }
    parse_health(&String::from_utf8_lossy(&body))
}

fn ensure_poller() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        std::thread::spawn(|| {
            let mut last_poll: Option<Instant> = None;
            let mut was_visible = false;
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let (backend, port, visible) = {
                    let g = inner().lock().unwrap();
                    (g.cfg_backend, g.cfg_port, g.visible)
                };
                if backend != Backend::NaServer {
                    last_poll = None;
                    was_visible = false;
                    continue;
                }
                if !visible {
                    was_visible = false;
                    continue;
                }
                // 可见上升沿立即一拍；之后 2s 一拍
                let due = !was_visible
                    || last_poll.is_none_or(|t| t.elapsed() >= Duration::from_secs(POLL_SECS));
                was_visible = true;
                if !due {
                    continue;
                }
                last_poll = Some(Instant::now());
                match http_get_health(port) {
                    Ok(info) => {
                        let mut g = inner().lock().unwrap();
                        bump(&mut g, Phase::Ready, Some(info));
                    }
                    Err(e) => {
                        crate::report::report("svchealth", &format!("health 轮询失败: {e}"));
                        let mut g = inner().lock().unwrap();
                        let keep = g.snap.info.clone();
                        bump(&mut g, Phase::Error(e), keep);
                    }
                }
            }
        });
    });
}
