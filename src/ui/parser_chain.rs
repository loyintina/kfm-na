//! parser_chain.rs — 三区排布器（解析页两轴插件契约 §四 v2，2026-09-21
//! 用户拍板：tmux 竖排常驻右下 + 右上滚动区（连接·服务卡）+ 左滚动区
//! （环境卡）——排版服从操作频率，「右滑→点窗口」恒定两步，常驻卡永不
//! 被滚出屏）
//!
//! v1（竖链 + 单页滚动）收编的是「卡知排布」病灶；v2 区域化后契约不变：
//!
//! - 注册链 CHAIN = 唯一有序卡表 + 卡→区静态归属（新功能律：加卡 =
//!   加一行 + 自报高度，排布/滚动/裁剪零改动）；
//! - 卡对排布零感知：不 import 邻卡常量、不问自己排第几、不知道自己
//!   在哪个区；自报高度 card_h 契约不变；
//! - 三区几何（左区窗/右上区窗/常驻槽）+ 两本滚动账 + 各区裁剪带
//!   唯一源都在本册；
//! - 高度收集 heights() = 各卡自报高度的唯一询问点，别处不许手抄
//!   高度账。

use crate::termview::{CELL_H, CELL_W};
use crate::ui::dual_pool::{self, PoolRect};
use crate::ui::{link_card, sys_card};

/// 链上卡 id（注册链席位）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainCardId {
    /// tmux 会话卡（常驻右下——卡高账在 parser_page::tmux_card_h，动态）
    Tmux,
    /// 连接服务合并卡（右上滚动区）
    Link,
    /// 环境卡（左滚动区）
    Sys,
}

/// 区（卡→区 = 注册表静态声明，卡自身零感知）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// 右下常驻区（tmux 卡，钉视口底，不滚）
    Dock,
    /// 右上滚动区（常驻槽之上，右列剩余纵段）
    RightTop,
    /// 左滚动区（页全区减去右列与列距）
    Left,
}

/// 注册槽：region = 归属区；gap_before = 与**区内**上一卡的间距
/// （区内首槽恒 0）
pub struct ChainSlot {
    pub id: ChainCardId,
    pub region: Region,
    pub gap_before: u32,
}

/// 注册链（有序 + 区归属——槽位/滚动/裁剪全从本表出）
pub static CHAIN: &[ChainSlot] = &[
    ChainSlot {
        id: ChainCardId::Tmux,
        region: Region::Dock,
        gap_before: 0,
    },
    ChainSlot {
        id: ChainCardId::Link,
        region: Region::RightTop,
        gap_before: 0,
    },
    ChainSlot {
        id: ChainCardId::Sys,
        region: Region::Left,
        gap_before: 0,
    },
];

/// 右列固定网格宽（宪法 §四 v2：列数 = 实现参数，约束 = 刚好放下
/// 会话框「名 + ✕」加左右留白，不随屏宽变）：内宽 12 格（名 8 + × 4）
pub const RIGHT_COL_W: u32 = CELL_W * 16;
/// 左右区列间距（2 格，与池卡左右间隔同档）
pub const REGION_GAP: u32 = CELL_W * 2;
/// 常驻槽与右上滚动区的间距（1 格，链槽间距同档）
pub const DOCK_GAP: u32 = CELL_H;

/// 各卡自报高度（排布器唯一输入；tmux 高由 tmux_card_h 喂入）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainHeights {
    pub tmux: u32,
    pub link: u32,
    pub sys: u32,
}

impl ChainHeights {
    pub fn get(&self, id: ChainCardId) -> u32 {
        match id {
            ChainCardId::Tmux => self.tmux,
            ChainCardId::Link => self.link,
            ChainCardId::Sys => self.sys,
        }
    }
}

/// 高度收集唯一源（各卡自报高度只在本函数被问起——别处手抄
/// card_h/CARD_H 的加法账 = 回潮）
pub fn heights(tmux: u32, n_svc_lines: usize) -> ChainHeights {
    ChainHeights {
        tmux,
        link: link_card::card_h(n_svc_lines),
        sys: sys_card::CARD_H,
    }
}

/// 两本滚动账（左区/右上区各自独立钳制；常驻区不滚——tmux 卡内滚
/// 归 parser_page 自己的 scroll 账，不在本册）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrolls {
    pub left: i64,
    pub right: i64,
}

/// 三区几何（涂装/命中/手势唯一源——三处各算一份必漂移）
#[derive(Debug, Clone)]
pub struct Regions {
    /// 页全区（池区上吞标题行后）
    pub area: PoolRect,
    /// 左滚动区窗口
    pub left: PoolRect,
    /// 右上滚动区窗口
    pub right_top: PoolRect,
    /// 常驻槽（tmux 卡外框，已钉视口底）
    pub dock: PoolRect,
}

/// 页全区（v1 layout_vp 的页级几何收编本册）：池区 + 上吞标题行
/// TAB_ROW_H（2026-09-19 用户拍板页标题撤，卡区上吞）
pub fn page_area(screen_w: u32, screen_h: u32, bottom_inset: u32) -> PoolRect {
    let a = dual_pool::pool_area(screen_w, screen_h, bottom_inset);
    PoolRect {
        y: a.y - i64::from(crate::ui::tab_bar::TAB_ROW_H),
        h: a.h + crate::ui::tab_bar::TAB_ROW_H,
        ..a
    }
}

/// 三区几何唯一源。visible_bottom = 键盘感知可视底（parser_page::
/// visible_bottom 同一份尺）；tmux_h = tmux 卡自报高（tmux_card_h——
/// 封顶视口由调用方以 cap 喂入）。常驻槽钉底：dock.y = 可视底 − 卡高；
/// 右上区 = 区顶 → 槽顶 − DOCK_GAP；左区 = 区顶 → 可视底。
pub fn regions(
    screen_w: u32,
    screen_h: u32,
    bottom_inset: u32,
    visible_bottom: i64,
    tmux_h: u32,
) -> Regions {
    let area = page_area(screen_w, screen_h, bottom_inset);
    let rx = area.x + i64::from(area.w) - i64::from(RIGHT_COL_W);
    let dock = PoolRect {
        x: rx,
        y: visible_bottom - i64::from(tmux_h),
        w: RIGHT_COL_W,
        h: tmux_h,
    };
    let right_top = PoolRect {
        x: rx,
        y: area.y,
        w: RIGHT_COL_W,
        h: (dock.y - i64::from(DOCK_GAP) - area.y).max(0) as u32,
    };
    let left = PoolRect {
        x: area.x,
        y: area.y,
        w: area.w.saturating_sub(RIGHT_COL_W + REGION_GAP),
        h: (visible_bottom - area.y).max(0) as u32,
    };
    Regions {
        area,
        left,
        right_top,
        dock,
    }
}

/// 卡 → 区（注册表静态归属唯一源）
pub fn region_of(id: ChainCardId) -> Region {
    CHAIN
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("未注册链卡 {id:?}——CHAIN 漏席 = 装配错误"))
        .region
}

/// 区窗口（Region → 对应几何；Dock = 常驻槽）
pub fn window_of(region: Region, r: &Regions) -> &PoolRect {
    match region {
        Region::Dock => &r.dock,
        Region::RightTop => &r.right_top,
        Region::Left => &r.left,
    }
}

/// 区内链高（本区全部槽：高 + 前距——多卡区的链底账唯一源）
fn region_chain_h(region: Region, h: &ChainHeights) -> u32 {
    let mut total = 0;
    for slot in CHAIN.iter().filter(|s| s.region == region) {
        total += slot.gap_before + h.get(slot.id);
    }
    total
}

/// 区内槽顶（相对区顶；区内链式账唯一源）
fn slot_top_in(region: Region, id: ChainCardId, h: &ChainHeights) -> i64 {
    let mut y = 0;
    for slot in CHAIN.iter().filter(|s| s.region == region) {
        if slot.id == id {
            return y + i64::from(slot.gap_before);
        }
        y += i64::from(slot.gap_before + h.get(slot.id));
    }
    panic!("未注册链卡 {id:?}——CHAIN 漏席 = 装配错误")
}

/// 区滚动上限（两本账各自的 max 唯一源；Dock 恒 0——常驻不滚）
pub fn scroll_max(id: ChainCardId, r: &Regions, h: &ChainHeights) -> i64 {
    let region = region_of(id);
    if region == Region::Dock {
        return 0;
    }
    let win = window_of(region, r);
    i64::from(region_chain_h(region, h).saturating_sub(win.h)).max(0)
}

/// 槽外框（涂装/命中落位唯一源）：Dock = 常驻槽原样；滚动区槽 =
/// 区窗内位置 − eff_scroll（钳制语义唯一在本函数——上限吃
/// scroll_max 同一份账）
pub fn slot_rect(id: ChainCardId, r: &Regions, h: &ChainHeights, s: &Scrolls) -> PoolRect {
    let region = region_of(id);
    let win = window_of(region, r);
    if region == Region::Dock {
        return win.clone();
    }
    let raw = match region {
        Region::RightTop => s.right,
        Region::Left => s.left,
        Region::Dock => unreachable!(),
    };
    let eff = raw.clamp(0, scroll_max(id, r, h));
    PoolRect {
        x: win.x,
        y: win.y + slot_top_in(region, id, h) - eff,
        w: win.w,
        h: h.get(id),
    }
}

/// 区裁剪带（涂装断墨/命中闸门同一份）：滚动区 = 区窗纵段；
/// Dock = 常驻槽纵段（常驻卡整体可见，带 = 槽本身）
pub fn clip_of(id: ChainCardId, r: &Regions) -> (i64, i64) {
    let win = window_of(region_of(id), r);
    (win.y, win.y + i64::from(win.h))
}
