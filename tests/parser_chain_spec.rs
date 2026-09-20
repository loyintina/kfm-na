//! parser_chain_spec.rs — 卡链排布器 A 档考题（两轴插件契约 §四：
//! 注册卡列表 × 各卡自报高度；卡对排布零感知，间距/落位/预留/链底
//! 四笔账唯一源都在排布器）

use kfm_na::termview::CELL_H;
use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::parser_chain::{self, ChainCardId, ChainHeights};
use kfm_na::ui::{link_card, sys_card};

fn h() -> ChainHeights {
    ChainHeights {
        tmux: 100,
        link: 50,
        sys: 40,
    }
}

#[test]
fn spec_chain_注册链三席有序() {
    // 现状三席：tmux → 连接服务合并卡 → 环境卡；首槽无前距，
    // 后两槽前距 = 一格（与原 LINK_GAP/SYS_GAP 同值——排布元数据
    // 收编排布器后数值不许漂移）
    assert_eq!(parser_chain::CHAIN.len(), 3);
    assert_eq!(parser_chain::CHAIN[0].id, ChainCardId::Tmux);
    assert_eq!(parser_chain::CHAIN[0].gap_before, 0);
    assert_eq!(parser_chain::CHAIN[1].id, ChainCardId::Link);
    assert_eq!(parser_chain::CHAIN[1].gap_before, CELL_H);
    assert_eq!(parser_chain::CHAIN[2].id, ChainCardId::Sys);
    assert_eq!(parser_chain::CHAIN[2].gap_before, CELL_H);
}

#[test]
fn spec_chain_槽顶链式账() {
    let h = h();
    // 槽顶 = tmux 卡顶 + 前方各槽（高 + 前距）
    assert_eq!(parser_chain::slot_top(ChainCardId::Tmux, 500, &h), 500);
    assert_eq!(
        parser_chain::slot_top(ChainCardId::Link, 500, &h),
        500 + 100 + i64::from(CELL_H)
    );
    assert_eq!(
        parser_chain::slot_top(ChainCardId::Sys, 500, &h),
        500 + 100 + i64::from(CELL_H) + 50 + i64::from(CELL_H)
    );
}

#[test]
fn spec_chain_链底与预留() {
    let h = h();
    // 链底 = 末槽底
    let expect_bottom = parser_chain::slot_top(ChainCardId::Sys, 500, &h) + 40;
    assert_eq!(parser_chain::chain_bottom(500, &h), expect_bottom);
    // 预留 = 本槽之后全部占位（前距 + 高）——tmux 卡底部预留带唯一源
    assert_eq!(
        parser_chain::reserved_below(ChainCardId::Tmux, &h),
        CELL_H + 50 + CELL_H + 40
    );
    assert_eq!(
        parser_chain::reserved_below(ChainCardId::Link, &h),
        CELL_H + 40
    );
    assert_eq!(parser_chain::reserved_below(ChainCardId::Sys, &h), 0);
}

#[test]
fn spec_chain_槽外框() {
    let h = h();
    let tmux_card = PoolRect {
        x: 43,
        y: 500,
        w: 900,
        h: 100,
    };
    let r = parser_chain::slot_rect(ChainCardId::Link, &tmux_card, &h);
    // x/w 与 tmux 卡同池区，y = 槽顶，h = 自报高
    assert_eq!(r.x, 43);
    assert_eq!(r.w, 900);
    assert_eq!(r.y, 500 + 100 + i64::from(CELL_H));
    assert_eq!(r.h, 50);
}

#[test]
fn spec_chain_高度收集自报同源() {
    // heights() = 各卡自报高度的唯一收集点：link = card_h(n)、
    // sys = CARD_H，别处不许手抄高度账
    let h = parser_chain::heights(999, 3);
    assert_eq!(h.tmux, 999);
    assert_eq!(h.link, link_card::card_h(3));
    assert_eq!(h.sys, sys_card::CARD_H);
}

#[test]
fn spec_chain_预留带_迁移等价() {
    // 迁移等价钉：排布器预留带 = 旧手抄账（LINK_GAP + card_h(n) +
    // SYS_GAP + CARD_H；两 GAP 均 = CELL_H）——旧 inset_extra_live +
    // INSET_EXTRA 六处消费点收编后数值不许变
    for n in [0usize, 1, 3, 8] {
        assert_eq!(
            parser_chain::reserved_below_tmux(n),
            CELL_H + link_card::card_h(n) + CELL_H + sys_card::CARD_H,
            "n={n} 预留带与旧账不等价"
        );
    }
}
