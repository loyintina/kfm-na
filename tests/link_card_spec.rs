//! 连接服务合并卡考题（A 档）：几何（一张二级卡两竖列，左连接右服务）、
//! 命中、预留量同源——纯逻辑先行钉死，涂装在 termview（眼手同尺，
//! 两边吃 link_card 同一份 layout）。文案面考卷留在 conn_card_spec /
//! svc_card_spec（合并不动文案，只动几何）。
//!
//! 变异抽检：①inset_extra_live 漏加 LINK_GAP（预留量与实高漂移 =
//! 两卡相叠/底部空洞）必须咬；②两列宽不等（列宽账错 = 左右列一大
//! 一小）必须咬；③[重连] 钮不钉列底（右列长时钮吊在字段后悬空）
//! 必须咬；④card_h 取 min 不取 max（高列内容出卡底）必须咬。

use kfm_na::ui::dual_pool::PoolRect;
use kfm_na::ui::link_card::{self, LinkHit};
use kfm_na::ui::parser_page as pp;

fn tmux_card() -> PoolRect {
    PoolRect {
        x: 40,
        y: 300,
        w: 1000,
        h: 800,
    }
}

#[test]
fn spec_几何_接在tmux卡下() {
    let tc = tmux_card();
    let l = link_card::layout(&tc, 2);
    assert_eq!(l.card.x, tc.x, "与 tmux 卡同左右缘（二级卡同池区宽）");
    assert_eq!(l.card.w, tc.w);
    assert_eq!(
        l.card.y,
        tc.y + i64::from(tc.h) + i64::from(link_card::LINK_GAP),
        "接在 tmux 卡正下方，间距 LINK_GAP"
    );
    assert_eq!(l.card.h, link_card::card_h(2), "卡高 = 卡高账同源");
}

#[test]
fn spec_几何_两竖列等宽半分() {
    let l = link_card::layout(&tmux_card(), 2);
    let cx = l.card.x + i64::from(pp::CARD_PAD_H);
    let cw = l.card.w - pp::CARD_PAD_H * 2;
    let col_w = (cw - pp::COL_GAP) / 2;
    // 左列
    assert_eq!(l.lheader.x, cx);
    assert_eq!(l.lheader.w, col_w, "左列宽 = (内容宽−列距)/2");
    for f in &l.lfields {
        assert_eq!(f.x, cx);
        assert_eq!(f.w, col_w);
    }
    assert_eq!(l.button.x, cx);
    assert_eq!(l.button.w, col_w);
    // 右列（变异②：两列宽不等必须咬）
    let rx = cx + i64::from(col_w + pp::COL_GAP);
    assert_eq!(l.rheader.x, rx, "右列起点 = 左列 + 列宽 + 列距");
    assert_eq!(l.rheader.w, col_w, "两列等宽");
    for f in &l.rfields {
        assert_eq!(f.x, rx);
        assert_eq!(f.w, col_w);
    }
    for s in &l.sessions {
        assert_eq!(s.x, rx, "会话行在右列");
        assert_eq!(s.w, col_w);
    }
    // 两列 mini 卡头同高齐顶
    assert_eq!(l.lheader.y, l.rheader.y);
    assert_eq!(l.lfields[0].y, l.rfields[0].y, "两列字段行齐顶");
}

#[test]
fn spec_几何_钮钉左列底() {
    // n=0：左列比右列长（钮占高）→ 钮在字段块一行距之后
    let l0 = link_card::layout(&tmux_card(), 0);
    let fields_end = l0.lfields[3].y + i64::from(l0.lfields[3].h);
    assert_eq!(
        l0.button.y,
        fields_end + i64::from(pp::ROW_GAP),
        "左列最长时钮直接接字段块（一行距）"
    );
    assert_eq!(
        l0.button.y + i64::from(l0.button.h),
        l0.card.y + i64::from(l0.card.h) - i64::from(pp::CARD_PAD_V),
        "钮底 = 卡底 − PAD_V（变异③：不钉底必须咬）"
    );
    // n=6：右列反超 → 钮仍钉列底，不许吊在字段后悬空
    let l6 = link_card::layout(&tmux_card(), 6);
    assert!(
        l6.rheader.y < l6.button.y,
        "右列长时左列字段与钮之间留空是钉底的设计形态"
    );
    assert_eq!(
        l6.button.y + i64::from(l6.button.h),
        l6.card.y + i64::from(l6.card.h) - i64::from(pp::CARD_PAD_V),
        "右列再长钮也钉列底"
    );
    // 会话行不许出卡底
    let last = l6.sessions.last().unwrap();
    assert!(
        last.y + i64::from(last.h) <= l6.card.y + i64::from(l6.card.h) - i64::from(pp::CARD_PAD_V),
        "末会话行不许出内容区"
    );
}

#[test]
fn spec_几何_卡高账取高列() {
    // n=0：左列长（带钮）；n 够大：右列反超（变异④：取 min 必须咬）
    assert_eq!(link_card::card_h(0), pp::CARD_PAD_V * 2 + link_card::LEFT_H);
    let n = 6;
    assert_eq!(
        link_card::card_h(n),
        pp::CARD_PAD_V * 2 + link_card::right_h(n),
        "右列反超时卡高跟右列"
    );
    assert!(
        link_card::right_h(n) > link_card::LEFT_H,
        "本组参数右列必须真反超"
    );
    // 单调：会话多一行卡高长一段
    assert!(link_card::card_h(3) > link_card::card_h(2));
}

#[test]
fn spec_预留量_与实高同源() {
    for n in [0, 1, 3, 6] {
        assert_eq!(
            link_card::inset_extra(n),
            link_card::LINK_GAP + link_card::card_h(n),
            "预留量 = 间距 + 卡实高（变异①：漏 LINK_GAP 必须咬）"
        );
    }
}

#[test]
fn spec_命中_只有重连可点() {
    let l = link_card::layout(&tmux_card(), 2);
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
    // 字段行/卡头/会话行/列间隙 = 纯展示不可点
    let f = &l.lfields[0];
    assert_eq!(link_card::hit(&l, f.x + 1, f.y + 1), None);
    assert_eq!(link_card::hit(&l, l.rheader.x + 1, l.rheader.y + 1), None);
    let s = &l.sessions[0];
    assert_eq!(link_card::hit(&l, s.x + 1, s.y + 1), None);
}
