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
//! 2026-09-30 BAR-212（用户拍板：连接段「重拉」「错误」两行提示没用）：
//! 连接段字段行 4 → 2（只剩 目标/本地口），左右段行数分家
//! （L_FIELDS=2 / R_FIELDS=4），LEFT_H 与 card_h() 账随之收。
//!
//! 变异抽检：①card_h 漏段距/漏通道段（卡高账与布局漂移 = 末件出卡底）
//! 必须咬；②两段顺序颠倒（通道段跑连接段上 = 拍板语义反）必须咬；
//! ③段内不全宽（窄列里还按两竖列半分排 = 字段值挤爆）必须咬；
//! ④调试钮行不并排/错半宽（两钮叠合或错位 = 误点）必须咬；
//! ⑤连接段行数回潮成 4（重拉/错误两行复活 = BAR-212 白干）必须咬。

use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::link_card::{self, LinkHit, LinkLayout, LinkLines};
use kfm_na::ui::parser_chain;
use kfm_na::ui::parser_page as pp;

/// 排布器配给制 layout（与生产同路径：slot_rect 配给卡外框 → layout_in
/// 填卡内；本考题只钉卡内几何，槽位/区窗/裁剪的钉在 parser_chain_spec）。
/// 折行账全单行 = 旧恒定几何（BAR-206 打回重做：折行往下长的钉在
/// spec_几何_折行账_* 三钉）
fn lay() -> LinkLayout {
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: link_card::card_h(&LinkLines::SINGLE, 0),
    };
    link_card::layout_in(card, 0, &LinkLines::SINGLE, 0)
}

#[test]
fn spec_几何_两段纵排全宽() {
    let l = lay();
    let cx = l.card.x + i64::from(pp::CARD_PAD_H);
    let cw = l.card.w - pp::CARD_PAD_H * 2;
    // 变异⑤：连接段两行 / 通道段四行，行数分家（回潮成同一份 N_FIELDS
    // = 重拉/错误复活或四口缺行，都必须咬）
    assert_eq!(link_card::L_FIELDS, 2);
    assert_eq!(link_card::R_FIELDS, 4);
    assert_eq!(l.lfields.len(), 2, "连接段只剩 目标/本地口 两行");
    assert_eq!(l.rfields.len(), 4, "通道段四口状态四行不动");
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
    let fields_end = l.lfields[1].y + i64::from(l.lfields[1].h);
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
    // card_h(SINGLE, _) = PAD_V·2 + 连接段 + 段距 + 通道段（全单行 = 恒定
    // ——变异①：漏段距/漏通道段必须咬；2026-09-24 起不再吃会话数；
    // 2026-10-01 起吃折行账，全 1 行 = 本参考值不变）
    assert_eq!(
        link_card::card_h(&LinkLines::SINGLE, 0),
        pp::CARD_PAD_V * 2 + link_card::LEFT_H + pp::ROW_GAP + link_card::RIGHT_H
    );
    // adv 对全单行账零影响（(n−1)×adv 恒 0）
    assert_eq!(
        link_card::card_h(&LinkLines::SINGLE, 40),
        link_card::card_h(&LinkLines::SINGLE, 0),
        "全单行时行推进不进账"
    );
    // BAR-212：连接段字段块 = 两行（2 行 + 1 行距）——LEFT_H 账同步收
    // （行数回潮成 4 = 卡高虚高留空洞，必须咬）
    assert_eq!(
        link_card::LEFT_H,
        pp::ROW_H
            + pp::ROW_GAP
            + (2 * link_card::FIELD_H + link_card::FIELD_GAP)
            + pp::ROW_GAP
            + pp::BTN_H,
        "连接段高 = 卡头 + 两字段行 + [重连] 钮"
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
    let full = link_card::card_h(&LinkLines::SINGLE, 0);
    let frame_h = full / 2; // 区窗只容半张卡（键盘弹起相）
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: frame_h,
    };
    let l0 = link_card::layout_in(card.clone(), 0, &LinkLines::SINGLE, 0);
    // 内芯带 = 卡内沿 − PAD_V 纵段（涂装断墨/命中闸门同一份）
    assert_eq!(l0.content_clip.0, card.y + i64::from(pp::CARD_PAD_V));
    assert_eq!(
        l0.content_clip.1,
        card.y + i64::from(frame_h) - i64::from(pp::CARD_PAD_V)
    );
    // scroll = 100：全部件 y 平移 −100，卡框不动（变异：框跟内容一起
    // 动 = 区滑回潮，必须咬）
    let ls = link_card::layout_in(card.clone(), 100, &LinkLines::SINGLE, 0);
    assert_eq!(ls.card.y, l0.card.y);
    assert_eq!(ls.card.h, l0.card.h);
    assert_eq!(ls.lheader.y, l0.lheader.y - 100);
    assert_eq!(ls.button.y, l0.button.y - 100);
    assert_eq!(ls.qbutton.y, l0.qbutton.y - 100);
    // 超 max 钳到 max（max = 自报高 − 框高）
    let max = i64::from(full - frame_h);
    let lc = link_card::layout_in(card.clone(), 999_999, &LinkLines::SINGLE, 0);
    let lm = link_card::layout_in(card.clone(), max, &LinkLines::SINGLE, 0);
    assert_eq!(lc.lheader.y, lm.lheader.y, "钳制语义唯一在 layout_in");
    // 负值钳 0
    let ln = link_card::layout_in(card, -50, &LinkLines::SINGLE, 0);
    assert_eq!(ln.lheader.y, l0.lheader.y);
}

#[test]
fn spec_卡内滚_裁出内芯带的钮不可点() {
    let full = link_card::card_h(&LinkLines::SINGLE, 0);
    let frame_h = full / 2;
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: frame_h,
    };
    // 滚到 [重连] 钮完全画出内芯带上沿：点钮原位 = None（只显不点，
    // 命中闸门与涂装断墨同一份 content_clip）
    let l0 = link_card::layout_in(card.clone(), 0, &LinkLines::SINGLE, 0);
    let btn_mid = l0.button.y + i64::from(l0.button.h) / 2;
    let need = btn_mid - l0.content_clip.0 + 10; // 让钮中点滚出带上沿
    let ls = link_card::layout_in(card, need, &LinkLines::SINGLE, 0);
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

// ── 折行账（BAR-206 打回重做，2026-10-01 用户裁定：省略 = 信息丢失——
// 字段值格折行往下长，行高/卡高随折行数长，永不删字）────────────────
//
// 变异抽检预期（cp 备份改坏看红、备份恢复）：
// ⑥field_row_h 漏乘 (n−1)（折行进账但只有 1 行的量）→ 「行高随折行长」红；
// ⑦折行长了行高但布局不跟（lfields[1]/钮仍按 FIELD_H 等步进）→
//   「布局跟折行账不重叠」红；
// ⑧卡高不吃折行账（card_h 仍恒定）→ 「卡高随折行长」红。

#[test]
fn spec_几何_折行账_行高随折行长() {
    let adv = 40u32; // meta 行推进（真机 zoom 1.2778 ≈ 51，夹具取整）
    let lines = LinkLines {
        l: [2, 1],
        r: [1, 1, 1, 1],
    };
    let card = PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: link_card::card_h(&lines, adv),
    };
    let l = link_card::layout_in(card, 0, &lines, adv);
    // 变异⑥：2 行值 = FIELD_H + 1×adv（不多不少）
    assert_eq!(l.lfields[0].h, link_card::FIELD_H + adv, "2 行值行高");
    assert_eq!(l.lfields[1].h, link_card::FIELD_H, "单行值行高不变");
    assert_eq!(l.rfields[0].h, link_card::FIELD_H);
    // 变异⑦：下一行/钮跟折行账走（等步进回潮 = 重叠必须咬）
    assert_eq!(
        l.lfields[1].y,
        l.lfields[0].y + i64::from(l.lfields[0].h + link_card::FIELD_GAP),
        "下一字段行接上一行底（不吃等步进）"
    );
    assert_eq!(
        l.button.y,
        l.lfields[1].y + i64::from(l.lfields[1].h + pp::ROW_GAP),
        "钮接折行后的字段块底"
    );
    // 三行值 = FIELD_H + 2×adv
    let lines3 = LinkLines {
        l: [3, 1],
        r: [1, 1, 1, 1],
    };
    let card3 = PoolRect {
        h: link_card::card_h(&lines3, adv),
        ..card_shell()
    };
    let l3 = link_card::layout_in(card3, 0, &lines3, adv);
    assert_eq!(l3.lfields[0].h, link_card::FIELD_H + 2 * adv, "3 行值行高");
}

/// 卡壳夹具（折行钉共用）
fn card_shell() -> PoolRect {
    PoolRect {
        x: 40,
        y: 300,
        w: parser_chain::col_w(parser_chain::page_area(1080, 2280, 0).w),
        h: 0,
    }
}

#[test]
fn spec_几何_折行账_卡高随折行长() {
    let adv = 40u32;
    let base = link_card::card_h(&LinkLines::SINGLE, adv);
    // 变异⑧：一个字段折 2 行 = 卡高 +1×adv；两个字段各折 2 行 = +2×adv
    let one = LinkLines {
        l: [2, 1],
        r: [1, 1, 1, 1],
    };
    assert_eq!(
        link_card::card_h(&one, adv),
        base + adv,
        "卡高随一字段折行长 1×adv"
    );
    let two = LinkLines {
        l: [2, 1],
        r: [1, 2, 1, 1],
    };
    assert_eq!(
        link_card::card_h(&two, adv),
        base + 2 * adv,
        "两段折行都进账"
    );
    // 卡高账与布局同源（折行相）：末件底 = 卡底 − PAD_V
    let card = PoolRect {
        h: link_card::card_h(&two, adv),
        ..card_shell()
    };
    let l = link_card::layout_in(card.clone(), 0, &two, adv);
    assert_eq!(
        l.rbutton.y + i64::from(l.rbutton.h),
        card.y + i64::from(card.h) - i64::from(pp::CARD_PAD_V),
        "折行相末件仍贴内容区底（账漏项 = 空洞/出底）"
    );
}

#[test]
fn spec_几何_折行账_字段互不争盒() {
    // 判卷口径 §五.2 不压盖：相邻字段行/钮零重叠（折行相）
    let adv = 40u32;
    let lines = LinkLines {
        l: [3, 2],
        r: [2, 1, 4, 1],
    };
    let card = PoolRect {
        h: link_card::card_h(&lines, adv),
        ..card_shell()
    };
    let l = link_card::layout_in(card, 0, &lines, adv);
    let no_overlap = |a: &PoolRect, b: &PoolRect, what: &str| {
        assert!(
            a.y + i64::from(a.h) <= b.y,
            "{what} 重叠（a 底 {} > b 顶 {}）",
            a.y + i64::from(a.h),
            b.y
        );
    };
    no_overlap(&l.lheader, &l.lfields[0], "连接段头/首字段");
    no_overlap(&l.lfields[0], &l.lfields[1], "连接段字段间");
    no_overlap(&l.lfields[1], &l.button, "字段/钮");
    no_overlap(&l.button, &l.rheader, "钮/通道段头");
    for w in l.rfields.windows(2) {
        no_overlap(&w[0], &w[1], "通道段字段间");
    }
    no_overlap(&l.rfields[3], &l.qbutton, "通道段末字段/调试钮");
}
