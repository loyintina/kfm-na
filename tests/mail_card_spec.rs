//! mail_card_spec.rs — 信箱入口卡 A 档考题（BAR-212：解析页左下常驻槽，
//! 卡头「信箱」+ 两入口行，点行开页面级信件列表卡）。
//!
//! 判卷维度：卡高账（恒定——卡高不吃数据）/ 卡内几何（行距/内缩）/
//! 命中（行 → 册键一一咬合，行序 = MailKey::all() 声明序；卡头与卡外
//! 不吞）。
//!
//! 变异抽检：①行序颠倒（点主册开 NA信箱）必须咬；②卡高漏一行距
//! （两行间粘连/底行出框）必须咬；③命中边界半开区间错（行缝串行）
//! 必须咬。

use kfm_na::mail_feed::MailKey;
use kfm_na::termview::{CELL_H, CELL_W};
use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::{mail_card, parser_page as pp};

fn card() -> PoolRect {
    PoolRect {
        x: 0,
        y: 1000,
        w: 40 * CELL_W,
        h: mail_card::card_h(),
    }
}

#[test]
fn spec_bar212_卡高账恒定() {
    assert_eq!(
        mail_card::card_h(),
        pp::CARD_PAD_V * 2 + pp::ROW_H + pp::ROW_GAP + 2 * pp::BTN_H + pp::ROW_GAP
    );
    // 三级框纪律：入口行 ≥3 格（宪法最小框）——编译期咬
    const {
        assert!(mail_card::ENTRY_H >= 3 * CELL_H);
    }
}

#[test]
fn spec_bar212_卡内几何行序() {
    let l = mail_card::layout_in(card());
    // 卡头贴卡内顶
    assert_eq!(l.header.y, 1000 + i64::from(pp::CARD_PAD_V));
    // 两入口行：行距 ROW_GAP，等高 BTN_H，卡内全宽内缩
    assert_eq!(l.rows[0].h, pp::BTN_H);
    assert_eq!(l.rows[1].y - l.rows[0].y, (pp::BTN_H + pp::ROW_GAP) as i64);
    assert_eq!(l.rows[0].x, i64::from(pp::CARD_PAD_H));
    // 末行底恰抵卡内底（卡高账与布局账咬合——漏行距这行就红）
    let last_bottom = l.rows[1].y + i64::from(l.rows[1].h);
    assert_eq!(
        last_bottom,
        1000 + i64::from(mail_card::card_h()) - i64::from(pp::CARD_PAD_V)
    );
}

#[test]
fn spec_bar212_命中行序咬合() {
    let l = mail_card::layout_in(card());
    let cx = l.rows[0].x + 5;
    // 行序 = MailKey::all() 声明序（主册上、NA信箱下）
    assert_eq!(
        mail_card::hit(&l, cx, l.rows[0].y + 5),
        Some(MailKey::MainBook)
    );
    assert_eq!(
        mail_card::hit(&l, cx, l.rows[1].y + 5),
        Some(MailKey::NaBook)
    );
    // 行缝（ROW_GAP 段）不串行
    assert_eq!(
        mail_card::hit(&l, cx, l.rows[0].y + i64::from(pp::BTN_H) + 1),
        None
    );
    // 卡头/卡外不吞
    assert_eq!(mail_card::hit(&l, cx, l.header.y + 2), None);
    assert_eq!(mail_card::hit(&l, cx, 500), None);
}
