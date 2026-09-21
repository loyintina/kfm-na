//! parser_chain.rs — 三区排布器（解析页两轴插件契约 §四 v3，2026-09-21
//! 用户拍板：tmux 竖排常驻右下 + 右上滚动区（连接·服务卡）+ 左滚动区
//! （环境卡）——排版服从操作频率，「右滑→点窗口」恒定两步，常驻卡永不
//! 被滚出屏。v3 同日二拍：右列 16 格定宽 → **页全区 1/2 比例制**（窄列
//! 折行实锤后放宽）；右上区 = **钉顶钳高 + 卡内内容滚**（框不动内容动）；
//! 光球横向避让让出右列——orb_avoid_x 唯一源）
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

/// 右列宽比（宪法 §四 v3，2026-09-21 用户拍板「还是占半屏合适」——
/// v2 的 16 格定宽在窄列折行实锤后退役：长字段值/状态词折行不可读）。
/// 列宽 = 页全区宽 × NUM/DEN，网格对齐；比例单常量，日后翻 1/3 只动这里
pub const COL_W_NUM: u32 = 1;
pub const COL_W_DEN: u32 = 2;

/// 右列宽（网格对齐——列缘必须落格线，像素宪法）
pub fn col_w(area_w: u32) -> u32 {
    (area_w * COL_W_NUM / COL_W_DEN) / CELL_W * CELL_W
}
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
    let cw = col_w(area.w);
    let rx = area.x + i64::from(area.w) - i64::from(cw);
    let dock = PoolRect {
        x: rx,
        y: visible_bottom - i64::from(tmux_h),
        w: cw,
        h: tmux_h,
    };
    let right_top = PoolRect {
        x: rx,
        y: area.y,
        w: cw,
        h: (dock.y - i64::from(DOCK_GAP) - area.y).max(0) as u32,
    };
    let left = PoolRect {
        x: area.x,
        y: area.y,
        w: area.w.saturating_sub(cw + REGION_GAP),
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

/// 槽外框（涂装/命中落位唯一源）：Dock = 常驻槽原样；Left 滚动区槽 =
/// 区窗内位置 − eff_scroll（钳制语义唯一在本函数——上限吃
/// scroll_max 同一份账）；**RightTop = 钉顶钳高**（宪法 §四 v3，
/// 2026-09-21 用户拍板「弹起后右上区域做卡片的压缩，里面内容的滑动，
/// 不要做卡片的滑动」）：卡框钉区顶、高钳进区窗，右账滚动位移不进
/// 槽位、进卡内内容（link_card::layout_in 的 scroll 参数——框不动
/// 内容动，与 tmux 卡内滚同语言）。RightTop 单卡区专属语义——多卡区
/// 的链式压缩待有第二卡再设计
pub fn slot_rect(id: ChainCardId, r: &Regions, h: &ChainHeights, s: &Scrolls) -> PoolRect {
    let region = region_of(id);
    let win = window_of(region, r);
    if region == Region::Dock {
        return win.clone();
    }
    if region == Region::RightTop {
        return PoolRect {
            x: win.x,
            y: win.y,
            w: win.w,
            h: h.get(id).min(win.h),
        };
    }
    let raw = match region {
        Region::Left => s.left,
        Region::RightTop | Region::Dock => unreachable!(),
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

/// 光球横向避让（宪法 §四 v3，2026-09-21 用户拍板「做 ai 对话光球的
/// 横向避让，把 tmux 窗口让出来」）：解析页滑入时球让出右列——渲染
/// （paint_over）与命中（handle_touch 起手）同吃本函数，眼手同尺唯一源。
/// panel_off_x = 解析页当前缝采样偏移（0 = 靠泊，屏宽 = 屏外），避让量
/// 随页滑入度同步：球心 x 上限 = 右列左缘 − 球可视半径，超出部分按
/// progress 收敛（页进多少球让多少）。球被按住（拖拽中）= 不避让——
/// 跟手优先，松手后落回避让位。状态核 x 永不改写：避让是纯展示/命中层
/// 变换，拖球/边界钳制/默认出生位都不感知
pub fn orb_avoid_x(
    state_x: f64,
    pressed: bool,
    panel_off_x: i32,
    screen_w: u32,
    screen_h: u32,
    bottom_inset: u32,
) -> f64 {
    if pressed || screen_w == 0 {
        return state_x;
    }
    let progress = 1.0 - (f64::from(panel_off_x) / f64::from(screen_w)).clamp(0.0, 1.0);
    if progress <= 0.0 {
        return state_x;
    }
    let area = page_area(screen_w, screen_h, bottom_inset);
    let limit = (area.x + i64::from(area.w) - i64::from(col_w(area.w))) as f64
        - f64::from(crate::ai_presence::ORB_RADIUS_PX);
    if state_x <= limit {
        state_x
    } else {
        state_x - (state_x - limit) * progress
    }
}
