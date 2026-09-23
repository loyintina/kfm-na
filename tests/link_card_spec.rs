//! 连接服务合并卡考题（A 档）：几何（2026-09-21 三区排布 v2：归右上
//! 滚动区窄列——两段纵排：连接段在上、通道段在下，段内全宽）、命中、
//! 卡高账两段相加——纯逻辑先行钉死，涂装在 termview（眼手同尺：两边
//! 吃 link_card 同一份 layout）。文案面考卷留在 conn_card_spec /
//! svc_card_spec（合并不动文案，只动几何）。
//!
//! 2026-09-24 通道段改造（用户裁决：「服务」段退役换四口状态+调试钮）：
//! 会话行表退役 → 卡高恒定（不再吃会话数）；通道段尾 = 钮行半宽并排
//! [跳闸/投 QUIC]（左半）+ [重启]（右半）。
//!
//! 变异抽检：①card_h 漏段距/漏通道段（卡高账与布局漂移 = 末件出卡底）
//! 必须咬；②两段顺序颠倒（通道段跑连接段上 = 拍板语义反）必须咬；
//! ③段内不全宽（窄列里还按两竖列半分排 = 字段值挤爆）必须咬；
//! ④调试钮行不并排/错半宽（两钮叠合或错位 = 误点）必须咬。

use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::link_card::{self, LinkHit, LinkLayout};
use kfm_na::ui::parser_chain;
use kfm_na::ui::parser_page as pp;

/// 排布器配给制 layout（与生产同路径：slot_rect 配给卡外框 → layout_in
/// 填卡内；本考题只钉卡内几何，槽位/区窗/裁剪的钉在 parser_chain_spec）
fn lay() -> LinkLayout {
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: link_card::card_h(),
    };
    link_card::layout_in(card, 0)
}

#[test]
fn spec_几何_两段纵排全宽() {
    let l = lay();
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
        assert_eq!(f.w, cw, "通道段字段行必须全宽");
    }
    // 纵序：连接段头 → 连接段字段 → 钮 → 通道段头 → 通道段字段 →
    // 调试钮行（变异②：两段顺序颠倒必须咬）
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
        "通道段头接钮底（段距一行）"
    );
    assert_eq!(
        l.rfields[0].y,
        l.rheader.y + i64::from(l.rheader.h + pp::ROW_GAP)
    );
    let rfields_end = l.rfields[3].y + i64::from(l.rfields[3].h);
    assert_eq!(
        l.qbutton.y,
        rfields_end + i64::from(pp::ROW_GAP),
        "调试钮行接通道段字段块（一行距）"
    );
    assert_eq!(l.rbutton.y, l.qbutton.y, "两钮同行");
}

#[test]
fn spec_几何_调试钮行半宽并排() {
    // 变异④：[跳闸/投 QUIC] 左半 + [重启] 右半，间一距，合满全宽——
    // 叠合/错位/留洞都必须咬
    let l = lay();
    let cx = l.card.x + i64::from(pp::CARD_PAD_H);
    let cw = l.card.w - pp::CARD_PAD_H * 2;
    assert_eq!(l.qbutton.x, cx, "QUIC 钮贴左");
    assert_eq!(
        l.rbutton.x,
        l.qbutton.x + i64::from(l.qbutton.w + link_card::BTN_GAP),
        "重启钮接 QUIC 钮右缘（一距）"
    );
    assert_eq!(
        l.rbutton.x + i64::from(l.rbutton.w),
        cx + i64::from(cw),
        "两钮合满全宽（右缘贴内容区右）"
    );
    assert_eq!(l.qbutton.h, l.rbutton.h);
}

#[test]
fn spec_几何_卡高账两段相加_恒定() {
    // card_h() = PAD_V·2 + 连接段 + 段距 + 通道段（恒定——变异①：漏段距/
    // 漏通道段必须咬；2026-09-24 起不再吃会话数）
    assert_eq!(
        link_card::card_h(),
        pp::CARD_PAD_V * 2 + link_card::LEFT_H + pp::ROW_GAP + link_card::RIGHT_H
    );
    // 卡高账与布局同源：末件底 = 卡底 − PAD_V（末件恒为调试钮行）
    let l = lay();
    assert_eq!(
        l.rbutton.y + i64::from(l.rbutton.h),
        l.card.y + i64::from(l.card.h) - i64::from(pp::CARD_PAD_V),
        "末件（调试钮行）必须贴内容区底（卡高账漏项 = 空洞/出底）"
    );
}

#[test]
fn spec_命中_只有三钮可点() {
    let l = lay();
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
    // [跳闸/投 QUIC] 钮（通道段尾左半）可点
    let qb = &l.qbutton;
    assert_eq!(
        link_card::hit(&l, qb.x + 1, qb.y + 1),
        Some(LinkHit::QuicToggle)
    );
    // [重启] 钮（通道段尾右半）可点
    let rb = &l.rbutton;
    assert_eq!(
        link_card::hit(&l, rb.x + 1, rb.y + 1),
        Some(LinkHit::Restart)
    );
    assert_eq!(
        link_card::hit(&l, rb.x + 1, rb.y + i64::from(rb.h)),
        None,
        "下缘开区间"
    );
    // 两钮缝（BTN_GAP）不可点
    assert_eq!(
        link_card::hit(&l, qb.x + i64::from(qb.w) + 1, qb.y + 1),
        None,
        "钮缝不可点"
    );
    // 字段行/卡头 = 纯展示不可点
    let f = &l.lfields[0];
    assert_eq!(link_card::hit(&l, f.x + 1, f.y + 1), None);
    assert_eq!(link_card::hit(&l, l.rheader.x + 1, l.rheader.y + 1), None);
}

#[test]
fn spec_卡内滚_钉框动内容() {
    // v3（2026-09-21 用户拍板）：「弹起后右上区域做卡片的压缩，里面
    // 内容的滑动，不要做卡片的滑动」——卡框 = 区窗内静物（钳高归
    // slot_rect），内容随右账卡内平移（本册 layout_in 的 scroll 参，
    // 钳制语义唯一在此）
    let full = link_card::card_h();
    let frame_h = full / 2; // 区窗只容半张卡（键盘弹起相）
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: frame_h,
    };
    let l0 = link_card::layout_in(card.clone(), 0);
    // 内芯带 = 卡内沿 − PAD_V 纵段（涂装断墨/命中闸门同一份）
    assert_eq!(l0.content_clip.0, card.y + i64::from(pp::CARD_PAD_V));
    assert_eq!(
        l0.content_clip.1,
        card.y + i64::from(frame_h) - i64::from(pp::CARD_PAD_V)
    );
    // scroll = 100：全部件 y 平移 −100，卡框不动（变异：框跟内容一起
    // 动 = 区滑回潮，必须咬）
    let ls = link_card::layout_in(card.clone(), 100);
    assert_eq!(ls.card.y, l0.card.y);
    assert_eq!(ls.card.h, l0.card.h);
    assert_eq!(ls.lheader.y, l0.lheader.y - 100);
    assert_eq!(ls.button.y, l0.button.y - 100);
    assert_eq!(ls.qbutton.y, l0.qbutton.y - 100);
    // 超 max 钳到 max（max = 自报高 − 框高）
    let max = i64::from(full - frame_h);
    let lc = link_card::layout_in(card.clone(), 999_999);
    let lm = link_card::layout_in(card.clone(), max);
    assert_eq!(lc.lheader.y, lm.lheader.y, "钳制语义唯一在 layout_in");
    // 负值钳 0
    let ln = link_card::layout_in(card, -50);
    assert_eq!(ln.lheader.y, l0.lheader.y);
}

#[test]
fn spec_卡内滚_裁出内芯带的钮不可点() {
    let full = link_card::card_h();
    let frame_h = full / 2;
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: frame_h,
    };
    // 滚到 [重连] 钮完全画出内芯带上沿：点钮原位 = None（只显不点，
    // 命中闸门与涂装断墨同一份 content_clip）
    let l0 = link_card::layout_in(card.clone(), 0);
    let btn_mid = l0.button.y + i64::from(l0.button.h) / 2;
    let need = btn_mid - l0.content_clip.0 + 10; // 让钮中点滚出带上沿
    let ls = link_card::layout_in(card, need);
    assert!(ls.button.y + i64::from(ls.button.h) / 2 < ls.content_clip.0);
    assert_eq!(
        link_card::hit(
            &ls,
            ls.button.x + 5,
            ls.button.y + i64::from(ls.button.h) / 2
        ),
        None,
        "裁出内芯带的钮只显不点"
    );
    // 内芯带下沿外同样不可点
    assert_eq!(
        link_card::hit(&ls, ls.button.x + 5, ls.content_clip.1 + 5),
        None
    );
}
