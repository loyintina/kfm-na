//! parser_page.rs — 解析页内容核（tmux 插件 v1，2026-09-19 用户立项：
//! 「右侧 tmux 插件 = 窗口管理器 + 重排按钮」，对象模型取 nz
//! tmux-tabs v2.1 会话版：标签 = 服务器全部 tmux 会话）
//!
//! 本册 = 状态核 + 几何/命中（A 档纯逻辑）；执行走 tmux_exec（B 档），
//! 涂装在 termview（眼手同尺：两边吃本册同一份 layout）。壳接线的
//! 动作语义（attach 重开连接/重排钉网格）注释见各方法。
//!
//! 布局（网格制，宪法三行块/最小 3 格纪律；2026-09-21 三区排布 v2，
//! 用户拍板「tmux 竖排放右下角常驻」）：
//!   首行布局区 = 空（页标题 2026-09-19 用户拍板撤掉，页全区上吞
//!   TAB_ROW_H——页级几何归 parser_chain::page_area，本册不揣度）；
//!   tmux 插件卡 = **右下常驻槽**（卡外框由 parser_chain::regions 配给，
//!   钉键盘感知可视底——永不被滚出屏，「右滑→点窗口」恒定两步）：
//!     卡头行（「tmux · N 会话」/ 状态行）→ 会话框表（**竖排一行一框**，
//!     每框 = 三级框行主形态全包框双色渐变，内容只有 名字+×——
//!     「·N窗/·他端」meta 2026-09-19 用户拍板撤；**可见窗上限 6 框**，
//!     超出内部滚动：框全量发出 + scroll 平移 + list_clip 裁/判）→
//!     **分隔线**（同日拍板：正渐变 c1→c2 底线家变异，非交互件）→
//!     命名行（命名态）→ 按钮带（常态 [重排][新窗]——↻ 刷新钮同日撤：
//!     列会话本就在开页/操作后自动刷，手动冗余；命名态 [确定][取消]）。
//!   关闭确认 = **跳框模态**（同日拍板：防误触，取代卡内确认带）——
//!   压暗层 + 居中卡 + [确定关闭][取消]，点框外 = 取消（跳框惯例）。
//!
//! **三区滚动（2026-09-21 v2，取代 09-20 单页 page_scroll）**：常驻槽
//! 不滚；左区（环境卡）/ 右上区（连接·服务卡）各一本滚动账，垂直
//! 拖拽按起手落点 x 命中分流（排布器 Scrolls/slot_rect/clip_of 唯一
//! 源）。键盘在场时三区窗同吃 bottom_inset 弹小（BAR-120 线视口化
//! 契约：区底 = 输入栏带以上）；卡布局不吃键盘（BAR-119 红线不动——
//! 只盖不重排）。手势仲裁：起手落会话框表带 = 表内滚动（既有），
//! 落左/右上区 = 对应区滚动，落常驻槽其余件 = 让回面板页。

use crate::termview::{CELL_H, CELL_W};
use crate::tmux_ctl::TmuxSession;
use crate::ui::dual_pool::PoolRect;
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
/// 列间距（2 格，与池卡左右间隔同档；link/sys 卡两竖列共用）
pub const COL_GAP: u32 = CELL_W * 2;
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
/// 会话框表可见行上限（竖排一行一框 × 6 行 = 6 框；超出内部滚动，
/// 同日用户拍板取代「超池区截断看不见」挂账。2026-09-21 三区 v2：
/// 一行两框 → 竖排一框，上限语义从「3 行」等价为「6 行」）
pub const MAX_VISIBLE_LINES: u32 = 6;
/// 可见行保底（2026-09-20 BAR-119 用户拍板）：键盘/chrome 任何纵向
/// 压力都不许把 tmux 卡吞到两行以下——它不再是纵向泄压阀；卡高随
/// 内容超池出屏（键盘遮盖），不许塌行自残
pub const MIN_VISIBLE_LINES: u32 = 2;

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
    /// 会话框（竖排一行一框）——**全量**发出（y 已减 eff_scroll），
    /// 可见性靠 list_clip 裁/判
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
    /// 可见窗容量（框数）= min(会话数, 可见行)——信息位，考题用
    pub visible_rows: usize,
}

/// 视口可视底（键盘感知）：屏高 − 底 inset（键盘+输入栏带）− 页环边距
/// − 页环底缘厚度 = 页环底内缘。布局账不吃它（BAR-119 红线不动：卡全价、
/// 只盖不重排），只有页面滚动窗吃——壳/手势/命中三处同此一份尺。
/// BAR-121 层级律：页环下沿必须压过内容——可视底让出底缘线厚
/// （AI_PAGE_FRAME_W），否则内容可画到环底线所在行，把环底边盖掉
/// （redroid 截屏定罪：中段框线被环境卡内芯盖掉，角弧区却干净）
pub fn visible_bottom(screen_h: u32, bottom_inset: u32) -> i64 {
    i64::from(screen_h)
        - i64::from(bottom_inset)
        - i64::from(crate::termview::AI_PAGE_FRAME_MARGIN)
        - i64::from(crate::termview::AI_PAGE_FRAME_W)
}

/// 按钮带行数（几何唯一源——卡高账与卡内布局同吃）：2026-09-21 v3
/// 用户拍板钮竖排（窄列并排两钮更窄——一钮一行全宽）；确认态 =
/// 跳框模态在，卡区按钮不画不可点，但卡按**常态**几何画在模态底下
/// （含两钮带占位——模态撤走零跳变）
fn btn_rows(mode: Mode) -> usize {
    match mode {
        Mode::Confirming => button_labels(Mode::Normal).len(),
        _ => button_labels(mode).len(),
    }
}

/// 按钮带块高（n 钮竖排：n 行 + (n-1) 行距；0 钮 = 0）
fn btn_block_h(mode: Mode) -> u32 {
    let n = btn_rows(mode) as u32;
    if n == 0 {
        0
    } else {
        n * BTN_H + (n - 1) * ROW_GAP
    }
}

/// 卡内几何账（A 档纯函数·唯一源）：(可见行数, 固定件高, 容量行数)。cap_h =
/// 常驻槽可用高（排布器以「可视底 − 区顶」喂入——**无键盘账**，
/// BAR-119 红线：卡内账不吃键盘 inset）；可见行 = min(容量, 实际)
/// 再抬到保底（BAR-119：挤压不许吞到两行以下；保底也不超实际行数
/// ——一行内容不强撑两行）；容量行数只随屏幕空间，与会话数脱钩
/// （BAR-145 顶锚修约的几何基石）
fn used_lines(n_sessions: usize, mode: Mode, cap_h: u32) -> (u32, u32, u32) {
    // 确认跳框是模态，不占卡内高度（卡按 Normal 几何画在模态底下）
    let extra = match mode {
        Mode::Normal | Mode::Confirming => 0,
        Mode::Naming => ROW_H + ROW_GAP,
    };
    let fixed = CARD_PAD_V * 2 + ROW_H + DIVIDER_ZONE + extra + btn_block_h(mode);
    let stride = BOX_H + ROW_GAP;
    let max_lines = if cap_h > fixed {
        ((cap_h - fixed) / stride).max(1)
    } else {
        1
    };
    let cap_lines = max_lines.min(MAX_VISIBLE_LINES);
    let total_lines = n_sessions as u32;
    let used = cap_lines
        .max(MIN_VISIBLE_LINES.min(total_lines))
        .min(total_lines);
    (used, fixed, cap_lines)
}

/// tmux 卡自报高（排布器三区几何的唯一输入——卡高 = 内容账全价，
/// BAR-119：不钳进池区，纵向压力不许本卡独吞塌行）。
/// **BAR-145 顶锚修约（2026-09-24 用户拍板）**：卡高吃**容量**不吃
/// 实际行数——会话数增减只影响列表区底部空位，已有行/分隔线/按钮
/// 一行一像素不挪。旧动态高（卡高随实际行数长）在底锚槽上是
/// 「名单翻动 → 全行位移 126px」的力学根源（点击行漂移病灶）
pub fn tmux_card_h(n_sessions: usize, mode: Mode, cap_h: u32) -> u32 {
    let (_used, fixed, cap_lines) = used_lines(n_sessions, mode, cap_h);
    let stride = BOX_H + ROW_GAP;
    let visible_h = stride.saturating_mul(cap_lines).saturating_sub(ROW_GAP);
    fixed + visible_h + ROW_GAP
}

/// 便捷壳（考题/调试专用）：屏寸 → 排布器三区 → 常驻槽卡内布局。
/// 产品涂装/命中/手势必须走 parser_chain::regions + layout_in 同路径
pub fn layout(
    screen_w: u32,
    screen_h: u32,
    bottom_inset: u32,
    n_sessions: usize,
    mode: Mode,
    scroll: i64,
) -> Layout {
    let vb = visible_bottom(screen_h, bottom_inset);
    let area = crate::ui::parser_chain::page_area(screen_w, screen_h, bottom_inset);
    // BAR-145 顶锚修约：卡高容量必须吃**无键盘**可视底（BAR-119 红线
    // 纯度——卡内账不吃键盘；钉底位移照吃，见 regions 的 vb）
    let cap = (visible_bottom(screen_h, 0) - area.y).max(0) as u32;
    let th = tmux_card_h(n_sessions, mode, cap);
    let regs = crate::ui::parser_chain::regions(screen_w, screen_h, bottom_inset, vb, th);
    layout_in(regs.dock, n_sessions, mode, scroll)
}

/// 卡内布局（2026-09-21 三区 v2）：卡外框由排布器配给（常驻槽
/// regions.dock——钉底/右列宽都已在排布器里约过，本卡不二次揣度），
/// 本函数只填卡内件。会话框竖排一行一框（宽 = 卡内全宽）；可见行
/// 上限 MAX_VISIBLE_LINES（6 框）、保底 MIN_VISIBLE_LINES，超出内部
/// 滚动：scroll 为像素位移（壳手势喂入），本函数内部 clamp 后几何吃
/// eff_scroll——眼手同尺，钳制语义唯一
pub fn layout_in(card: PoolRect, n_sessions: usize, mode: Mode, scroll: i64) -> Layout {
    let stride = BOX_H + ROW_GAP;
    // 可见行与卡高同源：cap = 卡高自身（排布器配给的 h 出自
    // tmux_card_h 同一份账，反推必一致）。**列表区高吃容量不吃实际**
    // （BAR-145 顶锚修约）：行顶锚排位、分隔线与按钮带对卡底钉死——
    // 会话数增减只改列表区底部空位，已有行一像素不挪
    let (used_lines_n, fixed, cap_lines) = used_lines(n_sessions, mode, card.h);
    let total_lines = n_sessions as u32;
    let visible_h = stride.saturating_mul(cap_lines).saturating_sub(ROW_GAP);
    let total_h = stride.saturating_mul(total_lines).saturating_sub(ROW_GAP);
    let scroll_max = i64::from(total_h.saturating_sub(visible_h));
    let eff_scroll = scroll.clamp(0, scroll_max);
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
    let mut rows = Vec::with_capacity(n_sessions);
    for i in 0..n_sessions {
        rows.push(PoolRect {
            x: cx,
            y: list_top + stride as i64 * i as i64 - eff_scroll,
            w: cw,
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
    // 按钮带（2026-09-21 v3 用户拍板竖排：窄列里并排两钮更窄——一钮
    // 一行全宽，纵序等 stride；确认态按常态几何排位，涂装吃
    // button_labels(mode) 空列 → 不画，命中模态臂屏蔽 → 不可点）
    let mut buttons = Vec::with_capacity(btn_rows(mode));
    for _ in 0..btn_rows(mode) {
        buttons.push(PoolRect {
            x: cx,
            y,
            w: cw,
            h: BTN_H,
        });
        y += i64::from(BTN_H + ROW_GAP);
    }
    let _ = fixed; // fixed 账只参与 used_lines 的可见行裁决，卡内件不直接消费
    Layout {
        card,
        header,
        rows,
        divider,
        naming,
        buttons,
        list_clip,
        scroll_max,
        visible_rows: n_sessions.min(used_lines_n as usize),
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
    /// 会话框表滚动位移（像素，≥0；上限 = layout_in().scroll_max——
    /// 几何侧另有一道 clamp，状态侧脏值不伤眼手同尺）
    scroll: i64,
    /// 左区滚动位移（2026-09-21 三区 v2，取代单页 page_scroll；上限 =
    /// parser_chain::scroll_max(Left)——几何侧同样有 clamp，脏值自愈）
    left_scroll: i64,
    /// 右上区滚动位移（同左区纪律）
    right_scroll: i64,
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
    pub left_scroll: i64,
    pub right_scroll: i64,
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
            left_scroll: 0,
            right_scroll: 0,
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

    pub fn left_scroll(&self) -> i64 {
        self.left_scroll
    }

    pub fn right_scroll(&self) -> i64 {
        self.right_scroll
    }

    /// 区滚动（壳手势喂增量，max 吃 parser_chain::scroll_max——壳每次
    /// 用当下几何算，状态核不揣屏寸）。变了才 bump（sig 鬼影纪律）
    pub fn left_scroll_by(&mut self, dy: i64, max: i64) {
        let ns = (self.left_scroll + dy).clamp(0, max.max(0));
        if ns != self.left_scroll {
            self.left_scroll = ns;
            self.bump();
        }
    }

    pub fn right_scroll_by(&mut self, dy: i64, max: i64) {
        let ns = (self.right_scroll + dy).clamp(0, max.max(0));
        if ns != self.right_scroll {
            self.right_scroll = ns;
            self.bump();
        }
    }

    /// 区内容高变了（行表刷新增减卡高）钳回上限——同 clamp_scroll 纪律
    pub fn clamp_left_scroll(&mut self, max: i64) {
        let ns = self.left_scroll.clamp(0, max.max(0));
        if ns != self.left_scroll {
            self.left_scroll = ns;
            self.bump();
        }
    }

    pub fn clamp_right_scroll(&mut self, max: i64) {
        let ns = self.right_scroll.clamp(0, max.max(0));
        if ns != self.right_scroll {
            self.right_scroll = ns;
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
            left_scroll: self.left_scroll,
            right_scroll: self.right_scroll,
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

// ---- 屏代账（BAR-145 仪器三期+修复，2026-09-24）：GPU 层当前纹理是哪
// 一代烘焙的——[touch] 行附「屏代N」，与活体 epoch 对表。**修复：
// 命中吃屏代快照（眼手同尺的真义）**——名单翻动（网络风暴期短命会话
// 起灭，9-23 夜实证 4↔5↔8 横跳）时，活体快照比屏上纹理新，用活体
// 命中 = 你点中的是你没看到的名单；吃屏代 = 你点中的就是你看到的，
// 屏在下一帧自追新名单

static BAKED_SNAP: Mutex<Option<ParserPageSnap>> = Mutex::new(None);

/// 烘焙完成落账（android_app slot_bake(Parser) 处唯一调用方）
pub fn note_baked_snap(snap: &ParserPageSnap) {
    *BAKED_SNAP.lock().unwrap() = Some(snap.clone());
}

/// 屏上正显示的那一代快照（解析页命中路径唯一合法源；无烘焙记录
/// = 页未上过屏，调用方回落活体）
pub fn baked_snap() -> Option<ParserPageSnap> {
    BAKED_SNAP.lock().unwrap().clone()
}

/// 当前屏上纹理的烘焙代（[touch] 遥测调用方）
pub fn baked_epoch() -> u64 {
    BAKED_SNAP.lock().unwrap().as_ref().map_or(0, |s| s.epoch)
}
