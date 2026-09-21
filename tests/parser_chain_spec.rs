//! parser_chain_spec.rs — 三区排布器 A 档考题（两轴插件契约 §四 v2，
//! 2026-09-21 用户拍板：tmux 竖排常驻右下 + 右上滚动区（连接·服务卡）
//! + 左滚动区（环境卡）——排版服从操作频率，常驻卡永不被滚出屏）
//!
//! 判卷维度：注册链区归属 / 三区几何（右列固定网格宽、常驻槽钉键盘
//! 感知可视底、区窗纵段）/ 槽位配给（滚动平移 + 钳制语义唯一）/
//! 两本滚动账 max / 区裁剪带 / 高度收集同源
//!
//! 变异抽检：①常驻槽钉顶不钉底（tmux 卡跑页首 = 拍板语义反）必须咬；
//! ②slot_rect 漏减滚动（卡不随账走）必须咬；③scroll_max 短内容出
//! 负窗（忘取 max(0)）必须咬；④右列宽随屏宽变（宪法：固定网格宽）
//! 必须咬。

use kfm_na::termview::CELL_H;
use kfm_na::ui::parser_chain::{self, ChainCardId, Region, Scrolls};
use kfm_na::ui::{link_card, sys_card};

const W: u32 = 1080;
const H: u32 = 2280;
/// 可视底（键盘感知——三区窗口与常驻槽钉底吃它）
const VB: i64 = 2000;

fn regs(tmux_h: u32) -> parser_chain::Regions {
    parser_chain::regions(W, H, 0, VB, tmux_h)
}

fn h(tmux: u32, n_svc: usize) -> parser_chain::ChainHeights {
    parser_chain::heights(tmux, n_svc)
}

const S0: Scrolls = Scrolls { left: 0, right: 0 };

#[test]
fn spec_注册链三席区归属() {
    // 现状三席：tmux→常驻区、连接·服务卡→右上滚动区、环境卡→左滚动区；
    // 区归属 = 注册表静态声明（卡自身零感知）
    assert_eq!(parser_chain::CHAIN.len(), 3);
    assert_eq!(parser_chain::CHAIN[0].id, ChainCardId::Tmux);
    assert_eq!(parser_chain::CHAIN[0].region, Region::Dock);
    assert_eq!(parser_chain::CHAIN[1].id, ChainCardId::Link);
    assert_eq!(parser_chain::CHAIN[1].region, Region::RightTop);
    assert_eq!(parser_chain::CHAIN[2].id, ChainCardId::Sys);
    assert_eq!(parser_chain::CHAIN[2].region, Region::Left);
    // region_of = 同一注册表的查询面（两源漂移禁）
    assert_eq!(parser_chain::region_of(ChainCardId::Tmux), Region::Dock);
    assert_eq!(parser_chain::region_of(ChainCardId::Link), Region::RightTop);
    assert_eq!(parser_chain::region_of(ChainCardId::Sys), Region::Left);
}

#[test]
fn spec_三区几何_右列定宽常驻钉底() {
    let r = regs(300);
    let area = parser_chain::page_area(W, H, 0);
    assert_eq!(r.area.x, area.x);
    assert_eq!(r.area.y, area.y);
    // 常驻槽：右列 × 钉可视底（变异①：钉顶必须咬）
    assert_eq!(r.dock.w, parser_chain::RIGHT_COL_W);
    assert_eq!(r.dock.x + i64::from(r.dock.w), area.x + i64::from(area.w));
    assert_eq!(r.dock.h, 300);
    assert_eq!(
        r.dock.y + i64::from(r.dock.h),
        VB,
        "常驻槽必须钉键盘感知可视底——永不被滚出屏"
    );
    // 右上区：同列，区顶 → 槽顶 − DOCK_GAP
    assert_eq!(r.right_top.x, r.dock.x);
    assert_eq!(r.right_top.w, parser_chain::RIGHT_COL_W);
    assert_eq!(r.right_top.y, area.y);
    assert_eq!(
        r.right_top.y + i64::from(r.right_top.h),
        r.dock.y - i64::from(parser_chain::DOCK_GAP),
        "右上区底必须让出常驻槽 + 槽距"
    );
    // 左区：区顶 → 可视底，宽 = 全区 − 右列 − 列距
    assert_eq!(r.left.x, area.x);
    assert_eq!(r.left.y, area.y);
    assert_eq!(
        r.left.w,
        area.w - parser_chain::RIGHT_COL_W - parser_chain::REGION_GAP
    );
    assert_eq!(r.left.y + i64::from(r.left.h), VB);
    // 变异④：右列宽不随屏宽变（宪法「固定网格宽」）
    let narrow = parser_chain::regions(600, 900, 0, 800, 300);
    assert_eq!(narrow.dock.w, parser_chain::RIGHT_COL_W);
    assert_eq!(narrow.right_top.w, parser_chain::RIGHT_COL_W);
}

#[test]
fn spec_槽位配给_scroll0贴区顶() {
    let r = regs(300);
    let hh = h(300, 2);
    // 常驻槽原样配给
    let t = parser_chain::slot_rect(ChainCardId::Tmux, &r, &hh, &S0);
    assert_eq!(t.x, r.dock.x);
    assert_eq!(t.y, r.dock.y);
    assert_eq!(t.w, r.dock.w);
    assert_eq!(t.h, r.dock.h);
    // 滚动区槽：scroll=0 贴区窗顶，宽 = 区宽，高 = 自报高
    let l = parser_chain::slot_rect(ChainCardId::Link, &r, &hh, &S0);
    assert_eq!(l.x, r.right_top.x);
    assert_eq!(l.y, r.right_top.y);
    assert_eq!(l.w, r.right_top.w);
    assert_eq!(l.h, link_card::card_h(2));
    let x = parser_chain::slot_rect(ChainCardId::Sys, &r, &hh, &S0);
    assert_eq!(x.x, r.left.x);
    assert_eq!(x.y, r.left.y);
    assert_eq!(x.w, r.left.w);
    assert_eq!(x.h, sys_card::CARD_H);
}

#[test]
fn spec_滚动账_max与钳制() {
    let r = regs(300);
    // 大 n 让连接卡高逾右上区窗 → 右账可滚；环境卡恒高 < 左区窗 → 0
    let hh = h(300, 50);
    let rt_h = i64::from(r.right_top.h);
    let expect_rmax = i64::from(link_card::card_h(50)) - rt_h;
    assert!(expect_rmax > 0, "本组参数右账必须真可滚");
    assert_eq!(
        parser_chain::scroll_max(ChainCardId::Link, &r, &hh),
        expect_rmax,
        "右账 max = 卡高 − 区窗高"
    );
    assert_eq!(
        parser_chain::scroll_max(ChainCardId::Sys, &r, &hh),
        0,
        "短内容 max 必须钳 0（变异③：负窗必须咬）"
    );
    assert_eq!(
        parser_chain::scroll_max(ChainCardId::Tmux, &r, &hh),
        0,
        "常驻区不滚（tmux 卡内滚归 parser_page 自己的账）"
    );
    // 槽位随账平移（变异②：漏减滚动必须咬）
    let s = Scrolls {
        left: 0,
        right: 120,
    };
    let l = parser_chain::slot_rect(ChainCardId::Link, &r, &hh, &s);
    assert_eq!(l.y, r.right_top.y - 120);
    // 超 max 钳到 max（钳制语义唯一在 slot_rect）
    let s2 = Scrolls {
        left: 0,
        right: 999_999,
    };
    let l2 = parser_chain::slot_rect(ChainCardId::Link, &r, &hh, &s2);
    assert_eq!(l2.y, r.right_top.y - expect_rmax);
    // 负值钳 0
    let s3 = Scrolls {
        left: -50,
        right: -50,
    };
    let l3 = parser_chain::slot_rect(ChainCardId::Link, &r, &hh, &s3);
    assert_eq!(l3.y, r.right_top.y);
    let x3 = parser_chain::slot_rect(ChainCardId::Sys, &r, &hh, &s3);
    assert_eq!(x3.y, r.left.y, "max=0 的区任何脏滚动都钳回区顶");
}

#[test]
fn spec_区裁剪带() {
    let r = regs(300);
    // 涂装断墨/命中闸门同一份：滚动区 = 区窗纵段；常驻区 = 槽纵段
    assert_eq!(
        parser_chain::clip_of(ChainCardId::Link, &r),
        (r.right_top.y, r.right_top.y + i64::from(r.right_top.h))
    );
    assert_eq!(
        parser_chain::clip_of(ChainCardId::Sys, &r),
        (r.left.y, r.left.y + i64::from(r.left.h))
    );
    assert_eq!(
        parser_chain::clip_of(ChainCardId::Tmux, &r),
        (r.dock.y, r.dock.y + i64::from(r.dock.h))
    );
    // 右上区带底不许淹进常驻槽（层级律：两区零重叠）
    let (_, lbot) = parser_chain::clip_of(ChainCardId::Link, &r);
    let (dtop, _) = parser_chain::clip_of(ChainCardId::Tmux, &r);
    assert!(lbot < dtop, "右上区带底必须停在常驻槽顶以上（含槽距）");
}

#[test]
fn spec_高度收集自报同源() {
    // heights() = 各卡自报高度的唯一收集点：link = card_h(n)、
    // sys = CARD_H，别处不许手抄高度账
    let hh = parser_chain::heights(999, 3);
    assert_eq!(hh.tmux, 999);
    assert_eq!(hh.link, link_card::card_h(3));
    assert_eq!(hh.sys, sys_card::CARD_H);
    // DOCK_GAP 与链槽间距同档（一格——排布元数据收编后数值不许漂移）
    assert_eq!(parser_chain::DOCK_GAP, CELL_H);
}
