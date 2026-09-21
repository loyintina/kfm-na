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

use crate::endpoint::EndpointKind;
use crate::settings::Backend;
use crate::sys_hist::{self, Hist};

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

/// sys JSON → na_sys::SysInfo（A 档纯函数，环境卡数据面）：契约 = 主键
/// 必须在（缺 load 键 = 对面不是新版 na-server，报错显形——与
/// parse_health 同纪律）；值 null = 该路采不到的合法显形 → None
/// （卡面显「—」）；非 JSON/类型错才报错。数值键缺键同归 None
/// （旧版缺面 404 已在 HTTP 层挡住，到这里缺键 = 对端半成品，
/// 占位不编造）。2026-09-20 三路扩键（procs/swap_*/uptime_s）缺键
/// 同归 None——旧版 na-server 不认新键，契约向旧兼容不破
pub fn parse_sys(json: &str) -> Result<na_sys::SysInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("sys 不是合法 JSON: {e}"))?;
    let procs = match v.get("procs") {
        Some(p) if p.is_array() => {
            let a = p.as_array().unwrap();
            match (
                a.first().and_then(|x| x.as_u64()),
                a.get(1).and_then(|x| x.as_u64()),
            ) {
                (Some(r), Some(t)) => Some((r, t)),
                _ => None,
            }
        }
        _ => None,
    };
    let load = match v.get("load") {
        None => return Err("sys 缺 load".into()),
        Some(l) if l.is_null() => None,
        Some(l) => {
            let a = l.as_array().ok_or_else(|| "sys load 非数组".to_string())?;
            if a.len() < 3 {
                return Err("sys load 不足三段".into());
            }
            let f = |i: usize| a[i].as_f64().ok_or_else(|| format!("load[{i}] 非数字"));
            Some(na_sys::LoadAvg {
                l1: f(0)?,
                l5: f(1)?,
                l15: f(2)?,
                procs,
            })
        }
    };
    let kb = |k: &str| v.get(k).and_then(|x| x.as_u64());
    let swap = match (kb("swap_total_kb"), kb("swap_free_kb")) {
        (Some(t), Some(f)) => Some((t, f)),
        _ => None,
    };
    let mem = match (kb("mem_total_kb"), kb("mem_avail_kb")) {
        (Some(total_kb), Some(avail_kb)) => Some(na_sys::MemInfo {
            total_kb,
            avail_kb,
            swap,
        }),
        _ => None,
    };
    let disk = match (kb("disk_total_b"), kb("disk_avail_b")) {
        (Some(t), Some(a)) => Some((t, a)),
        _ => None,
    };
    let uptime_s = kb("uptime_s");
    // 核数（2026-09-21 负载判色）：旧版 na-server 缺键 = None → 负载轨
    // 回退窗内峰值归一 + 中性档（契约向旧兼容不破，同 procs/swap 规）
    let cores = kb("cores")
        .filter(|n| *n > 0)
        .map(|n| n.min(u32::MAX as u64) as u32);
    Ok(na_sys::SysInfo {
        load,
        mem,
        disk,
        uptime_s,
        cores,
    })
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

/// 环境体征快照（环境卡数据源）：与 health 同轮询器同节拍，
/// 错误保留旧数据（闪断不清卡面纪律同 health）
#[derive(Debug, Clone)]
pub struct SysSnap {
    pub sys: Option<na_sys::SysInfo>,
    pub epoch: u64,
}

/// 历史账落盘路径（Android 沙箱私有目录；宿主/测试环境该路径不可写 =
/// 静默失败——落盘是缓存不是账本，读写失败一律吞掉不连坐进程）
pub const HIST_PATH: &str = "/data/data/dev.kfm.na/files/sys-hist.txt";
/// 落盘节流（拍）：每 15 拍（=30s）写一次——最坏丢 30s 历史，闪存写入
/// 也不至于 2s 一刷
const SAVE_EVERY: u32 = 15;

struct Inner {
    cfg_backend: Backend,
    cfg_port: u16,
    visible: bool,
    snap: HealthSnap,
    sys: SysSnap,
    /// 环境体征历史环形账（环境卡柱轨数据面，2026-09-21）：**对象轴两相
    /// 各一本**（hist_idx——服务器/本地互不清带，切环境各自续摊）；每采到
    /// 一拍即追（值不变照追——kfmv4「时钟驱动滑动」同规）
    hist: [Hist; 2],
    /// Server 账的归属（nasup 目标串）：换服务器 = 清账（别人的体征
    /// 曲线不许续在自己账上）；空串 = 归属未知不判
    hist_target: String,
    /// 落盘节流计数
    hist_writes: u32,
}

static INNER: OnceLock<Arc<Mutex<Inner>>> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);

fn inner() -> &'static Arc<Mutex<Inner>> {
    INNER.get_or_init(|| {
        // 落盘恢复（「别做从左长，最好是默认就是铺开的」）：进程重启不
        // 清零柱轨历史；读不到/版本不认 = 从零攒（缓存不是账本）
        let loaded = load_hist();
        Arc::new(Mutex::new(Inner {
            cfg_backend: Backend::Kfmv4,
            cfg_port: 9021,
            visible: false,
            snap: HealthSnap {
                phase: Phase::Kfmv4,
                info: None,
                epoch: 0,
            },
            sys: SysSnap {
                sys: None,
                epoch: 0,
            },
            hist: loaded.0,
            hist_target: loaded.1,
            hist_writes: 0,
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

fn bump_sys(g: &mut Inner, sys: Option<na_sys::SysInfo>) {
    if g.sys.sys != sys {
        g.sys.sys = sys;
        g.sys.epoch += 1;
        DIRTY.store(true, Ordering::Relaxed);
    }
}

/// 目标机标识（Server 账归属唯一源：nasup 目标串；supervisor 未起 =
/// 空串「归属未知」，此时不判归属不误清账）
fn server_target() -> String {
    crate::na_server_sup::snap()
        .map(|s| s.lock().unwrap().target.clone())
        .unwrap_or_default()
}

/// 落盘（失败吞掉）
fn save_hist(rings: &[Hist; 2], target: &str) {
    let text = sys_hist::encode_hist([&rings[0], &rings[1]], [target, ""]);
    let _ = std::fs::write(HIST_PATH, text);
}

/// 读盘（读不到 = 空账；版本不认在 decode 里整份弃）
fn load_hist() -> ([Hist; 2], String) {
    match std::fs::read_to_string(HIST_PATH) {
        Ok(t) => {
            let (rings, targets) = sys_hist::decode_hist(&t);
            (rings, targets[0].clone())
        }
        Err(_) => ([Hist::default(), Hist::default()], String::new()),
    }
}

/// 追一拍历史（环境卡柱轨）：值不变也追（拍序 = 时间轴），并置脏——
/// 追拍本身就是屏上内容换代（柱轨右移一柱）。落盘节流写。
fn push_hist(g: &mut Inner, kind: EndpointKind, s: sys_hist::Sample) {
    let tgt = server_target();
    if kind == EndpointKind::Server && !tgt.is_empty() && tgt != g.hist_target {
        g.hist[0].clear(); // 换服务器 = 清账（对象轴两相各一本之外的第三清账点）
        g.hist_target = tgt;
    }
    g.hist[sys_hist::hist_idx(kind)].push(s);
    g.hist_writes += 1;
    DIRTY.store(true, Ordering::Relaxed);
    if g.hist_writes >= SAVE_EVERY {
        g.hist_writes = 0;
        let (rings, target) = (g.hist.clone(), g.hist_target.clone());
        std::thread::spawn(move || save_hist(&rings, &target));
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
        bump_sys(&mut g, None); // 后端翻相清体征（kfmv4 时代的不许带进 na-server 相）
        // 柱轨不清账（2026-09-21 v2）：两相各一本历史账，翻相取另一本
        // 即天然清账；同一相翻后端（na-server/kfmv4 托管）= 同一台机器，
        // 历史留着（托管相 hist() 返空账不画陈旧柱，见下）
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

/// 读环境体征快照（环境卡涂装每烘焙拍一张；锁短）
pub fn sys_snap() -> SysSnap {
    inner().lock().unwrap().sys.clone()
}

/// 读环境体征历史账（柱轨涂装/合成期每帧一张；CAP 级样本克隆，便宜）。
/// 取**当前对象相**那本（服务器/本地各一本）；服务器相 + 后端非
/// na-server（kfmv4 托管，无体征面）= 空账——不画陈旧柱（账留着，
/// 翻回 na-server 后端即续摊）
pub fn hist() -> Hist {
    let kind = crate::endpoint::current();
    let g = inner().lock().unwrap();
    if kind == EndpointKind::Server && g.cfg_backend != Backend::NaServer {
        return Hist::default();
    }
    g.hist[sys_hist::hist_idx(kind)].clone()
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

/// GET 一个 JSON 面拿回 body（B 档胶水）：连接/写/读全带超时，
/// 非 200 即错（旧版 na-server 缺 sys 面 = 404 在这里显形）
fn http_get(port: u16, path: &str) -> Result<String, String> {
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let st =
        TcpStream::connect_timeout(&addr, HTTP_TIMEOUT).map_err(|e| format!("连接失败: {e}"))?;
    st.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    st.set_write_timeout(Some(HTTP_TIMEOUT)).ok();
    let req = crate::http1::serialize_request(&crate::http1::Request {
        method: "GET".into(),
        path: path.into(),
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
    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn ensure_poller() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        std::thread::spawn(|| {
            let mut last_poll: Option<Instant> = None;
            let mut was_on = false;
            let mut last_kind: Option<crate::endpoint::EndpointKind> = None;
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let (backend, port, visible) = {
                    let g = inner().lock().unwrap();
                    (g.cfg_backend, g.cfg_port, g.visible)
                };
                // 对象轴翻相清账（两轴第 6 步③：服务器体征不许带进
                // 本地相，互不清带）；翻相即拍（last_poll 勾销）。
                // 柱轨历史账不在此清（两相各一本，取另一本 = 天然清账）
                let kind = crate::endpoint::current();
                if last_kind != Some(kind) {
                    last_kind = Some(kind);
                    last_poll = None;
                    let mut g = inner().lock().unwrap();
                    bump_sys(&mut g, None);
                }
                // 节拍闸（2026-09-21 v2「默认铺开」）：**解析页可见 或
                // 应用在前台**都要拍——前台即在后台攒柱轨历史（用户
                // 拍板「别做从左长，最好是默认就是铺开的」：历史铺开度
                // = 手上拍数，退后台才停轮（零后台流量）；health 面
                // 仍只认页可见（卡面才用它）
                let fg = crate::gate::foreground();
                if !visible && !fg {
                    was_on = false;
                    continue;
                }
                // 上升沿立即一拍；之后 2s 一拍
                let due = !was_on
                    || last_poll.is_none_or(|t| t.elapsed() >= Duration::from_secs(POLL_SECS));
                was_on = true;
                if !due {
                    continue;
                }
                last_poll = Some(Instant::now());
                // 对象轴分流：本地相体征 = na_sys 直读（/proc + statvfs
                // 本机 /data，零网络零服务器——本机体征不需要任何
                // 后端）；服务器相 = 既有 HTTP 轮询（后端非 na-server
                // 不轮，kfmv4 托管态不变）。health 面本地相内容归
                // 第 6 步④，本地相分支不轮 health（服务器账留到
                // 翻回或④接线，闪断不清卡面纪律同构）
                if kind == crate::endpoint::EndpointKind::Local {
                    let info = na_sys::collect("/data");
                    let mut g = inner().lock().unwrap();
                    bump_sys(&mut g, Some(info));
                    push_hist(&mut g, kind, sys_hist::sample_of(&info)); // 柱轨同拍追
                    continue;
                }
                if backend != Backend::NaServer {
                    // 托管相复位节拍账（旧行为保鲜：翻回 na-server
                    // 后端即拍，不白等一拍）
                    last_poll = None;
                    was_on = false;
                    continue;
                }
                // health 面：只喂卡面，页不可见不白轮（前台攒历史不需要它）
                if visible {
                    match http_get(port, "/api/na/health").and_then(|b| parse_health(&b)) {
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
                // 环境体征同拍轮（环境卡数据面）：错误只报不换位，
                // 旧数据保留（闪断不清卡面纪律同 health）
                match http_get(port, "/api/na/sys").and_then(|b| parse_sys(&b)) {
                    Ok(info) => {
                        let mut g = inner().lock().unwrap();
                        bump_sys(&mut g, Some(info));
                        push_hist(&mut g, kind, sys_hist::sample_of(&info)); // 柱轨同拍追
                    }
                    Err(e) => {
                        crate::report::report("svchealth", &format!("sys 轮询失败: {e}"));
                    }
                }
            }
        });
    });
}
