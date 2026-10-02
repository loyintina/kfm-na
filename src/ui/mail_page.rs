//! mail_page.rs — 信箱页状态核 + 可变高三栏卡几何（BAR-214，2026-10-01
//! 用户拍板：信箱入口点开的不再是压暗层弹框，而是与 AI 页/池卡/文件树卡/
//! 解析页同级的整页卡——右缘家，从解析页右边向左平移挤入；出场唯一 =
//! 右滑平移退出，中央终端页平移回来）。
//!
//! 条目 = 三级框信渲染器（用户原话逐条兑现）：
//! - **第一栏（最上）**：发信人 H1 半包框标题（剥部门留二字名）居左，
//!   时间居右（灰字、不带时区），正文字箭头 + 正文字收件人；两个收件人
//!   上下并置（本栏随收件人数长行），栏内字上下居中。
//! - **第二栏**：字头标题，引用灰字样式，超长格折行。
//! - **第三栏**：摘要，正文样式，超长格折行。
//! - **渲染器不设最大高度**——卡高 = 三栏实量之和。
//! - **按发信人取色**：accent::accent_for_sender（同名同色/异名异色/
//!   与页色脱撞），渐变暗底 + 框吃发信人双色；点开三级框的查看器跳框
//!   颜色继承本卡（ViewerSnap.accent 维，壳穿线）。
//!
//! 滚动/追底/摘要懒加载窗语义与 mail_list 同款（旧 veil 列表卡退役，
//! 本册是其整页卡形态的继承者）；数据 = mail_feed 零改动。
//!
//! 本册 = 状态核 + 几何/命中/信头纯件（A 档，tests/mail_page_spec 钉死）；
//! 涂装在 termview（眼手同尺同一份 lays）。

use crate::mail_feed::{MailEntry, MailKey};
use crate::termview::{CELL_H, CELL_W};
use crate::ui::demo_page as dp;
use crate::ui::dual_pool::PoolRect;
use crate::ui::grid_text::grid_wrap;

// ---- 几何常量（网格制；涂装按实例格同尺换算）----

/// 条目卡内上下留白 1 格
pub const ITEM_PAD_V: u32 = CELL_H;
/// 条目卡内左右内缩 1 格
pub const ITEM_PAD_H: u32 = CELL_W;
/// 栏间距 0.5 格
pub const ROW_GAP: u32 = CELL_H / 2;
/// 条目卡间距 1 格
pub const ITEM_GAP: u32 = CELL_H;
/// 正文体行高 54px = line_h_grid(CELL_H, 1.0)（f32 ceil/round 非 const，
/// 值钉死 + 考题运行期与 line_h_grid 对表同源）
pub const BODY_LH: u32 = 54;
/// H1 行高 90px = line_h_grid(CELL_H, dp::H1_SCALE=1.7)（对表同上）
pub const H1_LH: u32 = 90;
/// H1 半包框块高 = 上垫 + 行带 + 下垫（md_layout H1 单行为期同式）
pub const H1_BLOCK_H: u32 = dp::HU + H1_LH + dp::HU;
/// 标题栏引用缩进（格）：左竖线 + 1 格缩进（md 引用同尺）
pub const TITLE_INDENT_CELLS: u32 = 2;

/// 摘要占位（懒加载两态词面，沿用 mail_list 旧约）
pub const SUMMARY_PENDING: &str = "（摘要待取）";
pub const SUMMARY_EMPTY: &str = "（无摘要）";

// ---- 信头纯件（剥部门/拆收件人/时间去时区）----

/// 剥部门留名（用户拍板：不要部门，就二字名）：串内含「部」且剥完
/// 还剩 ≥2 字 → 剥到第一个「部」（含）为止；否则原样（「全体」无部
/// 字天然原样）。
pub fn strip_dept(name: &str) -> String {
    if let Some(pos) = name.find('部') {
        let rest = &name[pos + '部'.len_utf8()..];
        if rest.chars().count() >= 2 {
            return rest.to_string();
        }
    }
    name.to_string()
}

/// 拆收件人：「、」为主分隔（兼容 ，,；;），逐段 trim 去空，逐段剥部门。
/// 两个收件人上下并置 = 本 Vec 逐行画；空串 = 一空 Vec（涂装不画箭头列）。
pub fn split_recipients(to: &str) -> Vec<String> {
    to.split(['、', '，', ',', '；', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(strip_dept)
        .collect()
}

/// 时间去时区（用户拍板：不要时区）：「2026-09-30 08:00 +08:00」→
/// 「2026-09-30 08:00」——取前两段（日期 + 时分）；本就无第三段原样。
pub fn fmt_time(time: &str) -> String {
    let mut it = time.split(' ');
    match (it.next(), it.next()) {
        (Some(d), Some(t)) if !d.is_empty() && !t.is_empty() => format!("{d} {t}"),
        _ => time.to_string(),
    }
}

// ---- 条目排版（可变高：卡高 = 三栏实量之和，不设最大高度）----

/// 一信的排版账（涂装直读；折行区间 = char 下标，与 grid_wrap 同约）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLay {
    /// 卡高（px）
    pub h: u32,
    /// 内容流内卡顶（px，前缀和）
    pub top: i64,
    /// 第一栏（卡内相对 y / 高）：高 = max(H1 块, 收件人行数 × BODY_LH)
    pub meta_y: u32,
    pub meta_h: u32,
    /// 收件人（已拆已剥部门，涂装逐行画）
    pub recipients: Vec<String>,
    /// 第二栏标题折行区间（≥1 行；空标题 = 单行空）
    pub title_lines: Vec<(usize, usize)>,
    pub title_y: u32,
    /// 第三栏摘要折行区间（≥1 行）
    pub sum_lines: Vec<(usize, usize)>,
    pub sum_y: u32,
}

/// 条目文宽（格）：卡宽 − 左右内缩，按 CELL_W 折算
pub fn text_cells_of(item_w: u32) -> u32 {
    item_w.saturating_sub(ITEM_PAD_H * 2) / CELL_W
}

/// 显示标题（缺字头标题回落信名——涂装与排版同一把尺，单源）
pub fn display_title(e: &MailEntry) -> &str {
    if e.title.is_empty() {
        &e.name
    } else {
        &e.title
    }
}

/// 排一信（纯函数）：text_cells = 卡内文宽（格）。折行宽保底 4 格防
/// 零宽死循；行数下限各栏 1 行。
pub fn lay_item(e: &MailEntry, top: i64, text_cells: u32) -> ItemLay {
    let recipients = split_recipients(&e.to);
    let n_rcp = recipients.len().max(1) as u32;
    let meta_h = H1_BLOCK_H.max(n_rcp * BODY_LH);
    let title_cells = text_cells.saturating_sub(TITLE_INDENT_CELLS).max(4);
    let body_cells = text_cells.max(4);
    let mut title_lines = grid_wrap(display_title(e), title_cells);
    if title_lines.is_empty() {
        title_lines.push((0, 0));
    }
    let sum_text: &str = match &e.summary {
        Some(s) if !s.is_empty() => s,
        Some(_) => SUMMARY_EMPTY,
        None => SUMMARY_PENDING,
    };
    let mut sum_lines = grid_wrap(sum_text, body_cells);
    if sum_lines.is_empty() {
        sum_lines.push((0, 0));
    }
    let meta_y = ITEM_PAD_V;
    let title_y = meta_y + meta_h + ROW_GAP;
    let sum_y = title_y + title_lines.len() as u32 * BODY_LH + ROW_GAP;
    let h = sum_y + sum_lines.len() as u32 * BODY_LH + ITEM_PAD_V;
    ItemLay {
        h,
        top,
        meta_y,
        meta_h,
        recipients,
        title_lines,
        title_y,
        sum_lines,
        sum_y,
    }
}

/// 整册排版（前缀和流水）：oldest_first 序，最新在底
pub fn lay_items(entries: &[MailEntry], text_cells: u32) -> Vec<ItemLay> {
    let mut top = 0i64;
    entries
        .iter()
        .map(|e| {
            let lay = lay_item(e, top, text_cells);
            top += i64::from(lay.h) + i64::from(ITEM_GAP);
            lay
        })
        .collect()
}

/// 内容全高（n 卡 + (n-1) 距；0 = 0）
pub fn total_h(lays: &[ItemLay]) -> i64 {
    match lays.last() {
        None => 0,
        Some(l) => l.top + i64::from(l.h),
    }
}

/// 滚动上限（px）
pub fn scroll_max(lays: &[ItemLay], viewport_h: i64) -> i64 {
    (total_h(lays) - viewport_h).max(0)
}

/// 条目卡矩形（视口坐标 → 屏坐标；offset_bottom = 视口底距内容底 px，
/// 0 = 贴底 = 最新信贴视口底）——眼手同尺唯一源
pub fn item_rect(
    area: &PoolRect,
    vp: (i64, i64),
    lays: &[ItemLay],
    i: usize,
    offset_bottom: i64,
) -> PoolRect {
    let vp_h = vp.1 - vp.0;
    let scroll_top = (total_h(lays) - vp_h - offset_bottom).max(0);
    PoolRect {
        x: area.x,
        y: vp.0 + lays[i].top - scroll_top,
        w: area.w,
        h: lays[i].h,
    }
}

/// 可见条目区间（半卡在沿上的也画；裁剪归涂装断墨带）
pub fn visible_range(
    lays: &[ItemLay],
    viewport_h: i64,
    offset_bottom: i64,
) -> std::ops::Range<usize> {
    if lays.is_empty() || viewport_h <= 0 {
        return 0..0;
    }
    let scroll_top = (total_h(lays) - viewport_h - offset_bottom).max(0);
    let scroll_bot = scroll_top + viewport_h;
    let first = lays.partition_point(|l| l.top + i64::from(l.h) <= scroll_top);
    let last = lays.partition_point(|l| l.top < scroll_bot);
    first..last.max(first)
}

/// 摘要懒加载窗：可见窗上沿再向上扩 SUMMARY_LOOKBACK 屏，下沿 = 视口底
pub const SUMMARY_LOOKBACK: i64 = 2;
pub fn summary_window(
    lays: &[ItemLay],
    viewport_h: i64,
    offset_bottom: i64,
) -> std::ops::Range<usize> {
    if lays.is_empty() || viewport_h <= 0 {
        return 0..0;
    }
    let scroll_top = (total_h(lays) - viewport_h - offset_bottom).max(0);
    let lo_y = (scroll_top - SUMMARY_LOOKBACK * viewport_h).max(0);
    let first = lays.partition_point(|l| l.top + i64::from(l.h) <= lo_y);
    let last = lays.partition_point(|l| l.top < scroll_top + viewport_h);
    first..last.max(first)
}

/// 命中：视口带内点中条目卡 = Item(i)；页内其余 = Page（吞掉防穿透）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailPageHit {
    Item(usize),
    Page,
}

pub fn hit(
    area: &PoolRect,
    vp: (i64, i64),
    lays: &[ItemLay],
    offset_bottom: i64,
    x: i64,
    y: i64,
) -> Option<MailPageHit> {
    let inside =
        |r: &PoolRect| x >= r.x && x < r.x + i64::from(r.w) && y >= r.y && y < r.y + i64::from(r.h);
    if y >= vp.0 && y < vp.1 {
        for i in visible_range(lays, vp.1 - vp.0, offset_bottom) {
            if inside(&item_rect(area, vp, lays, i, offset_bottom)) {
                return Some(MailPageHit::Item(i));
            }
        }
    }
    Some(MailPageHit::Page)
}

// ---- 页级几何（整页卡内芯；reader_geom 同尺配方）----

/// 顶栏高（物理 px，与阅读页 TOP_BAR_H 同尺配方）
pub const TOP_BAR_H: i64 = 110;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailGeom {
    pub x0: i64,
    pub x1: i64,
    pub bar_y0: i64,
    pub bar_y1: i64,
    /// 分隔线 y（顶栏底）
    pub div_y: i64,
    /// 条目视口纵段
    pub view_y0: i64,
    pub view_y1: i64,
}

pub fn mail_geom(w: u32, h: u32, bottom_inset: u32) -> MailGeom {
    let (ox, oy) = crate::ui::tab_bar::content_origin();
    let x0 = i64::from(ox);
    let x1 = i64::from(w)
        - i64::from(
            crate::termview::AI_PAGE_FRAME_MARGIN + crate::termview::AI_PAGE_FRAME_W + CELL_W,
        );
    let bar_y0 = i64::from(oy);
    let bar_y1 = bar_y0 + TOP_BAR_H;
    let view_y0 = bar_y1 + 1;
    let view_y1 = crate::ui::parser_page::visible_bottom(h, bottom_inset).max(view_y0);
    MailGeom {
        x0,
        x1,
        bar_y0,
        bar_y1,
        div_y: bar_y1,
        view_y0,
        view_y1,
    }
}

/// 条目列矩形（item_rect 的 area 入参唯一源）
pub fn items_area(g: &MailGeom) -> PoolRect {
    PoolRect {
        x: g.x0,
        y: 0,
        w: (g.x1 - g.x0).max(0) as u32,
        h: 0,
    }
}

// ---- 状态核（追底 = follow_tail 同款语义像素账版）----

#[derive(Debug, Clone)]
pub struct MailPageView {
    pub key: MailKey,
    offset_bottom: i64,
    /// 追底态：true = 恒贴底；上滑取消、回底恢复
    follow: bool,
    /// 排版缓存（壳每个烘焙轮喂；滚动态直接读）
    lays: Vec<ItemLay>,
    viewport_h: i64,
    epoch: u64,
}

impl MailPageView {
    pub fn new(key: MailKey) -> Self {
        MailPageView {
            key,
            offset_bottom: 0,
            follow: true,
            lays: Vec::new(),
            viewport_h: 0,
            epoch: 0,
        }
    }

    pub fn offset_bottom(&self) -> i64 {
        self.offset_bottom
    }
    pub fn follow(&self) -> bool {
        self.follow
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn lays(&self) -> &[ItemLay] {
        &self.lays
    }

    /// 排版+布局写回（壳烘焙轮喂）：追底态恒贴底；内容缩水钳回上限
    pub fn sync_items(&mut self, entries: &[MailEntry], text_cells: u32, viewport_h: i64) {
        self.lays = lay_items(entries, text_cells);
        self.viewport_h = viewport_h;
        let max = scroll_max(&self.lays, viewport_h);
        if self.follow {
            self.offset_bottom = 0;
        } else if self.offset_bottom > max {
            self.offset_bottom = max;
        }
    }

    /// 滚动（壳手势喂增量）：dy > 0 = 看更旧；回底 = 恢复追底。变了才
    /// bump（sig 鬼影纪律）
    pub fn scroll_by(&mut self, dy: i64) -> bool {
        let max = scroll_max(&self.lays, self.viewport_h);
        let ns = (self.offset_bottom + dy).clamp(0, max);
        if ns == self.offset_bottom {
            return false;
        }
        self.offset_bottom = ns;
        self.follow = ns == 0;
        self.epoch += 1;
        true
    }

    pub fn geo(&self) -> (i64, i64) {
        (self.viewport_h, self.offset_bottom)
    }
}

// ---- 全局句柄（reader_handle 同款：壳开收，涂装/命中取）----

static VIEW: std::sync::Mutex<Option<MailPageView>> = std::sync::Mutex::new(None);
static DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bump_dirty() {
    DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 开信箱页（召唤 Panel::Mail 时）：同册重开 = 归零追底
pub fn open(key: MailKey) {
    *VIEW.lock().unwrap() = Some(MailPageView::new(key));
    bump_dirty();
}

/// 收信箱页（离栈清理）
pub fn close() {
    if VIEW.lock().unwrap().take().is_some() {
        bump_dirty();
    }
}

pub fn open_key() -> Option<MailKey> {
    VIEW.lock().unwrap().as_ref().map(|v| v.key)
}

/// 页存活闸（BAR-220）：close 后 VIEW=None。退场滑出期涂装仍可见
/// （mail_draw=true）但内容不可能再变——烘焙槽以此闸空页重烘
pub fn is_open() -> bool {
    VIEW.lock().unwrap().is_some()
}

pub fn snap() -> Option<MailPageView> {
    VIEW.lock().unwrap().clone()
}

pub fn take_dirty() -> bool {
    DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed)
}

pub fn sync_items(entries: &[MailEntry], text_cells: u32, viewport_h: i64) {
    if let Some(v) = VIEW.lock().unwrap().as_mut() {
        v.sync_items(entries, text_cells, viewport_h);
    }
}

pub fn scroll_by(dy: i64) -> bool {
    let mut g = VIEW.lock().unwrap();
    match g.as_mut() {
        Some(v) => {
            let r = v.scroll_by(dy);
            if r {
                bump_dirty();
            }
            r
        }
        None => false,
    }
}
