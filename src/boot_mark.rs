//! boot_mark.rs — BAR-256 冷启动引导附着账（追踪信 NA0185；设计楼3，
//! 白露楼4 改 Pending 不落 AI 路，南舟楼2 三判据点）。
//!
//! 病灶：冷启动动态引导（cmd_bootstrap）的会话裁决在服务器 PTY 里发生，
//! 壳不知道结果——remote_attached 恒 None，路由读数与用户所见矛盾
//! （屏上是 tmux 会话窗，发送却按「裸 shell」落 AI 页）。
//!
//! 修法：引导脚本在 exec 前打 OSC 私有键标记（载荷＝附着的会话名／裸壳
//! 哨兵）；壳侧剥离器在**泵内分发之前**剥壳——显示 sink／飞行记录 rec／
//! 待机 replay 三消费方全净（白露①：一处剥壳，判卷凭据也不留标记）。
//!
//! 账取四值（南舟①＋白露②）：
//! - Pending：窗内未到——路由 Hold 留栏不吞字（**不落 AI 路**，楼4：
//!   不知道≠知道没有，不许替用户选目的地）；
//! - Stripped(名)：已剥净——路由窗路／壳扶正 remote_attached；
//! - Missed：超窗未收到——AI 路＋「引导账未回」提示＋报表行，**不静默**；
//! - Bad：载荷非法——同 Missed 且报表带原文。
//!
//! 窗是时刻量（南舟②）：起算点＝conn 层 spawn_terminal_session 携标记
//! 命令那一拍（`open`）；窗长 WINDOW_MS（初值 15s，实测定稿随落地楼）；
//! 超窗落 Missed（`tick`，值守线程 300ms 驱动）。
//!
//! 失效安全（楼3）：窗外到达的标记不记账、原样穿到网格（alacritty 对
//! 未知 OSC 静默忽略）；仿冒/巧合载荷只在窗内有效，窗长即攻击面。

use std::collections::HashMap;
use std::sync::Mutex;

/// OSC 私有键引入子：ESC ] 7777 ; kfmboot=（7777＝私有号段，真终端静默忽略）
pub const INTRO: &str = "\x1b]7777;kfmboot=";
/// 标记护栏上限（字节）：从引入子起积累超过它仍未见终结符 = 剥不动，
/// 已吞字节原样吐回（白露③ fail-open＋留痕），状态机复位继续直通
pub const MARK_CAP: usize = 64;
/// 宽限窗长（毫秒）初值：无会话时引导要跑 veran_resolve（楼3）；
/// 定稿依据（哪几台、什么值、为什么不是 8s/30s）随落地楼给
pub const WINDOW_MS: u64 = 15_000;

/// 附着账四值（None 永不再当「裸 shell」讲）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootMark {
    /// 窗内未到
    Pending,
    /// 已剥净（值＝会话名，或 BOOTSTRAP_SHELL_SENTINEL＝服务器判了裸壳）
    Stripped(String),
    /// 超窗未收到（终态，留痕由 tick 调用方做）
    Missed,
    /// 收到但载荷非法（值＝原文；结构合法的剥离才会进这里）
    Bad(String),
}

/// take_apply 的取用结果（壳扶正附着账用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Apply {
    /// 服务器裁决附着了该会话
    Session(String),
    /// 服务器裁决落裸 shell（哨兵标——已知裸壳，不是「不知道」）
    Bare,
}

/// record 裁决：Kept＝剥下的标记被账收下（字节就此消失）；
/// Rejected＝无账/超窗/非 Pending——调用方把原字节透回下游
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record {
    Kept,
    Rejected,
}

/// 剥壳产物段：Clean＝可下发三消费方的净字节；Mark＝剥出的完整标记
/// （raw＝原始字节——Rejected 时原样透回；payload＝引入子与终结符之间）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Clean(String),
    Mark { raw: String, payload: String },
}

// ---- 剥离器（流式状态机，纯逻辑零 IO） ----

/// 逐会话一只（泵槽内）。只缓存「仍可能是标记」的最短尾，不吞显示字节
pub struct BootStrip {
    hold: String,
}

impl BootStrip {
    pub const fn new() -> Self {
        Self {
            hold: String::new(),
        }
    }

    /// 喂一串字节事件，出有序段流（Clean/Mark 相间，拼接无损）。
    /// 普通 OSC 一个字节不动（只认自家私有键）；剥不动超上限原样吐回。
    pub fn feed(&mut self, chunk: &str) -> Vec<Seg> {
        let mut buf = std::mem::take(&mut self.hold);
        buf.push_str(chunk);
        let mut segs = Vec::new();
        let mut clean = String::new();
        let mut pos = 0usize;
        while pos < buf.len() {
            let Some(i) = buf[pos..].find('\x1b') else {
                clean.push_str(&buf[pos..]);
                pos = buf.len();
                break;
            };
            let at = pos + i;
            let rest = &buf[at..];
            if INTRO.starts_with(rest) {
                // 引入子前缀悬尾：净段先落，前缀留仓等下一串
                clean.push_str(&buf[pos..at]);
                pos = at;
                break;
            }
            if !rest.starts_with(INTRO) {
                // 别家序列（普通 OSC/CSI/……）：ESC 原样过，继续扫下一段。
                // 别家 OSC 体内不会再有 ESC 起头的我家引入子（终结符先于它）
                clean.push_str(&buf[pos..at + 1]);
                pos = at + 1;
                continue;
            }
            // 全引入子在位：净段先落，再找终结符（BEL 或 ST，先到者胜）
            clean.push_str(&buf[pos..at]);
            let body = at + INTRO.len();
            let bel = buf[body..].find('\x07').map(|k| body + k);
            let stt = buf[body..].find("\x1b\\").map(|k| body + k);
            let (term, tend) = match (bel, stt) {
                (Some(b), Some(s)) if b < s => (b, b + 1),
                (Some(_), Some(s)) => (s, s + 2),
                (Some(b), None) => (b, b + 1),
                (None, Some(s)) => (s, s + 2),
                (None, None) => {
                    if buf.len() - at > MARK_CAP {
                        // 剥不动：从引入子起全吐回，复位直通（白露③）
                        clean.push_str(&buf[at..]);
                        pos = buf.len();
                    } else {
                        // 悬尾从引入子起留仓（净段已落仓，不重复留）
                        pos = at;
                    }
                    break;
                }
            };
            if term - body > MARK_CAP {
                // 载荷超长判剥不动：标记区（含终结符）全算显示内容
                clean.push_str(&buf[at..tend]);
                pos = tend;
                continue;
            }
            if !clean.is_empty() {
                segs.push(Seg::Clean(std::mem::take(&mut clean)));
            }
            segs.push(Seg::Mark {
                raw: buf[at..tend].to_string(),
                payload: buf[body..term].to_string(),
            });
            pos = tend;
        }
        if pos < buf.len() {
            self.hold = buf[pos..].to_string();
        }
        if !clean.is_empty() {
            segs.push(Seg::Clean(clean));
        }
        segs
    }
}

impl Default for BootStrip {
    fn default() -> Self {
        Self::new()
    }
}

// ---- 四值账（每腿一户；只有 remote 腿走引导会开户） ----

struct Acct {
    state: BootMark,
    opened_ms: u64,
    deadline_ms: u64,
    applied: bool,
}

static REG: Mutex<Option<HashMap<&'static str, Acct>>> = Mutex::new(None);

fn reg() -> std::sync::MutexGuard<'static, Option<HashMap<&'static str, Acct>>> {
    REG.lock().unwrap()
}

/// 开户（起算点＝spawn 携标记命令那一拍）：Pending，窗 WINDOW_MS。
/// 重开户＝覆盖（重孵＝新窗，旧账遗物不记账）
pub fn open(leg: &'static str, now_ms: u64) {
    reg().get_or_insert_with(HashMap::new).insert(
        leg,
        Acct {
            state: BootMark::Pending,
            opened_ms: now_ms,
            deadline_ms: now_ms + WINDOW_MS,
            applied: false,
        },
    );
}

/// 销户（手动 attach/脱离压过引导账——真附着不许被旧账改判）
pub fn close(leg: &str) {
    if let Some(m) = reg().as_mut() {
        m.remove(leg);
    }
}

/// 读账（路由判定用；不消费、不迁移）
pub fn peek(leg: &str) -> Option<BootMark> {
    reg().as_ref()?.get(leg).map(|a| a.state.clone())
}

/// 超窗迁移（值守驱动）：Pending 且过窗 → Missed，返回
/// (起算点, 窗长) 供留痕（白露②：窗内未记到 ⇒ 留痕，不写「尽力」）。
/// Missed/Bad/Stripped 是终态不再迁移
pub fn tick(leg: &str, now_ms: u64) -> Option<(u64, u64)> {
    let mut g = reg();
    let a = g.as_mut()?.get_mut(leg)?;
    if a.state != BootMark::Pending || now_ms <= a.deadline_ms {
        return None;
    }
    a.state = BootMark::Missed;
    Some((a.opened_ms, a.deadline_ms - a.opened_ms))
}

/// 剥出的标记交给账：窗内 Pending 才收（Kept）；载荷非法落 Bad；
/// 无账/超窗/已终态 ⇒ Rejected（泵把原字节透回网格）
pub fn record(leg: &str, payload: &str, now_ms: u64) -> Record {
    let mut g = reg();
    let Some(a) = g.as_mut().and_then(|m| m.get_mut(leg)) else {
        return Record::Rejected;
    };
    if a.state != BootMark::Pending || now_ms > a.deadline_ms {
        return Record::Rejected;
    }
    // 载荷校验：非空、无控制符（BEL/ESC/换行都不许——名字与哨兵都是
    // 可打印字符；非法即 Bad，报红不静默）
    if payload.is_empty() || payload.chars().any(|c| c < '\x20' || c == '\x7f') {
        a.state = BootMark::Bad(payload.to_string());
    } else {
        a.state = BootMark::Stripped(payload.to_string());
    }
    Record::Kept
}

/// 壳取用（每圈一拍，幂等——取过一次不再给）：Stripped(名) → Session；
/// Stripped(哨兵) → Bare；其余（Pending/Missed/Bad/无账）→ None
pub fn take_apply(leg: &str) -> Option<Apply> {
    let mut g = reg();
    let a = g.as_mut()?.get_mut(leg)?;
    if a.applied {
        return None;
    }
    let BootMark::Stripped(p) = &a.state else {
        return None;
    };
    a.applied = true;
    if p == crate::tmux_ctl::BOOTSTRAP_SHELL_SENTINEL {
        Some(Apply::Bare)
    } else {
        Some(Apply::Session(p.clone()))
    }
}
