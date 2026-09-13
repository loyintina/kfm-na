//! cfg_page.rs — 配置页三层目录状态核（主题宪法 §五「双池的目录语义」，
//! 2026-09-13 用户拍板；核心层纯逻辑零 IO，A 档钉）。
//!
//! 条款兑现：
//! - 根目录（大类）= 首行标签栏（tab_bar.rs 已有，本册不管）
//! - **子目录选择 = 下池**：行表 = [全局] + 服务器×N + [+ 新增]，
//!   聚焦行 = 选中子目录
//! - **二级选项 = 上池**：聚焦子目录的字段行 + 顶部**联动下拉框**——
//!   下池聚焦到哪，下拉框选中到哪；下拉框换选，下池聚焦同步（双向联动）
//! - 三级展开 = 全屏页（v1b，本册不及）
//!
//! 眼手同尺：涂装与触摸命中读本册同一份几何（row_rect/trigger_rect/
//! dropdown_panel_rect），壳层不许另算。
//!
//! 数据流：壳持有 settings 解析结果（servers.json/terminal.json），
//! 重建时喂 `set_rows`（下池行显示数据）+ `set_fields`（上池字段行）；
//! 本册管 focus/dropdown/代际。任何影响涂装的变更 bump epoch——
//! 配置槽 sig 吃 epoch 一维（漏维 = 陈旧像素鬼影，ui-base §八 纪律）。

use crate::termview::{CELL_H, CELL_W};
use crate::ui::dual_pool::PoolRect;

/// 池行高 = 2 格（§五 池行条款：标题行+元信息行 = 2 格）
pub const POOL_ROW_H: u32 = CELL_H * 2;
/// 池内容距池框缘的内缩 = 1 格（布局层咬格）
pub const POOL_CONTENT_INSET: i64 = CELL_W as i64;

/// 下池行（子目录）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub title: String,
    pub meta: String,
}

/// 涂装/判卷快照（D9：gate 值守倒帧与前台帧同一份读数）
#[derive(Debug, Clone)]
pub struct CfgPageSnap {
    pub rows: Vec<RowView>,
    pub focus: usize,
    pub fields: Vec<(String, String)>,
    pub dropdown_open: bool,
    pub epoch: u64,
}

pub struct CfgPage {
    rows: Vec<RowView>,
    focus: usize,
    fields: Vec<(String, String)>,
    dropdown_open: bool,
    epoch: u64,
}

impl CfgPage {
    pub fn new() -> Self {
        CfgPage {
            rows: Vec::new(),
            focus: 0,
            fields: Vec::new(),
            dropdown_open: false,
            epoch: 0,
        }
    }

    /// 喂下池行表（壳重建：配置读盘/热重载后）。focus 越界 clamp；
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

    /// 喂上池字段行（聚焦切换/数据变更后由壳重建）
    pub fn set_fields(&mut self, fields: Vec<(String, String)>) {
        if fields != self.fields {
            self.fields = fields;
            self.epoch += 1;
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn dropdown_open(&self) -> bool {
        self.dropdown_open
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 点选下池行（双向联动的下→上半向：聚焦切换 = 上池内容跟换，
    /// 字段行重建归壳）。同标重点不重掷（代际不空涨）
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

    /// 下拉框点选（双向联动的上→下半向：换选 = 聚焦同步 + 收 panel）
    pub fn dropdown_pick(&mut self, i: usize) {
        self.select(i);
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

    /// 下池第 i 行的内容矩形（池内缘内缩 1 格，逐行 2 格高）
    pub fn row_rect(&self, i: usize, lower: &PoolRect) -> PoolRect {
        row_rect(i, lower)
    }

    /// 下池命中：y 落第几行（出池/行间隙 = None）
    pub fn row_at_y(&self, y: i64, lower: &PoolRect) -> Option<usize> {
        for i in 0..self.rows.len() {
            let r = row_rect(i, lower);
            if y >= r.y && y < r.y + r.h as i64 {
                return Some(i);
            }
        }
        None
    }

    /// 上池下拉触发器矩形（上池首行 2 格，§六 触发器条款）
    pub fn trigger_rect(&self, upper: &PoolRect) -> PoolRect {
        trigger_rect(upper)
    }

    /// 上池字段行矩形（触发器之下逐行）
    pub fn field_rect(&self, i: usize, upper: &PoolRect) -> PoolRect {
        field_rect(i, upper)
    }

    /// 下拉 panel 矩形（顶部栏向下弹——宪法 §六：方向反了会弹出屏外）：
    /// 触发器下缘起，行数 = 下池行数，最高不出配置页可视区（壳喂 max_h）
    pub fn dropdown_panel_rect(&self, upper: &PoolRect, max_h: u32) -> PoolRect {
        dropdown_panel_rect(self.rows.len(), upper, max_h)
    }

    /// 下拉 panel 命中：y 落第几行（panel 外 = None）
    pub fn dropdown_item_at_y(&self, y: i64, upper: &PoolRect, max_h: u32) -> Option<usize> {
        let p = dropdown_panel_rect(self.rows.len(), upper, max_h);
        if y < p.y || y >= p.y + p.h as i64 {
            return None;
        }
        let i = ((y - p.y) as u32 / POOL_ROW_H) as usize;
        (i < self.rows.len()).then_some(i)
    }

    /// 上池内容高（喂 dual_pool.set_upper_content_h）：触发器 + 字段行
    pub fn upper_content_h(&self) -> u32 {
        POOL_CONTENT_INSET as u32 + (1 + self.fields.len() as u32) * POOL_ROW_H
    }

    pub fn snap(&self) -> CfgPageSnap {
        CfgPageSnap {
            rows: self.rows.clone(),
            focus: self.focus,
            fields: self.fields.clone(),
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

/// 下池第 i 行的内容矩形（池内缘内缩 1 格，逐行 2 格高）
pub fn row_rect(i: usize, lower: &PoolRect) -> PoolRect {
    PoolRect {
        x: lower.x + POOL_CONTENT_INSET,
        y: lower.y + POOL_CONTENT_INSET + (i as i64) * POOL_ROW_H as i64,
        w: lower.w.saturating_sub((POOL_CONTENT_INSET * 2) as u32),
        h: POOL_ROW_H,
    }
}

/// 上池下拉触发器矩形（上池首行 2 格，§六 触发器条款）
pub fn trigger_rect(upper: &PoolRect) -> PoolRect {
    PoolRect {
        x: upper.x + POOL_CONTENT_INSET,
        y: upper.y + POOL_CONTENT_INSET,
        w: upper.w.saturating_sub((POOL_CONTENT_INSET * 2) as u32),
        h: POOL_ROW_H,
    }
}

/// 上池字段行矩形（触发器之下逐行）
pub fn field_rect(i: usize, upper: &PoolRect) -> PoolRect {
    let t = trigger_rect(upper);
    PoolRect {
        y: t.y + POOL_ROW_H as i64 + (i as i64) * POOL_ROW_H as i64,
        ..t
    }
}

/// 下拉 panel 矩形（顶部栏向下弹）：触发器下缘起，行数 = row_count，
/// 最高不出配置页可视区（调用方喂 max_h）
pub fn dropdown_panel_rect(row_count: usize, upper: &PoolRect, max_h: u32) -> PoolRect {
    let t = trigger_rect(upper);
    let want = (row_count as u32) * POOL_ROW_H;
    PoolRect {
        x: t.x,
        y: t.y + POOL_ROW_H as i64,
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
