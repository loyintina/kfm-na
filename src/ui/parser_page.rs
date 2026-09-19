//! parser_page.rs — 解析页内容核（tmux 插件 v1，2026-09-19 用户立项：
//! 「右侧 tmux 插件 = 窗口管理器 + 重排按钮」，对象模型取 nz
//! tmux-tabs v2.1 会话版：标签 = 服务器全部 tmux 会话）
//!
//! 本册 = 状态核 + 几何/命中（A 档纯逻辑）；执行走 tmux_exec（B 档），
//! 涂装在 termview（眼手同尺：两边吃本册同一份 layout）。壳接线的
//! 动作语义（attach 重开连接/重排钉网格）注释见各方法。
//!
//! 布局（网格制，宪法三行块/最小 3 格纪律）：
//!   首行布局区 = 页标题「解析 · tmux」（涂装侧画，本册不算几何）；
//!   标题下 1 格 = tmux 插件卡（二级框，动态高 = 内容定，上限池区）：
//!     卡头行（「tmux · N 会话」/ 状态行）→ 会话行表（行尾 ×）→
//!     命名行（命名态）/ 确认带（确认态）→ 按钮带（常态 [重排][+新窗][↻]；
//!     命名/确认态 [确定][取消]）。

use crate::termview::{CELL_H, CELL_W};
use crate::tmux_ctl::TmuxSession;
use crate::ui::dual_pool::{self, PoolRect};
use std::sync::{Arc, Mutex};

/// 卡头/会话行/命名行/确认带行高 = 2 格
pub const ROW_H: u32 = CELL_H * 2;
/// 按钮高 = 3 格（宪法「最小的框 ≥3 格」）
pub const BTN_H: u32 = CELL_H * 3;
/// 行间距
pub const ROW_GAP: u32 = CELL_W;
/// 按钮间距
pub const BTN_GAP: u32 = CELL_W * 3;
/// 卡内上下留白
pub const CARD_PAD_V: u32 = CELL_H;
/// 卡内左右内缩
pub const CARD_PAD_H: u32 = CELL_W * 2;
/// 行尾 × 命中宽
pub const KILL_W: u32 = CELL_W * 4;

/// 卡片模式（按钮带语义随模式换）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Naming,
    Confirming,
}

/// 模式 → 按钮标签（涂装/命中唯一源——两处各写一份必漂移）
pub fn button_labels(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Normal => &["重排", "+新窗", "↻"],
        Mode::Naming => &["确定", "取消"],
        Mode::Confirming => &["确定关闭", "取消"],
    }
}

/// 命中结果（Button 的语义按当时 Mode 由 button_action 解）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Session(usize),
    Kill(usize),
    Button(usize),
}

/// 按钮动作（壳动作分发表）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Reflow,
    New,
    Refresh,
    NamingOk,
    NamingCancel,
    ConfirmOk,
    ConfirmCancel,
}

pub fn button_action(mode: Mode, i: usize) -> Option<Action> {
    match (mode, i) {
        (Mode::Normal, 0) => Some(Action::Reflow),
        (Mode::Normal, 1) => Some(Action::New),
        (Mode::Normal, 2) => Some(Action::Refresh),
        (Mode::Naming, 0) => Some(Action::NamingOk),
        (Mode::Naming, 1) => Some(Action::NamingCancel),
        (Mode::Confirming, 0) => Some(Action::ConfirmOk),
        (Mode::Confirming, 1) => Some(Action::ConfirmCancel),
        _ => None,
    }
}

/// 一页布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct Layout {
    pub card: PoolRect,
    pub header: PoolRect,
    pub rows: Vec<PoolRect>,
    pub naming: Option<PoolRect>,
    pub confirm: Option<PoolRect>,
    pub buttons: Vec<PoolRect>,
    /// 可见行数（卡片高超池区时截断——v1 不滚动，超出会话看不着，
    /// 挂账：会话 >max_visible 时加行滚动）
    pub visible_rows: usize,
}

/// 布局纯函数：卡片外区 = 双池同一池区（标题下 1 格起）；卡高 = 内容
/// 定、上限池区高；可见行数按剩余高度截
pub fn layout(
    screen_w: u32,
    screen_h: u32,
    bottom_inset: u32,
    n_sessions: usize,
    mode: Mode,
) -> Layout {
    let area = dual_pool::pool_area(screen_w, screen_h, bottom_inset);
    let extra = match mode {
        Mode::Normal => 0,
        Mode::Naming | Mode::Confirming => ROW_H + ROW_GAP,
    };
    let fixed = CARD_PAD_V * 2 + ROW_H + ROW_GAP + extra + BTN_H;
    let stride = ROW_H + ROW_GAP;
    let max_rows = if area.h > fixed {
        ((area.h - fixed + ROW_GAP) / stride).max(1) as usize
    } else {
        1
    };
    let visible_rows = n_sessions.min(max_rows);
    let content_h = fixed + stride * visible_rows as u32;
    let card_h = content_h.min(area.h);
    let card = PoolRect {
        x: area.x,
        y: area.y,
        w: area.w,
        h: card_h,
    };
    let cx = card.x + i64::from(CARD_PAD_H);
    let cw = card.w.saturating_sub(CARD_PAD_H * 2);
    let mut y = card.y + i64::from(CARD_PAD_V);
    let header = PoolRect {
        x: cx,
        y,
        w: cw,
        h: ROW_H,
    };
    y += i64::from(ROW_H + ROW_GAP);
    let mut rows = Vec::with_capacity(visible_rows);
    for _ in 0..visible_rows {
        rows.push(PoolRect {
            x: cx,
            y,
            w: cw,
            h: ROW_H,
        });
        y += i64::from(stride);
    }
    let naming = (mode == Mode::Naming).then(|| {
        let r = PoolRect {
            x: cx,
            y,
            w: cw,
            h: ROW_H,
        };
        y += i64::from(stride);
        r
    });
    let confirm = (mode == Mode::Confirming).then(|| {
        let r = PoolRect {
            x: cx,
            y,
            w: cw,
            h: ROW_H,
        };
        y += i64::from(stride);
        r
    });
    let labels = button_labels(mode);
    let n = labels.len() as u32;
    let bw = cw.saturating_sub(BTN_GAP * (n - 1)) / n;
    let mut buttons = Vec::with_capacity(labels.len());
    for i in 0..labels.len() {
        buttons.push(PoolRect {
            x: cx + (bw + BTN_GAP) as i64 * i as i64,
            y,
            w: bw,
            h: BTN_H,
        });
    }
    Layout {
        card,
        header,
        rows,
        naming,
        confirm,
        buttons,
        visible_rows,
    }
}

/// 命中（x/y 屏坐标 i64，与涂装同一份 Layout）
pub fn hit(l: &Layout, x: i64, y: i64) -> Option<Hit> {
    let inside =
        |r: &PoolRect| x >= r.x && x < r.x + i64::from(r.w) && y >= r.y && y < r.y + i64::from(r.h);
    for (i, r) in l.rows.iter().enumerate() {
        if inside(r) {
            // 行尾 × 带 = Kill；其余 = Session
            if x >= r.x + i64::from(r.w) - i64::from(KILL_W) {
                return Some(Hit::Kill(i));
            }
            return Some(Hit::Session(i));
        }
    }
    for (i, r) in l.buttons.iter().enumerate() {
        if inside(r) {
            return Some(Hit::Button(i));
        }
    }
    None
}

/// 查询状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// 从未查询（壳见 Idle + 远程配置在 = 触发首查）
    Idle,
    Loading,
    Ready,
    Error(String),
}

/// 页面状态核（一切变更 bump epoch——涂装 sig 的唯一代际源）
pub struct ParserPage {
    sessions: Vec<TmuxSession>,
    status: Status,
    /// 本端附着的会话名（壳从远程启动命令提取/attach 后更新）
    attached: Option<String>,
    /// 命名输入内容（Some = 命名态，IME 聚焦）
    naming: Option<String>,
    /// 关闭确认目标（sessions 下标）
    confirming: Option<usize>,
    epoch: u64,
}

/// 涂装快照（壳帧涂装吃 clone，锁短）
#[derive(Debug, Clone)]
pub struct ParserPageSnap {
    pub sessions: Vec<TmuxSession>,
    pub status: Status,
    pub attached: Option<String>,
    pub naming: Option<String>,
    pub confirming: Option<usize>,
    pub epoch: u64,
}

impl Default for ParserPage {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserPage {
    pub fn new() -> Self {
        ParserPage {
            sessions: Vec::new(),
            status: Status::Idle,
            attached: None,
            naming: None,
            confirming: None,
            epoch: 0,
        }
    }

    pub fn mode(&self) -> Mode {
        if self.naming.is_some() {
            Mode::Naming
        } else if self.confirming.is_some() {
            Mode::Confirming
        } else {
            Mode::Normal
        }
    }

    fn bump(&mut self) {
        self.epoch += 1;
    }

    pub fn set_loading(&mut self) {
        self.status = Status::Loading;
        self.bump();
    }

    pub fn set_sessions(&mut self, ss: Vec<TmuxSession>) {
        self.sessions = ss;
        self.status = Status::Ready;
        // 行表变了确认下标可能悬空——一律收
        self.confirming = None;
        self.bump();
    }

    pub fn set_error(&mut self, e: String) {
        self.status = Status::Error(e);
        self.bump();
    }

    pub fn set_attached(&mut self, name: Option<String>) {
        if self.attached != name {
            self.attached = name;
            self.bump();
        }
    }

    pub fn begin_naming(&mut self) {
        self.naming = Some(String::new());
        self.confirming = None;
        self.bump();
    }

    pub fn naming_push(&mut self, s: &str) {
        if let Some(n) = &mut self.naming {
            n.push_str(s);
            self.bump();
        }
    }

    pub fn naming_pop(&mut self) {
        if let Some(n) = &mut self.naming {
            n.pop();
            self.bump();
        }
    }

    /// 取名离开命名态（Some = 用户输入原文，清洗归 tmux_ctl::sanitize_name）
    pub fn naming_take(&mut self) -> Option<String> {
        let r = self.naming.take();
        self.bump();
        r
    }

    pub fn cancel_naming(&mut self) {
        if self.naming.take().is_some() {
            self.bump();
        }
    }

    pub fn naming_active(&self) -> bool {
        self.naming.is_some()
    }

    pub fn begin_confirm(&mut self, i: usize) {
        if i < self.sessions.len() {
            self.confirming = Some(i);
            self.bump();
        }
    }

    pub fn cancel_confirm(&mut self) {
        if self.confirming.take().is_some() {
            self.bump();
        }
    }

    /// 确认目标会话名（下标悬空 = None——壳按 None 收确认态）
    pub fn confirm_target(&self) -> Option<String> {
        self.confirming
            .and_then(|i| self.sessions.get(i))
            .map(|s| s.name.clone())
    }

    pub fn session_name(&self, i: usize) -> Option<String> {
        self.sessions.get(i).map(|s| s.name.clone())
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn attached(&self) -> Option<&str> {
        self.attached.as_deref()
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn snap(&self) -> ParserPageSnap {
        ParserPageSnap {
            sessions: self.sessions.clone(),
            status: self.status.clone(),
            attached: self.attached.clone(),
            naming: self.naming.clone(),
            confirming: self.confirming,
            epoch: self.epoch,
        }
    }
}

// ---- 全局句柄（cfg_page 同款注册模式：壳注册，涂装/命中/闸门取）----

pub type SharedParserPage = Arc<Mutex<ParserPage>>;

static HANDLE: Mutex<Option<SharedParserPage>> = Mutex::new(None);

pub fn register_parser_page(page: SharedParserPage) {
    *HANDLE.lock().unwrap() = Some(page);
}

pub fn parser_page_handle() -> Option<SharedParserPage> {
    HANDLE.lock().unwrap().clone()
}
