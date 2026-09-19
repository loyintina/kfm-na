//! parser_page.rs — 解析页内容核（tmux 插件 v1，2026-09-19 用户立项：
//! 「右侧 tmux 插件 = 窗口管理器 + 重排按钮」，对象模型取 nz
//! tmux-tabs v2.1 会话版：标签 = 服务器全部 tmux 会话）
//!
//! 本册 = 状态核 + 几何/命中（A 档纯逻辑）；执行走 tmux_exec（B 档），
//! 涂装在 termview（眼手同尺：两边吃本册同一份 layout）。壳接线的
//! 动作语义（attach 重开连接/重排钉网格）注释见各方法。
//!
//! 布局（网格制，宪法三行块/最小 3 格纪律）：
//!   首行布局区 = 空（页标题 2026-09-19 用户拍板撤掉，卡区上吞
//!   TAB_ROW_H——本册 layout 回收，涂装/命中同一份不破眼手同尺）；
//!   吞并后的标题行起 = tmux 插件卡（二级框，动态高 = 内容定，上限池区）：
//!     卡头行（「tmux · N 会话」/ 状态行）→ 会话框表（**一行两框**，
//!     每框 = 三级框行主形态全包框双色渐变，内容只有 名字+×——
//!     「·N窗/·他端」meta 2026-09-19 用户拍板撤；**可见窗上限 6 框**，
//!     超出内部滚动：框全量发出 + scroll 平移 + list_clip 裁/判）→
//!     **分隔线**（同日拍板：正渐变 c1→c2 底线家变异，非交互件）→
//!     命名行（命名态）→ 按钮带（常态 [重排][新窗]——↻ 刷新钮同日撤：
//!     列会话本就在开页/操作后自动刷，手动冗余；命名态 [确定][取消]）。
//!   关闭确认 = **跳框模态**（同日拍板：防误触，取代卡内确认带）——
//!   压暗层 + 居中卡 + [确定关闭][取消]，点框外 = 取消（跳框惯例）。

use crate::termview::{CELL_H, CELL_W};
use crate::tmux_ctl::TmuxSession;
use crate::ui::dual_pool::{self, PoolRect};
use std::sync::{Arc, Mutex};

/// 卡头/命名行高 = 2 格
pub const ROW_H: u32 = CELL_H * 2;
/// 会话框高 = 3 格（2026-09-19 用户修宪：所有三级框至少两行高——
/// 2 格观感仍是「一行高」，3 格 = 真·两行 + 上下各近一格空隙）
pub const BOX_H: u32 = CELL_H * 3;
/// 按钮高 = 3 格（宪法「最小的框 ≥3 格」）
pub const BTN_H: u32 = CELL_H * 3;
/// 行间距
pub const ROW_GAP: u32 = CELL_W;
/// 一行两框的列间距（2 格，与池卡左右间隔同档）
pub const COL_GAP: u32 = CELL_W * 2;
/// 按钮间距
pub const BTN_GAP: u32 = CELL_W * 3;
/// 卡内上下留白
pub const CARD_PAD_V: u32 = CELL_H;
/// 卡内左右内缩
pub const CARD_PAD_H: u32 = CELL_W * 2;
/// 框尾 × 命中宽
pub const KILL_W: u32 = CELL_W * 4;
/// 分隔线带高（会话框表与按钮带之间，线体在带内垂直居中）
/// ——2026-09-19 用户拍板：分隔线 = 底线家变异（正渐变 c1→c2 横向）；
/// 同日二拍：线体上下各留 ≥1 格净空（带 = 2 格，线居中 → 上下各 ~1 格）
pub const DIVIDER_ZONE: u32 = CELL_H * 2;
/// 分隔线线体厚
pub const DIVIDER_H: u32 = 2;
/// 会话框表可见行上限（一行两框 × 3 行 = 6 框；超出内部滚动，
/// 同日用户拍板取代「超池区截断看不见」挂账）
pub const MAX_VISIBLE_LINES: u32 = 3;

/// 卡片模式（按钮带语义随模式换；Confirming = 跳框模态在，卡区按钮不画
/// 不可点——模态屏蔽）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Naming,
    Confirming,
}

/// 模式 → 按钮标签（涂装/命中唯一源——两处各写一份必漂移）
pub fn button_labels(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Normal => &["重排", "新窗"],
        Mode::Naming => &["确定", "取消"],
        Mode::Confirming => &[],
    }
}

/// 命中结果（Button 的语义按当时 Mode 由 button_action 解；
/// Modal* = 关闭确认跳框的命中——模态在时卡区命中全屏蔽）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Session(usize),
    Kill(usize),
    Button(usize),
    /// 跳框 [确定关闭]
    ModalOk,
    /// 跳框 [取消]
    ModalCancel,
    /// 点跳框外 = 取消（跳框惯例）
    ModalDismiss,
}

/// 按钮动作（壳动作分发表）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Reflow,
    New,
    NamingOk,
    NamingCancel,
}

pub fn button_action(mode: Mode, i: usize) -> Option<Action> {
    match (mode, i) {
        (Mode::Normal, 0) => Some(Action::Reflow),
        (Mode::Normal, 1) => Some(Action::New),
        (Mode::Naming, 0) => Some(Action::NamingOk),
        (Mode::Naming, 1) => Some(Action::NamingCancel),
        _ => None,
    }
}

/// 一页布局（涂装/命中同一份——眼手同尺）
#[derive(Debug, Clone)]
pub struct Layout {
    pub card: PoolRect,
    pub header: PoolRect,
    /// 会话框（一行两框，按行主序排：偶数左列、奇数右列）——**全量**
    /// 发出（y 已减 eff_scroll），可见性靠 list_clip 裁/判
    pub rows: Vec<PoolRect>,
    /// 会话表与按钮带之间的分隔线（带内居中线体）
    pub divider: PoolRect,
    pub naming: Option<PoolRect>,
    pub buttons: Vec<PoolRect>,
    /// 会话框表纵裁剪带（涂装断墨/命中闸门同一份）：滚动后画出
    /// 带外的框不许显、不许点
    pub list_clip: (i64, i64),
    /// 滚动上限（总内容高超可见窗的部分；0 = 不可滚）
    pub scroll_max: i64,
    /// 可见窗容量（框数）= min(会话数, 可见行×2)——信息位，考题用
    pub visible_rows: usize,
}

/// 布局纯函数：卡片外区 = 双池同一池区（标题下 1 格起）；卡高 = 内容
/// 定、上限池区高。会话框表可见行 ≤ MAX_VISIBLE_LINES（6 框），超出
/// 内部滚动：scroll 为像素位移（壳手势喂入），本函数内部 clamp 后
/// 几何吃 eff_scroll——眼手同尺，钳制语义唯一
pub fn layout(
    screen_w: u32,
    screen_h: u32,
    bottom_inset: u32,
    n_sessions: usize,
    mode: Mode,
    scroll: i64,
) -> Layout {
    let area = dual_pool::pool_area(screen_w, screen_h, bottom_inset);
    // 页标题已撤（2026-09-19 用户拍板）：卡区上吞标题行高 TAB_ROW_H，
    // 卡顶对齐原首行布局区顶——池区其余三缘不动
    let area = PoolRect {
        y: area.y - i64::from(crate::ui::tab_bar::TAB_ROW_H),
        h: area.h + crate::ui::tab_bar::TAB_ROW_H,
        ..area
    };
    // 确认跳框是模态，不占卡内高度（卡按 Normal 几何画在模态底下）
    let extra = match mode {
        Mode::Normal | Mode::Confirming => 0,
        Mode::Naming => ROW_H + ROW_GAP,
    };
    let stride = BOX_H + ROW_GAP;
    // 卡高账：PAD_V·2 + 头 ROW_H + ROW_GAP + 可见行区 + 分隔线带 +
    // extra + 钮 BTN_H；可见行区 = stride·L − ROW_GAP（末行下不留行距）
    let fixed = CARD_PAD_V * 2 + ROW_H + DIVIDER_ZONE + extra + BTN_H;
    let max_lines = if area.h > fixed {
        ((area.h - fixed) / stride).max(1)
    } else {
        1
    };
    let cap_lines = max_lines.min(MAX_VISIBLE_LINES);
    let total_lines = n_sessions.div_ceil(2) as u32;
    // 可见窗高 = min(容量, 实际行数)——卡高随内容缩；滚动上限 =
    // 总内容高超可见窗的部分
    let used_lines = cap_lines.min(total_lines);
    let visible_h = stride.saturating_mul(used_lines).saturating_sub(ROW_GAP);
    let total_h = stride.saturating_mul(total_lines).saturating_sub(ROW_GAP);
    let scroll_max = i64::from(total_h.saturating_sub(visible_h));
    let eff_scroll = scroll.clamp(0, scroll_max);
    let content_h = fixed + visible_h + ROW_GAP;
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
    let list_top = y;
    let list_clip = (list_top, list_top + i64::from(visible_h));
    let bw = cw.saturating_sub(COL_GAP) / 2;
    let mut rows = Vec::with_capacity(n_sessions);
    for i in 0..n_sessions {
        let (line, col) = (i / 2, i % 2);
        rows.push(PoolRect {
            x: cx + (bw + COL_GAP) as i64 * col as i64,
            y: list_top + stride as i64 * line as i64 - eff_scroll,
            w: bw,
            h: BOX_H,
        });
    }
    y += i64::from(visible_h);
    let divider = PoolRect {
        x: cx,
        y: y + i64::from(DIVIDER_ZONE - DIVIDER_H) / 2,
        w: cw,
        h: DIVIDER_H,
    };
    y += i64::from(DIVIDER_ZONE);
    let naming = (mode == Mode::Naming).then(|| {
        let r = PoolRect {
            x: cx,
            y,
            w: cw,
            h: ROW_H,
        };
        y += i64::from(ROW_H + ROW_GAP);
        r
    });
    let labels = button_labels(mode);
    let mut buttons = Vec::with_capacity(labels.len());
    if let Some(n) = std::num::NonZeroU32::new(labels.len() as u32) {
        let btw = cw.saturating_sub(BTN_GAP * (n.get() - 1)) / n;
        for i in 0..labels.len() {
            buttons.push(PoolRect {
                x: cx + (btw + BTN_GAP) as i64 * i as i64,
                y,
                w: btw,
                h: BTN_H,
            });
        }
    }
    Layout {
        card,
        header,
        rows,
        divider,
        naming,
        buttons,
        list_clip,
        scroll_max,
        visible_rows: n_sessions.min(used_lines as usize * 2),
    }
}

/// 确认跳框几何（涂装/命中同一份）：卡宽 40 格居中、卡高 = 标题 2 格 +
/// 间距 + 双钮 3 格 + 上下留白（内容只有一句话「关闭 '名'？」放标题位）
pub const MODAL_CARD_W: u32 = CELL_W * 40;
pub const MODAL_TITLE_H: u32 = CELL_H * 2;
pub const MODAL_PAD_V: u32 = CELL_H;
pub const MODAL_GAP: u32 = CELL_H / 2;
pub const MODAL_BTN_GAP: u32 = CELL_W * 2;

pub fn confirm_card(screen_w: u32, screen_h: u32) -> PoolRect {
    let w = MODAL_CARD_W.min(screen_w.saturating_sub(CELL_W * 4));
    let h = MODAL_PAD_V * 2 + MODAL_TITLE_H + MODAL_GAP + BTN_H;
    PoolRect {
        x: (i64::from(screen_w) - i64::from(w)) / 2,
        y: (i64::from(screen_h) - i64::from(h)) / 2,
        w,
        h,
    }
}

/// 跳框双钮：[确定关闭][取消]（卡内底部一行，左右等宽）
pub fn confirm_buttons(card: &PoolRect) -> [PoolRect; 2] {
    let bw = (card.w.saturating_sub(CARD_PAD_H * 2 + MODAL_BTN_GAP)) / 2;
    let y = card.y + i64::from(card.h - MODAL_PAD_V - BTN_H);
    let x0 = card.x + i64::from(CARD_PAD_H);
    [
        PoolRect {
            x: x0,
            y,
            w: bw,
            h: BTN_H,
        },
        PoolRect {
            x: x0 + i64::from(bw + MODAL_BTN_GAP),
            y,
            w: bw,
            h: BTN_H,
        },
    ]
}

/// 跳框按钮标签（涂装/命中唯一源）
pub const CONFIRM_LABELS: [&str; 2] = ["确定关闭", "取消"];

/// 命中（x/y 屏坐标 i64，与涂装同一份 Layout）。模态（确认跳框）在时
/// 只认跳框：钮/卡内（吞）/卡外（Dismiss）；卡区命中全屏蔽
pub fn hit(l: &Layout, x: i64, y: i64, screen_w: u32, screen_h: u32, mode: Mode) -> Option<Hit> {
    let inside =
        |r: &PoolRect| x >= r.x && x < r.x + i64::from(r.w) && y >= r.y && y < r.y + i64::from(r.h);
    if mode == Mode::Confirming {
        let card = confirm_card(screen_w, screen_h);
        let btns = confirm_buttons(&card);
        if inside(&btns[0]) {
            return Some(Hit::ModalOk);
        }
        if inside(&btns[1]) {
            return Some(Hit::ModalCancel);
        }
        if inside(&card) {
            return None; // 卡内非钮区 = 吞掉，不许穿透
        }
        return Some(Hit::ModalDismiss);
    }
    for (i, r) in l.rows.iter().enumerate() {
        // 纵裁剪带外的框不许点（滚动后画出带上沿/下沿的半框只显不点
        // 会误触——命中闸门与涂装断墨同一份 list_clip，眼手同尺）
        if y < l.list_clip.0 || y >= l.list_clip.1 {
            continue;
        }
        if inside(r) {
            // 框尾 × 带 = Kill；其余 = Session
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
    /// 会话框表滚动位移（像素，≥0；上限 = layout().scroll_max——
    /// 几何侧另有一道 clamp，状态侧脏值不伤眼手同尺）
    scroll: i64,
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
    pub scroll: i64,
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
            scroll: 0,
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

    pub fn scroll(&self) -> i64 {
        self.scroll
    }

    /// 列表滚动（壳手势喂增量；max 吃 layout().scroll_max——壳每次
    /// 用当下几何算，状态核不揣屏寸）。变了才 bump（sig 鬼影纪律）
    pub fn scroll_by(&mut self, dy: i64, max: i64) {
        let ns = (self.scroll + dy).clamp(0, max.max(0));
        if ns != self.scroll {
            self.scroll = ns;
            self.bump();
        }
    }

    /// 行表刷新后钳回上限（会话变少 max 缩，壳拿新 layout 的
    /// scroll_max 调；变了才 bump）
    pub fn clamp_scroll(&mut self, max: i64) {
        let ns = self.scroll.clamp(0, max.max(0));
        if ns != self.scroll {
            self.scroll = ns;
            self.bump();
        }
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
            scroll: self.scroll,
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
