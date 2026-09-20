//! parser_chain.rs — 卡链排布器（解析页两轴插件契约 §四，宪法
//! docs/active/解析页.md：注册卡列表 × 各卡自报高度）
//!
//! 收编前病灶：链底账写死在 parser_page::layout_vp（三卡顺序 +
//! 高度常量跨卡 import），tmux 卡底部预留带手抄
//! link_card::inset_extra_live()+sys_card::INSET_EXTRA 六处，
//! link_card::layout/sys_card::layout 各自知道「我接在谁的正下方」
//! ——卡知排布 = 插拔灾难。收编后：
//!
//! - 注册链 CHAIN = 唯一有序卡表（新功能律：加卡 = 加一行 +
//!   自报高度，排布/预留/链底账零改动）；
//! - 卡间距是排布元数据，归排布器私有——卡对排布零感知；
//! - 槽顶/链底/预留/槽外框四笔账唯一源都在本册；
//! - 高度收集 heights() = 各卡 card_h/CARD_H 的唯一询问点，
//!   别处不许手抄高度账。

use crate::termview::CELL_H;
use crate::ui::dual_pool::PoolRect;
use crate::ui::{link_card, sys_card};

/// 链上卡 id（注册链席位）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainCardId {
    /// tmux 会话卡（首卡——卡高账在 parser_page::layout_vp，动态）
    Tmux,
    /// 连接服务合并卡
    Link,
    /// 环境卡
    Sys,
}

/// 注册槽：gap_before = 与上一卡的间距（首槽恒 0）
pub struct ChainSlot {
    pub id: ChainCardId,
    pub gap_before: u32,
}

/// 注册链（有序——页面滚动窗/落位/预留全从本表出）。间距数值 =
/// 原 link_card::LINK_GAP / sys_card::SYS_GAP（均一格），收编不漂移
pub static CHAIN: &[ChainSlot] = &[
    ChainSlot {
        id: ChainCardId::Tmux,
        gap_before: 0,
    },
    ChainSlot {
        id: ChainCardId::Link,
        gap_before: CELL_H,
    },
    ChainSlot {
        id: ChainCardId::Sys,
        gap_before: CELL_H,
    },
];

/// 各卡自报高度（排布器唯一输入；tmux 高由 layout_vp 的卡高账喂入）
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

/// 槽顶 y（链式账唯一源）：tmux 卡顶 + 前方各槽（高 + 前距）。
/// 未注册 id = 装配错误显形，不许静默
pub fn slot_top(id: ChainCardId, tmux_top: i64, h: &ChainHeights) -> i64 {
    let mut y = tmux_top;
    for (i, slot) in CHAIN.iter().enumerate() {
        if slot.id == id {
            return y;
        }
        y += i64::from(h.get(slot.id));
        if let Some(next) = CHAIN.get(i + 1) {
            y += i64::from(next.gap_before);
        }
    }
    panic!("未注册链卡 {id:?}——CHAIN 漏席 = 装配错误")
}

/// 链底 = 末槽底（页面滚动窗的链底账唯一源）
pub fn chain_bottom(tmux_top: i64, h: &ChainHeights) -> i64 {
    let last = CHAIN.last().expect("CHAIN 非空——注册链是静态表");
    slot_top(last.id, tmux_top, h) + i64::from(h.get(last.id))
}

/// 本槽之后全部占位（前距 + 高的总和）——tmux 卡底部预留带唯一源
/// （收编前 = inset_extra_live()+INSET_EXTRA 手抄六处）
pub fn reserved_below(id: ChainCardId, h: &ChainHeights) -> u32 {
    (chain_bottom(0, h) - slot_top(id, 0, h) - i64::from(h.get(id))) as u32
}

/// tmux 卡底部预留带便捷面（活件：n_svc_lines 每帧从 svc 快照读）
pub fn reserved_below_tmux(n_svc_lines: usize) -> u32 {
    reserved_below(ChainCardId::Tmux, &heights(0, n_svc_lines))
}

/// 槽外框：x/w 与 tmux 卡同池区，y = 槽顶，h = 自报高（涂装/命中
/// 落位唯一源——卡不再知道「我接在谁下面」）
pub fn slot_rect(id: ChainCardId, tmux_card: &PoolRect, h: &ChainHeights) -> PoolRect {
    PoolRect {
        x: tmux_card.x,
        y: slot_top(id, tmux_card.y, h),
        w: tmux_card.w,
        h: h.get(id),
    }
}
