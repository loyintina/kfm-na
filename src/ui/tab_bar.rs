//! tab_bar.rs — 配置卡标签栏（主题宪法 §四 首行布局区，核心层纯逻辑，A 档钉）。
//!
//! 条款兑现：无标题栏——整张卡都是内容区，标签行在首行自开布局区；
//! 标签行 = 2 格高（§七）；选中态 = 功能光标开口框（宪法 §三/§四
//! 三修：形态/涂装归 cursor.rs 与 termview，本册只管几何——弹簧移动、
//! 落点咬格、**线长随机机制**：选中切换重掷顶/底线长，同标重按不重掷）；
//! 横滑区（内容超出可横滚，pan clamp）；**手势仲裁边界单源**——
//! `in_row`/`hit` 是壳层「标签行上的横向滑动不触发面板拖拽/页面滑向」
//! 的判定尺（眼手同尺：涂装侧 tab_rects 与命中判定同一份几何）。
//!
//! 光标框移动 = fx_spring 欠阻尼弹簧（键盘 inset 同核）：select 瞬间从
//! 当前位置重定基续弹（来回狂点不跳变），600ms 兜底贴死。
//!
//! 时间戳：壳层喂 report::boot_ms 同钟毫秒；本册零墙钟（考题喂假钟）。
//!
//! 标定值（theme.md §七 登记表回填项）：内容原点 (43,55) = 环内缘 +
//! 1 格 padding；标签宽 = 文字格 + 2 padding 格；标签间距 1 格。

use crate::termview::{AI_PAGE_FRAME_MARGIN, AI_PAGE_FRAME_W, CELL_H, CELL_W};

/// 标签行高 = 2 格（§七 相对比例条款；2026-09-12 真机实测拍板：
/// 1 格太扁——24px 字贴边，2 格留白才像可点目标；咬格不破，不用 2.5）
pub const TAB_ROW_H: u32 = CELL_H * 2;
/// 标签文字两侧 padding 各 1 格
pub const TAB_PAD_X: u32 = CELL_W;
/// 标签间距 1 格
pub const TAB_GAP: u32 = CELL_W;

/// 文字格宽（CJK 宽字 2 格，其余 1 格）：0x2E80 起是 CJK 部首/假名/汉字
/// 全家——池名场景够用；终端网格的宽字判定是 alacritty 内账，这里只是
/// 标签排版尺（眼手同尺范围 = 标签栏内部自洽）
pub fn text_cells(s: &str) -> u32 {
    s.chars()
        .map(|c| if (c as u32) >= 0x2E80 { 2 } else { 1 })
        .sum()
}

/// 内容区原点（咬格）：x = 环左内缘（MARGIN + 3 倍粗左缘）+ 1 格，
/// y = 环上内缘（MARGIN + 细缘）+ 1 格。整卡内容布局的共同原点——
/// 标签行之后的首行布局区/双池都从这里起排（§四 无标题栏）
pub fn content_origin() -> (u32, u32) {
    (
        AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W * 3 + CELL_W,
        AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_H,
    )
}

/// 标签行原点 x（宪法 §四 四修 左缘对齐条款）：= 双池左框左缘
/// （环粗左缘 + 2 格内边距，与 dual_pool pool_area 的 x 同源同值）——
/// 光标左粗线与上下双池左框逐像素一线。内容带/命中带仍用
/// content_origin（43），标签行整体右让 1 格是对齐不是缩带
pub fn tab_row_origin_x() -> u32 {
    AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W * 3 + CELL_W * 2
}

/// 标签行带命中（仲裁边界）：y ∈ [oy, oy+1格)。x 不设限——行带是横滑区，
/// 整个行高带上的手势都归标签栏（滑动不穿透给面板拖拽）
pub fn in_row(y: f64) -> bool {
    let oy = content_origin().1 as f64;
    y >= oy && y < oy + f64::from(TAB_ROW_H)
}

/// 单个标签的矩形（x 可负——横滚滚出左缘；涂装/命中共用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRect {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// 标签栏状态：池名表 + 选中 + 横滚 + 光标弹簧 + 开口光标几何。
/// 壳层持有一份（配置卡常驻，不随召唤重置——召唤即随机的只是 accent，
/// 标签是内容不是装修）
pub struct TabBar {
    tabs: Vec<String>,
    selected: usize,
    /// 横滚偏移（<=0，负 = 内容左移看右侧）
    scroll_px: i64,
    /// 可视视口宽（内容区宽；pan clamp / select 可见性的尺子）
    viewport_w: u32,
    /// 光标弹簧：select 瞬间 from = 当时位置（重定基）
    cursor_from: f32,
    cursor_start_ms: u64,
    /// 开口光标随机源（宪法 §四 线长随机机制；种子壳层注时间戳）
    rng: crate::ui::accent::AccentRng,
    /// 当前一付线长（选中切换时重掷；同标重按不重掷——没移动不换装）
    geom: crate::ui::cursor::OpenCursorGeom,
}

impl TabBar {
    /// 定种子构造（考题/兜底用——开口光标线长每 boot 同一付，确定性复现）
    pub fn new(tabs: &[&str], viewport_w: u32) -> Self {
        Self::new_seeded(tabs, viewport_w, 0x5EED_5EED_5EED_5EED)
    }

    /// 注种子构造（生产装配用——壳层喂时间戳，线长每 boot 重新随机，
    /// 宪法 §四 线长随机机制）
    pub fn new_seeded(tabs: &[&str], viewport_w: u32, seed: u64) -> Self {
        let mut rng = crate::ui::accent::AccentRng::new(seed);
        let geom = tabs
            .first()
            .map(|t| crate::ui::cursor::roll((text_cells(t) + 2) * CELL_W, &mut rng))
            .unwrap_or(crate::ui::cursor::OpenCursorGeom { top_w: 0, bot_w: 0 });
        TabBar {
            tabs: tabs.iter().map(|s| s.to_string()).collect(),
            selected: 0,
            scroll_px: 0,
            viewport_w,
            cursor_from: tab_row_origin_x() as f32,
            cursor_start_ms: 0,
            rng,
            geom,
        }
    }

    pub fn set_viewport_w(&mut self, w: u32) {
        self.viewport_w = w;
        self.scroll_px = self.scroll_px.clamp(self.min_scroll(), 0);
    }

    pub fn tabs(&self) -> &[String] {
        &self.tabs
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll_px(&self) -> i64 {
        self.scroll_px
    }

    /// 全部标签的矩形（scroll 已平移；涂装与命中同这份——眼手同尺）
    pub fn tab_rects(&self) -> Vec<TabRect> {
        rects_of(&self.tabs, self.scroll_px)
    }

    /// 未加 scroll 的基准 x（select 可见性/光标目标的计算尺）；
    /// 起点 = 标签行原点 61（四修 左缘对齐，§四）
    fn base_x(&self, i: usize) -> i64 {
        let ox = tab_row_origin_x() as i64;
        self.tabs[..i].iter().fold(ox, |x, t| {
            x + ((text_cells(t) + 2) * CELL_W) as i64 + TAB_GAP as i64
        })
    }

    /// 内容总宽（标签 + 间距；横滚溢出判定用）
    pub fn content_w(&self) -> u32 {
        let n = self.tabs.len() as u32;
        if n == 0 {
            return 0;
        }
        self.tabs
            .iter()
            .map(|t| (text_cells(t) + 2) * CELL_W)
            .sum::<u32>()
            + (n - 1) * TAB_GAP
    }

    fn min_scroll(&self) -> i64 {
        // 内容右缘（标签行原点 + 总宽）对齐视口右缘为下限——漏算原点
        // 会让末标签尾巴永远停在视口外（钉④⑤实踩）；四修：原点从
        // 内容带 43 换标签行 61（左缘对齐条款），公式同构换尺
        (self.viewport_w as i64 - tab_row_origin_x() as i64 - self.content_w() as i64).min(0)
    }

    /// 命中标签（y 先过行带闸，x 查 scroll 平移后的矩形）
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        if !in_row(y) {
            return None;
        }
        self.tab_rects()
            .iter()
            .position(|r| x >= r.x as f64 && x < (r.x + r.w as i64) as f64)
    }

    /// 横滚（手指 dx 直接喂；clamp 到 [-（溢出量), 0]，无溢出空操作）
    pub fn pan(&mut self, dx: f64) {
        let old = self.scroll_px;
        self.scroll_px = (self.scroll_px + dx as i64).clamp(self.min_scroll(), 0);
        // 光标弹簧起点跟着内容走——滚动时光标贴标签不追赶
        self.cursor_from += (self.scroll_px - old) as f32;
    }

    /// 选中（点按抬手调用）：弹簧从当前位置重定基 + 滚动保证完整可见
    /// + 真换标时重掷开口光标线长（宪法 §四 随机机制：移动才换装）
    pub fn select(&mut self, i: usize, now_ms: u64) {
        if i >= self.tabs.len() {
            return;
        }
        self.cursor_from = self.cursor_x(now_ms);
        self.cursor_start_ms = now_ms;
        if i != self.selected {
            self.geom =
                crate::ui::cursor::roll((text_cells(&self.tabs[i]) + 2) * CELL_W, &mut self.rng);
        }
        self.selected = i;
        // 可见性：把选中标签完整拉进视口（左出界右拉、右出界左拉）。
        // scroll 突变不进弹簧——from 补 scroll 差（与 pan 同规：
        // 光标贴内容不追赶）；scroll 没变时 from 不动 = 纯切换续弹
        let old_scroll = self.scroll_px;
        let bx = self.base_x(i);
        let w = ((text_cells(&self.tabs[i]) + 2) * CELL_W) as i64;
        let vis_x = bx + self.scroll_px;
        if w >= self.viewport_w as i64 {
            self.scroll_px = -bx; // 标签比视口还宽：左缘对齐，右缘滚动看
        } else if vis_x < 0 {
            self.scroll_px = -bx;
        } else if vis_x + w > self.viewport_w as i64 {
            self.scroll_px = self.viewport_w as i64 - bx - w;
        }
        self.scroll_px = self.scroll_px.clamp(self.min_scroll(), 0);
        self.cursor_from += (self.scroll_px - old_scroll) as f32;
    }

    /// 光标框目标 x（弹簧终点 = 选中标签的当前视口 x；落点咬格——
    /// base_x 全是格整数倍，scroll 由 pan/select 喂整数）
    pub fn cursor_target(&self) -> f32 {
        (self.base_x(self.selected) + self.scroll_px) as f32
    }

    /// 光标框当前 x（欠阻尼弹簧采样；收敛/超时 = target 即终态）
    pub fn cursor_x(&self, now_ms: u64) -> f32 {
        crate::ui::fx_spring::spring_pos(
            self.cursor_from,
            self.cursor_target(),
            now_ms.saturating_sub(self.cursor_start_ms),
        )
    }

    /// 涂装快照（壳层逐帧/值守倒帧取数；tabs 克隆——涂装无权碰状态）
    pub fn snap(&self, now_ms: u64) -> TabBarSnap {
        TabBarSnap {
            tabs: self.tabs.clone(),
            selected: self.selected,
            scroll_px: self.scroll_px,
            cursor_x: self.cursor_x(now_ms),
            cursor_top_w: self.geom.top_w,
            cursor_bot_w: self.geom.bot_w,
        }
    }
}

/// 涂装快照（眼手同尺：rects_of 与 TabBar::tab_rects 同一份几何）
pub struct TabBarSnap {
    pub tabs: Vec<String>,
    pub selected: usize,
    pub scroll_px: i64,
    pub cursor_x: f32,
    /// 开口光标顶/底线长（宪法 §四 随机机制产物，涂装照抄不许自算）
    pub cursor_top_w: i64,
    pub cursor_bot_w: i64,
}

/// 标签矩形序列（自由函数版：涂装侧从快照算，状态侧从 self 算——
/// 同一实体，眼手同尺）。x 起点 = 标签行原点 61（四修 左缘对齐）；
/// y 仍取内容原点行（55）
pub fn rects_of(tabs: &[String], scroll_px: i64) -> Vec<TabRect> {
    let oy = content_origin().1;
    let mut x = tab_row_origin_x() as i64 + scroll_px;
    tabs.iter()
        .map(|t| {
            let w = (text_cells(t) + 2) * CELL_W;
            let r = TabRect {
                x,
                y: oy as i64,
                w,
                h: TAB_ROW_H,
            };
            x += w as i64 + TAB_GAP as i64;
            r
        })
        .collect()
}

/// 内容视口宽（壳层 resize 时喂 set_viewport_w 的尺子）：
/// 右内缘（屏宽 - MARGIN - 细缘）再让 1 格 padding，减原点 x
pub fn content_viewport_w(screen_w: u32) -> u32 {
    screen_w
        .saturating_sub(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_W)
        .saturating_sub(content_origin().0)
}

// ---- 共享句柄（D9 同源：gate 值守倒帧与前台帧同一份标签栏读数）----

use std::sync::{Arc, Mutex, RwLock};

pub type SharedTabBar = Arc<Mutex<TabBar>>;

static TAB_BAR_HANDLE: RwLock<Option<SharedTabBar>> = RwLock::new(None);

/// 注册（android_app 装配时调一次）；重注册 = 覆盖（热更核新实例）
pub fn register_tab_bar(bar: SharedTabBar) {
    *TAB_BAR_HANDLE.write().unwrap() = Some(bar);
}

/// 读句柄（gate 值守倒帧取快照用；未注册 = None 兜底不画标签栏）
pub fn tab_bar_handle() -> Option<SharedTabBar> {
    TAB_BAR_HANDLE.read().unwrap().clone()
}
