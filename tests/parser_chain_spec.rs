//! parser_chain_spec.rs — 三区排布器 A 档考题（两轴插件契约 §四 v3，
//! 2026-09-21 用户拍板：tmux 竖排常驻右下 + 右上滚动区（连接·服务卡）
//! + 左滚动区（环境卡）——排版服从操作频率，常驻卡永不被滚出屏。
//!
//! v3 同日二拍：右列 16 格定宽 → 页全区 **1/2 比例制**；右上区 = **钉顶
//! 钳高 + 卡内内容滚**（框不动内容动）；光球横向避让让出右列）
//!
//! 判卷维度：注册链区归属 / 三区几何（右列比例宽网格对齐、常驻槽钉
//! 键盘感知可视底、区窗纵段）/ 槽位配给（Left 滚动平移钳制 + RightTop
//! 钉顶钳高）/ 两本滚动账 max / 区裁剪带 / 高度收集同源 / 光球避让
//!
//! 变异抽检：①常驻槽钉顶不钉底（tmux 卡跑页首 = 拍板语义反）必须咬；
//! ②Left 槽位漏减滚动（卡不随账走）必须咬；③scroll_max 短内容出
//! 负窗（忘取 max(0)）必须咬；④右列宽忘网格对齐/比例错（宪法 v3：
//! 页全区 1/2）必须咬；⑤RightTop 槽位回潮成区滑（钉顶钳高语义反）
//! 必须咬；⑥光球避让忘乘 progress/忘减球半径/pressed 也避让必须咬。

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

fn h(tmux: u32, _n_svc: usize) -> parser_chain::ChainHeights {
    // 2026-09-24 通道段改造：link 卡高恒定（会话行表退役），n_svc 形参
    // 退役——保留夹具签名免大面积改写调用点
    parser_chain::heights(tmux, sys_card::CARD_H)
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
fn spec_三区几何_右列比例宽常驻钉底() {
    let r = regs(300);
    let area = parser_chain::page_area(W, H, 0);
    let cw = parser_chain::col_w(area.w);
    assert_eq!(r.area.x, area.x);
    assert_eq!(r.area.y, area.y);
    // 右列 = 页全区 1/2 比例制（宪法 v3），网格对齐（变异④：比例错/
    // 忘对齐必须咬）
    assert_eq!(cw % kfm_na::termview::CELL_W, 0, "列缘必须落格线");
    assert!(
        (i64::from(cw) * 2 - i64::from(area.w)).abs() <= i64::from(kfm_na::termview::CELL_W) * 2,
        "右列宽必须 ≈ 页全区一半（半宽取整 + 网格取整双截尾，两格公差内）"
    );
    // 常驻槽：右列 × 钉可视底（变异①：钉顶必须咬）
    assert_eq!(r.dock.w, cw);
    assert_eq!(r.dock.x + i64::from(r.dock.w), area.x + i64::from(area.w));
    assert_eq!(r.dock.h, 300);
    assert_eq!(
        r.dock.y + i64::from(r.dock.h),
        VB,
        "常驻槽必须钉键盘感知可视底——永不被滚出屏"
    );
    // 右上区：同列，区顶 → 槽顶 − DOCK_GAP
    assert_eq!(r.right_top.x, r.dock.x);
    assert_eq!(r.right_top.w, cw);
    assert_eq!(r.right_top.y, area.y);
    assert_eq!(
        r.right_top.y + i64::from(r.right_top.h),
        r.dock.y - i64::from(parser_chain::DOCK_GAP),
        "右上区底必须让出常驻槽 + 槽距"
    );
    // 左区：区顶 → 可视底，宽 = 全区 − 右列 − 列距
    assert_eq!(r.left.x, area.x);
    assert_eq!(r.left.y, area.y);
    assert_eq!(r.left.w, area.w - cw - parser_chain::REGION_GAP);
    assert_eq!(r.left.y + i64::from(r.left.h), VB);
    // 比例制 = 随页全区宽走（v2 定宽回潮必须咬）：窄屏列宽同比缩
    let narrow = parser_chain::regions(600, 900, 0, 800, 300);
    let narea = parser_chain::page_area(600, 900, 0);
    assert_eq!(narrow.dock.w, parser_chain::col_w(narea.w));
    assert_eq!(narrow.right_top.w, parser_chain::col_w(narea.w));
    assert!(narrow.dock.w < cw, "窄屏右列必须同比收窄（定宽回潮咬）");
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
    // 右上区槽（v3 钉顶钳高）：贴区窗顶，宽 = 区宽，高 = min(自报高, 区窗高)
    let l = parser_chain::slot_rect(ChainCardId::Link, &r, &hh, &S0);
    assert_eq!(l.x, r.right_top.x);
    assert_eq!(l.y, r.right_top.y);
    assert_eq!(l.w, r.right_top.w);
    assert_eq!(l.h, link_card::card_h().min(r.right_top.h));
    let x = parser_chain::slot_rect(ChainCardId::Sys, &r, &hh, &S0);
    assert_eq!(x.x, r.left.x);
    assert_eq!(x.y, r.left.y);
    assert_eq!(x.w, r.left.w);
    assert_eq!(x.h, sys_card::CARD_H);
}

#[test]
fn spec_滚动账_max与钳制() {
    // 2026-09-24 通道段改造：link 卡高恒定 ~1206 ——大 tmux 让常驻槽吃
    // 高、右上区窗比卡矮 → 右账可滚；环境卡恒高 < 左区窗 → 0
    let r = regs(1500);
    let hh = h(1500, 0);
    let rt_h = i64::from(r.right_top.h);
    let expect_rmax = i64::from(link_card::card_h()) - rt_h;
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
    // RightTop 钉顶钳高（v3）：滚动位移不进槽位（进卡内内容）——
    // 任意右账脏值槽位都钉区顶、高钳区窗（变异⑤：回潮成区滑必须咬）
    for raw in [0, 120, 999_999, -50] {
        let s = Scrolls {
            left: 0,
            right: raw,
        };
        let l = parser_chain::slot_rect(ChainCardId::Link, &r, &hh, &s);
        assert_eq!(l.y, r.right_top.y, "右账 {raw}：右上卡框必须钉区顶");
        assert_eq!(l.h, r.right_top.h, "右账 {raw}：卡高必须钳进区窗");
    }
    // Left 区滑照旧（变异②：漏减滚动必须咬）——压可视底造左区可滚相
    let sq = parser_chain::regions(W, H, 0, area_y() + 200, 100);
    let sh_max = parser_chain::scroll_max(ChainCardId::Sys, &sq, &hh);
    assert!(sh_max > 0, "本组参数左账必须真可滚");
    let s = Scrolls {
        left: 120,
        right: 0,
    };
    let x = parser_chain::slot_rect(ChainCardId::Sys, &sq, &hh, &s);
    assert_eq!(x.y, sq.left.y - 120);
    // 超 max 钳到 max（钳制语义唯一在 slot_rect）
    let s2 = Scrolls {
        left: 999_999,
        right: 0,
    };
    let x2 = parser_chain::slot_rect(ChainCardId::Sys, &sq, &hh, &s2);
    assert_eq!(x2.y, sq.left.y - sh_max);
    // 负值钳 0
    let s3 = Scrolls {
        left: -50,
        right: -50,
    };
    let x3 = parser_chain::slot_rect(ChainCardId::Sys, &sq, &hh, &s3);
    assert_eq!(x3.y, sq.left.y, "脏负滚动必须钳回区顶");
}

/// 区顶 y（page_area 的同源读数——压可视底造可滚相的辅助）
fn area_y() -> i64 {
    parser_chain::page_area(W, H, 0).y
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
    // heights() = 各卡自报高度的唯一收集点：link = card_h()（恒定）、
    // sys = CARD_H，别处不许手抄高度账
    let hh = parser_chain::heights(999, sys_card::CARD_H);
    assert_eq!(hh.tmux, 999);
    assert_eq!(hh.link, link_card::card_h());
    assert_eq!(hh.sys, sys_card::CARD_H);
    // DOCK_GAP 与链槽间距同档（一格——排布元数据收编后数值不许漂移）
    assert_eq!(parser_chain::DOCK_GAP, CELL_H);
}

#[test]
fn spec_光球横向避让() {
    let area = parser_chain::page_area(W, H, 0);
    let limit = (area.x + i64::from(area.w) - i64::from(parser_chain::col_w(area.w))) as f64
        - f64::from(kfm_na::ai_presence::ORB_RADIUS_PX);
    let over = limit + 300.0; // 球心在右列里的态
    let inside = limit - 200.0; // 球心本就在右列外的态
    let avoid =
        |x: f64, pressed: bool, off: i32| parser_chain::orb_avoid_x(x, pressed, off, W, H, 0);
    // 屏外（off = 屏宽，progress = 0）→ 不动（变异⑥：忘乘 progress 咬）
    assert_eq!(avoid(over, false, W as i32), over);
    // 靠泊（off = 0）+ 球心在右列 → 收到右列左缘 − 球半径（变异⑥：
    // 忘减球半径咬）
    assert_eq!(avoid(over, false, 0), limit);
    // 靠泊 + 球心本就在右列外 → 不动（不误伤左区用户位）
    assert_eq!(avoid(inside, false, 0), inside);
    // 半程滑入：避让量 = 超出部分 × progress（页进多少球让多少）
    let half = avoid(over, false, (W / 2) as i32);
    assert!(
        (half - (over + limit) / 2.0).abs() < 1e-9,
        "半程必须收敛一半"
    );
    // 按住拖拽中 = 不避让（跟手优先；变异⑥：pressed 也避让咬）
    assert_eq!(avoid(over, true, 0), over);
}
