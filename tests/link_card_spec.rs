//! 连接服务合并卡考题（A 档）：几何（2026-09-21 三区排布 v2：归右上
//! 滚动区窄列——两竖列改两段纵排：连接段在上、服务段在下，段内全宽）、
//! 命中、卡高账两段相加——纯逻辑先行钉死，涂装在 termview（眼手同尺：
//! 两边吃 link_card 同一份 layout）。文案面考卷留在 conn_card_spec /
//! svc_card_spec（合并不动文案，只动几何）。
//!
//! 变异抽检：①card_h 漏段距/漏服务段（卡高账与布局漂移 = 末件出卡底）
//! 必须咬；②两段顺序颠倒（服务段跑连接段上 = 拍板语义反）必须咬；
//! ③段内不全宽（窄列里还按两竖列半分排 = 字段值挤爆）必须咬。

use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::link_card::{self, LinkHit, LinkLayout};
use kfm_na::ui::parser_chain;
use kfm_na::ui::parser_page as pp;

/// 排布器配给制 layout（与生产同路径：slot_rect 配给卡外框 → layout_in
/// 填卡内；本考题只钉卡内几何，槽位/滚动/裁剪的钉在 parser_chain_spec）
fn lay(n: usize) -> LinkLayout {
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::RIGHT_COL_W,
        h: link_card::card_h(n),
    };
    link_card::layout_in(card, n)
}

#[test]
fn spec_几何_两段纵排全宽() {
    let l = lay(2);
    let cx = l.card.x + i64::from(pp::CARD_PAD_H);
    let cw = l.card.w - pp::CARD_PAD_H * 2;
    // 变异③：段内必须全宽（窄列半分 = 字段值挤爆）
    assert_eq!(l.lheader.x, cx);
    assert_eq!(l.lheader.w, cw);
    for f in &l.lfields {
        assert_eq!(f.x, cx);
        assert_eq!(f.w, cw, "连接段字段行必须全宽");
    }
    assert_eq!(l.button.x, cx);
    assert_eq!(l.button.w, cw);
    assert_eq!(l.rheader.x, cx);
    assert_eq!(l.rheader.w, cw);
    for f in &l.rfields {
        assert_eq!(f.x, cx);
        assert_eq!(f.w, cw, "服务段字段行必须全宽");
    }
    for s in &l.sessions {
        assert_eq!(s.x, cx);
        assert_eq!(s.w, cw, "会话行必须全宽");
    }
    // 纵序：连接段头 → 连接段字段 → 钮 → 服务段头 → 服务段字段 → 会话行
    // （变异②：两段顺序颠倒必须咬）
    let fields_end = l.lfields[3].y + i64::from(l.lfields[3].h);
    assert_eq!(
        l.lfields[0].y,
        l.lheader.y + i64::from(l.lheader.h + pp::ROW_GAP)
    );
    assert_eq!(
        l.button.y,
        fields_end + i64::from(pp::ROW_GAP),
        "钮直接接连接段字段块（一行距）"
    );
    assert_eq!(
        l.rheader.y,
        l.button.y + i64::from(l.button.h + pp::ROW_GAP),
        "服务段头接钮底（段距一行）"
    );
    assert_eq!(
        l.rfields[0].y,
        l.rheader.y + i64::from(l.rheader.h + pp::ROW_GAP)
    );
    let rfields_end = l.rfields[3].y + i64::from(l.rfields[3].h);
    assert_eq!(
        l.sessions[0].y,
        rfields_end + i64::from(pp::ROW_GAP),
        "会话行接服务段字段块"
    );
}

#[test]
fn spec_几何_卡高账两段相加() {
    // card_h(n) = PAD_V·2 + 连接段 + 段距 + 服务段(n)（变异①：漏段距/
    // 漏服务段必须咬）
    assert_eq!(
        link_card::card_h(0),
        pp::CARD_PAD_V * 2 + link_card::LEFT_H + pp::ROW_GAP + link_card::right_h(0)
    );
    let n = 6;
    assert_eq!(
        link_card::card_h(n),
        pp::CARD_PAD_V * 2 + link_card::LEFT_H + pp::ROW_GAP + link_card::right_h(n)
    );
    // 单调：会话多一行卡高长一段
    assert!(link_card::card_h(3) > link_card::card_h(2));
    // 卡高账与布局同源：末件底 = 卡底 − PAD_V（n=0 末件 = 服务段末字段）
    let l0 = lay(0);
    let last0 = &l0.rfields[3];
    assert_eq!(
        last0.y + i64::from(last0.h),
        l0.card.y + i64::from(l0.card.h) - i64::from(pp::CARD_PAD_V),
        "n=0 末件必须贴内容区底（卡高账漏项 = 空洞/出底）"
    );
    let l6 = lay(6);
    let last6 = l6.sessions.last().unwrap();
    assert_eq!(
        last6.y + i64::from(last6.h),
        l6.card.y + i64::from(l6.card.h) - i64::from(pp::CARD_PAD_V),
        "n=6 末会话行必须贴内容区底"
    );
}

#[test]
fn spec_命中_只有重连可点() {
    let l = lay(2);
    let b = &l.button;
    assert_eq!(
        link_card::hit(&l, b.x + 1, b.y + 1),
        Some(LinkHit::Reconnect)
    );
    assert_eq!(
        link_card::hit(&l, b.x + i64::from(b.w), b.y),
        None,
        "右缘开区间"
    );
    // 字段行/卡头/会话行 = 纯展示不可点
    let f = &l.lfields[0];
    assert_eq!(link_card::hit(&l, f.x + 1, f.y + 1), None);
    assert_eq!(link_card::hit(&l, l.rheader.x + 1, l.rheader.y + 1), None);
    let s = &l.sessions[0];
    assert_eq!(link_card::hit(&l, s.x + 1, s.y + 1), None);
}
