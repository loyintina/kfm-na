//! mail_list.rs — 信件列表页级卡状态核 + 几何（BAR-212，2026-09-30
//! 用户立项）：点信箱入口卡弹出的页面级卡片，列出册内每封信的条目卡。
//!
//! 形制（用户原话逐条兑现）：
//! - **页面级卡片**：几何 = modal 跳框同族安全带（顶 4 格 + 底 = 输入栏
//!   带 + 2 格——「不要把底栏的 ai 输入栏给盖住，跟其他页面级卡片一样
//!   的逻辑」）；压暗层归 ChromeSlot::ModalVeil（BAR-163 翻案件复用），
//!   详情查看器弹起时压在列表卡之上（同槽内叠序：dim → 列表卡 →
//!   viewer/modal）。
//! - **条目 = 三级框卡片**（comp_registry 三级框行主形态，它是「每封信
//!   的解析显示器」）：首行 = 时间 + 发信人 → 收信人（中间右箭头连接）；
//!   次行 = 字头标题（大字档）；三行 = 摘要（懒加载件，未取到显占位）。
//! - **滚动 + 追底**：信多 → 视口内滚动（惯性甩尾归壳接线 scroll.rs
//!   Fling 件）；**最新的信在最下方、视口默认追底**——新信到了自动
//!   贴底上滚（ai_page follow_tail 状态机同款语义，像素账版）。
//! - **摘要懒加载**：visible_range/summary_window 纯函数给窗，壳喂
//!   mail_feed::ensure_summaries——视口 + 上方两屏先行，滚上去再补。
//!
//! 本册 = 状态核 + 几何/命中/窗口裁决（A 档纯逻辑，tests/mail_list_spec
//! 钉死）；涂装在 termview（眼手同尺同一份 layout）；数据 = mail_feed。

use crate::termview::{CELL_H, CELL_W};
use crate::ui::dual_pool::PoolRect;
use crate::ui::modal;

// ---- 几何常量（网格制，modal 跳框同族尺）----

/// 条目卡内上下留白 1 格
pub const ITEM_PAD_V: u32 = CELL_H;
/// 条目卡内左右内缩 1 格（卡内再嵌三级框，比页卡内边距窄一档）
pub const ITEM_PAD_H: u32 = CELL_W;
/// 元信息行高（时间 + 从 → 致）1 格
pub const ITEM_META_H: u32 = CELL_H;
/// 标题行高 2 格（大字档）
pub const ITEM_TITLE_H: u32 = CELL_H * 2;
/// 摘要行高 1 格（宽内截断，不折行——全文归详情查看器）
pub const ITEM_SUMMARY_H: u32 = CELL_H;
/// 条目内行距 0.5 格（半格网 §一）
pub const ITEM_ROW_GAP: u32 = CELL_H / 2;
/// 条目卡高（恒定 7 格——卡高不吃内容：摘要缺位 = 占位行不等高跳变）
pub const ITEM_H: u32 =
    ITEM_PAD_V * 2 + ITEM_META_H + ITEM_ROW_GAP + ITEM_TITLE_H + ITEM_ROW_GAP + ITEM_SUMMARY_H;
/// 条目卡间距 1 格
pub const ITEM_GAP: u32 = CELL_H;
/// 条目 stride（排布/滚动/窗口裁决唯一尺）
pub const ITEM_STRIDE: u32 = ITEM_H + ITEM_GAP;

/// 摘要占位（懒加载未取到/取到为空两态共用词面——「固定前」的过渡态）
pub const SUMMARY_PENDING: &str = "（摘要待取）";
/// 摘要空态（服务端取到但信无摘要块/摘要为空——与待取分词，防用户
/// 误以为还在等网）
pub const SUMMARY_EMPTY: &str = "（无摘要）";

/// 列表卡矩形（涂装/命中唯一源）：modal 安全带内顶格全开——列表是
/// 长内容场景，卡恒取封顶高（不随信件数伸缩，少量信也同一张卡）
pub fn card_rect(screen_w: u32, screen_h: u32) -> PoolRect {
    PoolRect {
        x: i64::from(modal::MODAL_SIDE_MARGIN),
        y: i64::from(modal::MODAL_MAX_MARGIN_TOP),
        w: screen_w.saturating_sub(modal::MODAL_SIDE_MARGIN * 2),
        h: screen_h.saturating_sub(modal::MODAL_MAX_MARGIN_TOP + modal::MODAL_MAX_MARGIN_BOTTOM),
    }
}

/// 标题带（卡内顶：册名 · N 封）
pub fn title_rect(card: &PoolRect) -> PoolRect {
    PoolRect {
        x: card.x + modal::MODAL_PAD_X,
        y: card.y + i64::from(modal::MODAL_PAD_Y),
        w: (card.w as i64 - modal::MODAL_PAD_X * 2).max(0) as u32,
        h: modal::MODAL_TITLE_H,
    }
}

/// 关闭钮（卡内底全宽 3 格，modal 同件）
pub fn close_btn_rect(card: &PoolRect) -> PoolRect {
    PoolRect {
        x: card.x + modal::MODAL_PAD_X,
        y: card.y + i64::from(card.h) - i64::from(modal::MODAL_PAD_Y + modal::MODAL_CLOSE_H),
        w: (card.w as i64 - modal::MODAL_PAD_X * 2).max(0) as u32,
        h: modal::MODAL_CLOSE_H,
    }
}

/// 列表视口纵段（涂装裁剪/滚动上限/窗口裁决同一份）：标题带 + 分隔线
/// 带（上 0.5 + 1px + 下 0.5，modal 同尺）之下 → 关闭钮上 0.5 格
pub fn viewport_of(card: &PoolRect) -> (i64, i64) {
    let top = title_rect(card).y
        + i64::from(modal::MODAL_TITLE_H)
        + i64::from(modal::MODAL_FIELD_GAP)
        + 1
        + i64::from(modal::MODAL_FIELD_GAP);
    let bottom = close_btn_rect(card).y - i64::from(modal::MODAL_FIELD_GAP);
    (top, bottom.max(top))
}

/// 内容全高（n 条目：n 卡 + (n-1) 距；0 条目 = 0）
pub fn total_h(n_items: usize) -> i64 {
    if n_items == 0 {
        0
    } else {
        ITEM_STRIDE as i64 * n_items as i64 - i64::from(ITEM_GAP)
    }
}

/// 滚动上限（px）：内容全高 − 视口高；装得下 = 0
pub fn scroll_max(n_items: usize, viewport_h: i64) -> i64 {
    (total_h(n_items) - viewport_h).max(0)
}

/// 条目卡矩形（视口坐标 → 屏坐标）：i 条目顶 = 视口底 − 距底偏移 −
/// 内容从尾数起的纵位。眼手同尺唯一源——offset_bottom = 视口底距内容
/// 底的像素账（0 = 贴底 = 最新信贴视口底）
pub fn item_rect(card: &PoolRect, n_items: usize, i: usize, offset_bottom: i64) -> PoolRect {
    let (vp_top, vp_bot) = viewport_of(card);
    let vp_h = vp_bot - vp_top;
    let scroll_top = (total_h(n_items) - vp_h - offset_bottom).max(0);
    PoolRect {
        x: card.x + modal::MODAL_PAD_X,
        y: vp_top + ITEM_STRIDE as i64 * i as i64 - scroll_top,
        w: (card.w as i64 - modal::MODAL_PAD_X * 2).max(0) as u32,
        h: ITEM_H,
    }
}

/// 可见条目区间（涂装只画视口内的；半卡在沿上的也画——裁剪归涂装
/// 断墨带 = viewport_of 同一份）
pub fn visible_range(
    n_items: usize,
    viewport_h: i64,
    offset_bottom: i64,
) -> std::ops::Range<usize> {
    if n_items == 0 || viewport_h <= 0 {
        return 0..0;
    }
    let scroll_top = (total_h(n_items) - viewport_h - offset_bottom).max(0);
    let first = (scroll_top / ITEM_STRIDE as i64).max(0) as usize;
    let last = ((scroll_top + viewport_h) / ITEM_STRIDE as i64 + 1).min(n_items as i64) as usize;
    first..last.max(first)
}

/// 摘要懒加载窗（BAR-212 用户拍板：视口 + 上方几屏先行，滚到再补）：
/// 可见窗上沿再向上扩 SUMMARY_LOOKBACK 屏（上方 = 更旧的信 = 追底态下
/// 「上面几屏」），下沿 = 视口底。返回条目下标区间
pub const SUMMARY_LOOKBACK: i64 = 2;
pub fn summary_window(
    n_items: usize,
    viewport_h: i64,
    offset_bottom: i64,
) -> std::ops::Range<usize> {
    if n_items == 0 || viewport_h <= 0 {
        return 0..0;
    }
    let scroll_top = (total_h(n_items) - viewport_h - offset_bottom).max(0);
    let lo = ((scroll_top - SUMMARY_LOOKBACK * viewport_h) / ITEM_STRIDE as i64).max(0) as usize;
    let hi = ((scroll_top + viewport_h) / ITEM_STRIDE as i64 + 1).min(n_items as i64) as usize;
    lo..hi.max(lo)
}

// ---- 状态核（追底 = ai_page follow_tail 同款语义的像素账版）----

/// 列表卡状态（一开一册；offset_bottom = 视口底距内容底的 px）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailListView {
    pub key: crate::mail_feed::MailKey,
    offset_bottom: i64,
    /// 追底态：true = 恒贴底（新信/摘要进来视口不动点）；上滑取消、
    /// 回底恢复（ai_page 同律）
    follow: bool,
    /// 布局缓存（壳涂装/手势每次按实时屏寸喂——状态核不揣屏寸）
    n_items: usize,
    viewport_h: i64,
    epoch: u64,
}

impl MailListView {
    pub fn new(key: crate::mail_feed::MailKey) -> Self {
        MailListView {
            key,
            offset_bottom: 0,
            follow: true,
            n_items: 0,
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

    /// 布局写回（壳涂装/命中每轮喂）：追底态恒贴底；条目缩水钳回上限
    pub fn sync_layout(&mut self, n_items: usize, viewport_h: i64) {
        self.n_items = n_items;
        self.viewport_h = viewport_h;
        let max = scroll_max(n_items, viewport_h);
        if self.follow {
            self.offset_bottom = 0;
        } else if self.offset_bottom > max {
            self.offset_bottom = max;
        }
    }

    /// 滚动（壳手势喂增量）：dy > 0 = 看更旧（视口离底远）；回底 =
    /// offset 归 0 = 恢复追底。变了才 bump（sig 鬼影纪律）
    pub fn scroll_by(&mut self, dy: i64) -> bool {
        let max = scroll_max(self.n_items, self.viewport_h);
        let ns = (self.offset_bottom + dy).clamp(0, max);
        // ns 不变 = follow 也不变（follow=false 时 offset>0 是不变式），
        // 直接早退——不变不 bump（sig 鬼影纪律）
        if ns == self.offset_bottom {
            return false;
        }
        self.offset_bottom = ns;
        self.follow = ns == 0;
        self.epoch += 1;
        true
    }

    /// 当前几何账（涂装/命中/懒加载窗同一份）
    pub fn geo(&self) -> (usize, i64, i64) {
        (self.n_items, self.viewport_h, self.offset_bottom)
    }
}

/// 命中分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailListHit {
    /// 关闭钮
    Close,
    /// 条目卡（下标，oldest_first 序）
    Item(usize),
    /// 卡内非命中区（吞掉，不许穿透压暗层下的解析页）
    Card,
    /// 点卡外 = 收起（跳框惯例）
    Outside,
}

/// 命中（x/y 屏坐标 i64，与涂装同一份几何）
pub fn hit(card: &PoolRect, n_items: usize, offset_bottom: i64, x: i64, y: i64) -> MailListHit {
    let inside =
        |r: &PoolRect| x >= r.x && x < r.x + i64::from(r.w) && y >= r.y && y < r.y + i64::from(r.h);
    if inside(&close_btn_rect(card)) {
        return MailListHit::Close;
    }
    let (vp_top, vp_bot) = viewport_of(card);
    if y >= vp_top && y < vp_bot {
        for i in 0..n_items {
            if inside(&item_rect(card, n_items, i, offset_bottom)) {
                return MailListHit::Item(i);
            }
        }
    }
    if inside(card) {
        return MailListHit::Card;
    }
    MailListHit::Outside
}

// ---- 全局句柄（cfg viewer 同款：壳开收，涂装/命中/闸门取）----

static VIEW: std::sync::Mutex<Option<MailListView>> = std::sync::Mutex::new(None);
static DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bump_dirty() {
    DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 开列表卡（点信箱入口行）：同册重开 = 归零追底（回底看最新）
pub fn open(key: crate::mail_feed::MailKey) {
    *VIEW.lock().unwrap() = Some(MailListView::new(key));
    bump_dirty();
}

/// 收列表卡（关闭钮/点卡外）
pub fn close() {
    if VIEW.lock().unwrap().take().is_some() {
        bump_dirty();
    }
}

/// 开着哪册（None = 未开）
pub fn open_key() -> Option<crate::mail_feed::MailKey> {
    VIEW.lock().unwrap().as_ref().map(|v| v.key)
}

/// 状态快照（涂装直读）
pub fn snap() -> Option<MailListView> {
    VIEW.lock().unwrap().clone()
}

/// 壳脏帧消耗口（mail_feed::take_dirty 同款）
pub fn take_dirty() -> bool {
    DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// 布局写回 + 滚动（壳接线口，锁内一把过）
pub fn sync_layout(n_items: usize, viewport_h: i64) {
    if let Some(v) = VIEW.lock().unwrap().as_mut() {
        let old = v.epoch;
        v.sync_layout(n_items, viewport_h);
        if v.epoch != old {
            bump_dirty();
        }
    }
}

/// 滚动（壳手势喂增量；返回 true = 变了要重烘）
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
