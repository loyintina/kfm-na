//! cfg_page.rs — 配置页三层目录状态核（主题宪法 §五「双池的目录语义」
//! 二版，2026-09-13 用户拍板；核心层纯逻辑零 IO，A 档钉）。
//!
//! 二版条款兑现（kfmv4 池卡实证 4 图对齐）：
//! - 根目录（大类）= 首行标签栏（tab_bar.rs 已有，本册不管）
//! - **子目录选择 = 下池**：行表由壳喂（系统管理大类目前仅一行
//!   「系统管理」）；行 = 3 格高圆角框，选中 = accent 渐变描边（涂装侧）
//! - **二级选项 = 上池**：字段框行表（标签列 + 值框），首行是
//!   **下拉行**（如「默认服务器」）——下拉服务上池内容自身的选项集，
//!   **不与下池联动**（推翻初版双向联动条款）
//! - 三级展开 = 全屏页（v1b，本册不及）
//!
//! 眼手同尺：涂装与触摸命中读本册同一份几何（lower_row_rect/
//! upper_row_rect/value_box_rect/trigger_rect/dropdown_panel_rect），
//! 壳层不许另算。
//!
//! 数据流：壳持有 settings 解析结果（servers.json/terminal.json），
//! 重建时喂 `set_rows`（下池行）+ `set_upper`（上池字段框行）+
//! `set_options`（下拉选项与选中）；本册管 focus/dropdown/代际。
//! 下拉点选只改 option_sel——换选的业务动作（写 terminal.json 等）
//! 归壳（核心零 IO），壳点选后读 `option_sel()` 执行。任何影响涂装
//! 的变更 bump epoch——配置槽 sig 吃 epoch 一维（漏维 = 陈旧像素
//! 鬼影，ui-base §八 纪律）。

use crate::termview::{CELL_H, CELL_W};
use crate::ui::dual_pool::PoolRect;

/// 下池池行高 = 3 格（二版 §五 池行条款：2 格实机太挤）
pub const LOWER_ROW_H: u32 = CELL_H * 3;
/// 上池字段框行高 = 2 格（二版 §七 标定）
pub const FIELD_ROW_H: u32 = CELL_H * 2;
/// 池内容距池框缘的内缩 = 1 格（布局层咬格）
pub const POOL_CONTENT_INSET: i64 = CELL_W as i64;
/// 行间留隙（三级框不贴边，kfmv4 实证样式）
pub const ROW_GAP: i64 = 10;
/// 字段框标签列宽 = 8 格（值框在其右，二版 §七 标定；6 格实机截断
/// 5 字标签——「默认服务器」5×20px+内缩 18 = 118 > 108）
pub const LABEL_COL_W: i64 = CELL_W as i64 * 8;

/// 下池行（子目录）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub title: String,
    pub meta: String,
}

/// 上池字段框行：标签列 + 值框；首行 is_dropdown = 下拉行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpperRow {
    pub label: String,
    pub value: String,
    pub is_dropdown: bool,
}

/// 涂装/判卷快照（D9：gate 值守倒帧与前台帧同一份读数）
#[derive(Debug, Clone)]
pub struct CfgPageSnap {
    pub rows: Vec<RowView>,
    pub focus: usize,
    pub upper: Vec<UpperRow>,
    pub options: Vec<String>,
    pub option_sel: usize,
    pub dropdown_open: bool,
    pub epoch: u64,
}

pub struct CfgPage {
    rows: Vec<RowView>,
    focus: usize,
    upper: Vec<UpperRow>,
    options: Vec<String>,
    option_sel: usize,
    dropdown_open: bool,
    epoch: u64,
}

impl CfgPage {
    pub fn new() -> Self {
        CfgPage {
            rows: Vec::new(),
            focus: 0,
            upper: Vec::new(),
            options: Vec::new(),
            option_sel: 0,
            dropdown_open: false,
            epoch: 0,
        }
    }

    /// 喂下池行表（壳重建：大类切换后）。focus 越界 clamp；
    /// 行表变了 bump 代际
    pub fn set_rows(&mut self, rows: Vec<RowView>) {
        if rows != self.rows {
            self.rows = rows;
            self.epoch += 1;
        }
        if self.focus >= self.rows.len() {
            self.focus = self.rows.len().saturating_sub(1);
            self.epoch += 1;
        }
    }

    /// 喂上池字段框行表（下拉换选/数据变更后由壳重建）
    pub fn set_upper(&mut self, upper: Vec<UpperRow>) {
        if upper != self.upper {
            self.upper = upper;
            self.epoch += 1;
        }
    }

    /// 喂下拉选项表与选中项（壳从 terminal.json defaultSession 解析）。
    /// sel 越界 clamp；变了才 bump
    pub fn set_options(&mut self, options: Vec<String>, sel: usize) {
        let sel = sel.min(options.len().saturating_sub(1));
        if options != self.options || sel != self.option_sel {
            self.options = options;
            self.option_sel = sel;
            self.epoch += 1;
        }
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn option_sel(&self) -> usize {
        self.option_sel
    }

    pub fn dropdown_open(&self) -> bool {
        self.dropdown_open
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 点选下池行（子目录切换 = 上池内容跟换，重建归壳）。
    /// 同标重点不重掷（代际不空涨）
    pub fn select(&mut self, i: usize) {
        if self.rows.is_empty() {
            return;
        }
        let i = i.min(self.rows.len() - 1);
        if i != self.focus {
            self.focus = i;
            self.epoch += 1;
        }
    }

    /// 上池下拉框开合
    pub fn toggle_dropdown(&mut self) {
        self.dropdown_open = !self.dropdown_open;
        self.epoch += 1;
    }

    /// 下拉框点选：换选 + 收 panel。业务动作（写配置/重建上池）归壳——
    /// 壳在本调用后读 `option_sel()` 执行
    pub fn dropdown_pick(&mut self, i: usize) {
        if self.options.is_empty() {
            return;
        }
        let i = i.min(self.options.len() - 1);
        if i != self.option_sel {
            self.option_sel = i;
            self.epoch += 1;
        }
        if self.dropdown_open {
            self.dropdown_open = false;
            self.epoch += 1;
        }
    }

    /// 下拉框开着时点别处 = 收（宪法 §六 下拉栏常规语义）
    pub fn dismiss_dropdown(&mut self) {
        if self.dropdown_open {
            self.dropdown_open = false;
            self.epoch += 1;
        }
    }

    // ---- 几何（眼手同尺唯一来源）----

    /// 下池命中：y 落第几行（出池/行间隙 = None）
    pub fn lower_row_at_y(&self, y: i64, lower: &PoolRect) -> Option<usize> {
        for i in 0..self.rows.len() {
            let r = lower_row_rect(i, lower);
            if y >= r.y && y < r.y + r.h as i64 {
                return Some(i);
            }
        }
        None
    }

    /// 上池下拉触发器矩形（首行字段框的值框位，§六 触发器条款）
    pub fn trigger_rect(&self, upper: &PoolRect) -> PoolRect {
        trigger_rect(upper)
    }

    /// 下拉 panel 矩形（顶部栏向下弹——宪法 §六：方向反了会弹出屏外）：
    /// 触发器下缘起，行数 = 选项数，最高不出配置页可视区（壳喂 max_h）
    pub fn dropdown_panel_rect(&self, upper: &PoolRect, max_h: u32) -> PoolRect {
        dropdown_panel_rect(self.options.len(), upper, max_h)
    }

    /// 下拉 panel 命中：y 落第几行（panel 外 = None）
    pub fn dropdown_item_at_y(&self, y: i64, upper: &PoolRect, max_h: u32) -> Option<usize> {
        let p = dropdown_panel_rect(self.options.len(), upper, max_h);
        if y < p.y || y >= p.y + p.h as i64 {
            return None;
        }
        let i = ((y - p.y) as u32 / FIELD_ROW_H) as usize;
        (i < self.options.len()).then_some(i)
    }

    /// 上池内容高（喂 dual_pool.set_upper_content_h）：字段框行 + 留隙
    pub fn upper_content_h(&self) -> u32 {
        let n = self.upper.len() as u32;
        if n == 0 {
            return 0;
        }
        POOL_CONTENT_INSET as u32 + n * FIELD_ROW_H + (n - 1) * ROW_GAP as u32
    }

    pub fn snap(&self) -> CfgPageSnap {
        CfgPageSnap {
            rows: self.rows.clone(),
            focus: self.focus,
            upper: self.upper.clone(),
            options: self.options.clone(),
            option_sel: self.option_sel,
            dropdown_open: self.dropdown_open,
            epoch: self.epoch,
        }
    }
}

impl Default for CfgPage {
    fn default() -> Self {
        Self::new()
    }
}

// ---- 几何自由函数（眼手同尺唯一来源：CfgPage 方法与涂装侧共用）----

/// 下池第 i 行的框矩形（池内缘内缩 1 格，逐行 3 格高 + 留隙）
pub fn lower_row_rect(i: usize, lower: &PoolRect) -> PoolRect {
    PoolRect {
        x: lower.x + POOL_CONTENT_INSET,
        y: lower.y + POOL_CONTENT_INSET + (i as i64) * (LOWER_ROW_H as i64 + ROW_GAP),
        w: lower.w.saturating_sub((POOL_CONTENT_INSET * 2) as u32),
        h: LOWER_ROW_H,
    }
}

/// 上池第 i 行字段框矩形（池内缘内缩 1 格，逐行 2 格高 + 留隙）
pub fn upper_row_rect(i: usize, upper: &PoolRect) -> PoolRect {
    PoolRect {
        x: upper.x + POOL_CONTENT_INSET,
        y: upper.y + POOL_CONTENT_INSET + (i as i64) * (FIELD_ROW_H as i64 + ROW_GAP),
        w: upper.w.saturating_sub((POOL_CONTENT_INSET * 2) as u32),
        h: FIELD_ROW_H,
    }
}

/// 字段框的值框位（标签列之右；下拉触发器/值文本都画这里）
pub fn value_box_rect(row: &PoolRect) -> PoolRect {
    PoolRect {
        x: row.x + LABEL_COL_W,
        y: row.y,
        w: row.w.saturating_sub(LABEL_COL_W as u32),
        h: row.h,
    }
}

/// 上池下拉触发器矩形 = 首行字段框的值框位
pub fn trigger_rect(upper: &PoolRect) -> PoolRect {
    value_box_rect(&upper_row_rect(0, upper))
}

/// 下拉 panel 矩形（顶部栏向下弹）：触发器下缘起，行数 = 选项数，
/// 最高不出配置页可视区（调用方喂 max_h）
pub fn dropdown_panel_rect(opt_count: usize, upper: &PoolRect, max_h: u32) -> PoolRect {
    let t = trigger_rect(upper);
    let want = (opt_count as u32) * FIELD_ROW_H;
    PoolRect {
        x: t.x,
        y: t.y + FIELD_ROW_H as i64,
        w: t.w,
        h: want.min(max_h),
    }
}

// ---- 共享句柄（D9 同源：gate 值守倒帧与前台帧同一份配置页读数）----

use std::sync::{Arc, Mutex, RwLock};

pub type SharedCfgPage = Arc<Mutex<CfgPage>>;

static CFG_PAGE_HANDLE: RwLock<Option<SharedCfgPage>> = RwLock::new(None);

/// 注册（android_app 装配时调一次）；重注册 = 覆盖（热更核新实例）
pub fn register_cfg_page(page: SharedCfgPage) {
    *CFG_PAGE_HANDLE.write().unwrap() = Some(page);
}

/// 读句柄（gate 值守倒帧取快照用；未注册 = None 兜底不画内容）
pub fn cfg_page_handle() -> Option<SharedCfgPage> {
    CFG_PAGE_HANDLE.read().unwrap().clone()
}
