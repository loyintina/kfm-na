//! parser_page_spec.rs — 解析页 tmux 插件核考题（A 档：几何/命中/状态机）
//!
//! 判卷维度：
//! - layout：卡片在池区内、动态高 = 内容定、上限截断可见框；**一行两框**
//!   （2026-09-19 用户拍板：框 = 三级框行主形态双色渐变，内容只有
//!   名字+×，meta「·N窗/·他端」撤）；常态按钮带 = [重排][新窗]
//!   （↻ 刷新撤——列会话开页/操作后自动刷，手动冗余）；命名态
//!   [确定][取消]；确认态走跳框模态（卡区无按钮无确认带）
//! - 跳框几何：confirm_card 居中、双钮在卡内不重叠、高 2 格框行纪律
//! - hit：框命中（本体=Session / 框尾 × 带=Kill，两列都要）、按钮命中、
//!   卡外=None；确认态只认跳框（钮/卡内吞/卡外 Dismiss，卡区全屏蔽）；
//!   命中与涂装吃同一份 Layout（眼手同尺的本体）
//! - 状态机：set_sessions 收确认态防下标悬空；epoch 凡变更必 +1
//!   （涂装 sig 靠它——漏 bump = 鬼影）；命名/确认模式流转
//!
//! 变异抽检方向：两列框宽忘减 COL_GAP（右列出卡）、hit 框尾 × 带判据
//! 改 >（少 1px）、跳框卡外命中退化为 None（点外取消死）、
//! set_sessions 忘收 confirming——本文件必须红。

use kfm_na::tmux_ctl::TmuxSession;
use kfm_na::ui::parser_page::{
    self, Action, Hit, Mode, ParserPage, Status, button_action, button_labels,
};

const W: u32 = 1080;
const H: u32 = 2280;
const INSET: u32 = 0;

fn ss(names: &[&str]) -> Vec<TmuxSession> {
    names
        .iter()
        .map(|n| TmuxSession {
            name: (*n).into(),
            windows: 1,
            attached: false,
        })
        .collect()
}

// ---- layout ----

#[test]
fn spec_layout_卡片在池区内() {
    let l = parser_page::layout(W, H, INSET, 3, Mode::Normal);
    let area = kfm_na::ui::dual_pool::pool_area(W, H, INSET);
    // 页标题撤后卡区上吞 TAB_ROW_H（2026-09-19 用户拍板）：卡顶 = 池区顶
    // 减一行标签高，卡高上限同步放大
    let tab_h = kfm_na::ui::tab_bar::TAB_ROW_H;
    assert_eq!(l.card.x, area.x);
    assert_eq!(l.card.y, area.y - i64::from(tab_h));
    assert!(l.card.w <= area.w);
    assert!(l.card.h <= area.h + tab_h);
    assert_eq!(l.rows.len(), 3);
    assert_eq!(l.buttons.len(), 2); // 常态 [重排][新窗]
}

#[test]
fn spec_layout_两列框行() {
    // 4 会话 = 两行两列；框高 = 2 格（宪法三级框行最小高）；框宽 =
    // (内容宽 − 列距) / 2；右列左缘 = 左列右缘 + COL_GAP；框都在卡内
    let l = parser_page::layout(W, H, INSET, 4, Mode::Normal);
    assert_eq!(l.rows.len(), 4);
    let stride = (parser_page::ROW_H + parser_page::ROW_GAP) as i64;
    let b0 = &l.rows[0];
    let b1 = &l.rows[1];
    let b2 = &l.rows[2];
    assert_eq!(b0.h, parser_page::ROW_H, "框高必须 2 格");
    // 绝对框宽钉（相对位钉不住「忘减列距」变异——COL_GAP==CARD_PAD_H
    // 时右缘恰好贴卡缘蒙混过关，变异抽检实录）
    let cw = l.card.w - parser_page::CARD_PAD_H * 2;
    assert_eq!(
        b0.w,
        (cw - parser_page::COL_GAP) / 2,
        "框宽 = (内容宽−列距)/2"
    );
    assert_eq!(b0.y, b1.y, "同行两框同 y");
    assert_eq!(b1.x, b0.x + b0.w as i64 + parser_page::COL_GAP as i64);
    assert_eq!(b2.y, b0.y + stride, "第二行框 y = 首行 + stride");
    assert_eq!(b2.x, b0.x, "第二行左列与首行左列对齐");
    for r in &l.rows {
        assert!(r.x >= l.card.x, "框左出卡");
        assert!(r.x + r.w as i64 <= l.card.x + l.card.w as i64, "框右出卡");
        assert!(r.y >= l.card.y && r.y + r.h as i64 <= l.card.y + l.card.h as i64);
    }
    // 奇数会话：末行只有左列一框
    let l3 = parser_page::layout(W, H, INSET, 3, Mode::Normal);
    assert_eq!(l3.rows.len(), 3);
    assert_eq!(l3.rows[2].x, l3.rows[0].x);
}

#[test]
fn spec_layout_动态高随内容长() {
    let l2 = parser_page::layout(W, H, INSET, 2, Mode::Normal);
    let l5 = parser_page::layout(W, H, INSET, 5, Mode::Normal);
    assert!(l5.card.h > l2.card.h, "5 框卡必须比 2 框卡高");
}

#[test]
fn spec_layout_模式按钮带() {
    assert_eq!(button_labels(Mode::Normal), ["重排", "新窗"]);
    assert_eq!(button_labels(Mode::Naming), ["确定", "取消"]);
    assert!(button_labels(Mode::Confirming).is_empty()); // 确认走跳框，卡区无钮
    let ln = parser_page::layout(W, H, INSET, 1, Mode::Naming);
    assert!(ln.naming.is_some());
    assert_eq!(ln.buttons.len(), 2);
    // 确认态 = 卡按 Normal 几何（跳框模态不占卡高）
    let lc = parser_page::layout(W, H, INSET, 1, Mode::Confirming);
    let lnor = parser_page::layout(W, H, INSET, 1, Mode::Normal);
    assert!(lc.naming.is_none());
    assert!(lc.buttons.is_empty());
    assert_eq!(lc.card.h, lnor.card.h, "确认态卡高 = 常态（跳框不占卡高）");
}

#[test]
fn spec_layout_跳框几何() {
    let card = parser_page::confirm_card(W, H);
    // 居中
    assert_eq!(card.x, (W as i64 - card.w as i64) / 2);
    assert_eq!(card.y, (H as i64 - card.h as i64) / 2);
    let btns = parser_page::confirm_buttons(&card);
    // 双钮：在卡内、同 y、不重叠、高 3 格
    assert_eq!(btns[0].h, parser_page::BTN_H);
    assert_eq!(btns[0].y, btns[1].y);
    assert!(btns[1].x >= btns[0].x + btns[0].w as i64, "双钮重叠");
    for b in &btns {
        assert!(b.x >= card.x && b.x + b.w as i64 <= card.x + card.w as i64);
        assert!(b.y >= card.y && b.y + b.h as i64 <= card.y + card.h as i64);
    }
    // 小屏兜底：卡宽钳制不出屏
    let small = parser_page::confirm_card(400, 800);
    assert!(small.x >= 0 && small.x + small.w as i64 <= 400);
}

#[test]
fn spec_layout_按钮互不重叠且在卡内() {
    for mode in [Mode::Normal, Mode::Naming] {
        let l = parser_page::layout(W, H, INSET, 2, mode);
        for (i, b) in l.buttons.iter().enumerate() {
            assert!(b.x >= l.card.x, "{mode:?} 钮{i} 左出卡");
            assert!(
                b.x + b.w as i64 <= l.card.x + l.card.w as i64,
                "{mode:?} 钮{i} 右出卡"
            );
            assert!(
                b.y + b.h as i64 <= l.card.y + l.card.h as i64,
                "{mode:?} 钮{i} 底出卡"
            );
            if i > 0 {
                let prev = &l.buttons[i - 1];
                assert!(b.x >= prev.x + prev.w as i64, "{mode:?} 钮{i} 与前钮重叠");
            }
        }
    }
}

#[test]
fn spec_layout_超池区截断可见框() {
    // 小屏高塞 100 会话：卡高不许越池区（吞标题行后的放大池区），
    // 可见框数截断（一行两框 = 偶数）
    let l = parser_page::layout(600, 900, INSET, 100, Mode::Normal);
    let area = kfm_na::ui::dual_pool::pool_area(600, 900, INSET);
    assert!(l.card.h <= area.h + kfm_na::ui::tab_bar::TAB_ROW_H);
    assert_eq!(l.rows.len(), l.visible_rows);
    assert!(l.visible_rows < 100);
    assert!(l.visible_rows >= 1);
}

// ---- hit ----

#[test]
fn spec_hit_框本体与kill带_两列() {
    let l = parser_page::layout(W, H, INSET, 4, Mode::Normal);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Normal);
    let b0 = &l.rows[0];
    // 框本体中点 = Session(0)
    assert_eq!(
        hit(b0.x + 10, b0.y + b0.h as i64 / 2),
        Some(Hit::Session(0))
    );
    // 框尾 × 带内 = Kill(0)
    let kx = b0.x + b0.w as i64 - parser_page::KILL_W as i64 + 2;
    assert_eq!(hit(kx, b0.y + b0.h as i64 / 2), Some(Hit::Kill(0)));
    // × 带左缘界上 = Kill（边界归属钉死：>= 含左缘）
    assert_eq!(hit(kx - 2, b0.y + b0.h as i64 / 2), Some(Hit::Kill(0)));
    // × 带左缘外 1px 仍是 Session
    assert_eq!(hit(kx - 3, b0.y + b0.h as i64 / 2), Some(Hit::Session(0)));
    // 右列框 = Session(1)，其 × 带 = Kill(1)
    let b1 = &l.rows[1];
    assert_eq!(hit(b1.x + 10, b1.y + 2), Some(Hit::Session(1)));
    let kx1 = b1.x + b1.w as i64 - parser_page::KILL_W as i64 + 2;
    assert_eq!(hit(kx1, b1.y + 2), Some(Hit::Kill(1)));
    // 第二行左列 = Session(2)
    let b2 = &l.rows[2];
    assert_eq!(hit(b2.x + 10, b2.y + 2), Some(Hit::Session(2)));
    // 列隙（两框之间）= None
    assert_eq!(hit(b0.x + b0.w as i64 + 2, b0.y + 2), None);
}

#[test]
fn spec_hit_按钮与卡外() {
    let l = parser_page::layout(W, H, INSET, 2, Mode::Normal);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Normal);
    let b0 = &l.buttons[0];
    assert_eq!(
        hit(b0.x + b0.w as i64 / 2, b0.y + b0.h as i64 / 2),
        Some(Hit::Button(0))
    );
    let b1 = &l.buttons[1];
    assert_eq!(hit(b1.x + 2, b1.y + 2), Some(Hit::Button(1)));
    // 卡外（池区空白 / 屏外）= None
    assert_eq!(hit(0, 0), None);
    assert_eq!(hit(l.card.x + 2, l.card.y + l.card.h as i64 + 200), None);
}

#[test]
fn spec_hit_确认态只认跳框() {
    let l = parser_page::layout(W, H, INSET, 2, Mode::Confirming);
    let hit = |x: i64, y: i64| parser_page::hit(&l, x, y, W, H, Mode::Confirming);
    let card = parser_page::confirm_card(W, H);
    let btns = parser_page::confirm_buttons(&card);
    // 双钮
    assert_eq!(
        hit(btns[0].x + 5, btns[0].y + 5),
        Some(Hit::ModalOk),
        "左钮 = 确定关闭"
    );
    assert_eq!(
        hit(btns[1].x + 5, btns[1].y + 5),
        Some(Hit::ModalCancel),
        "右钮 = 取消"
    );
    // 卡内非钮区 = 吞（None，不许穿透）
    assert_eq!(hit(card.x + 5, card.y + 5), None);
    // 卡外 = Dismiss（点框外取消）；哪怕点在背后的会话框上也一样
    assert_eq!(hit(2, 2), Some(Hit::ModalDismiss));
    let b0 = &l.rows[0];
    assert_eq!(
        hit(b0.x + 10, b0.y + b0.h as i64 / 2),
        Some(Hit::ModalDismiss),
        "模态在时卡区命中必须屏蔽（点在框上也只算框外取消）"
    );
}

#[test]
fn spec_button_action_全模式映射() {
    assert_eq!(button_action(Mode::Normal, 0), Some(Action::Reflow));
    assert_eq!(button_action(Mode::Normal, 1), Some(Action::New));
    assert_eq!(button_action(Mode::Naming, 0), Some(Action::NamingOk));
    assert_eq!(button_action(Mode::Naming, 1), Some(Action::NamingCancel));
    assert_eq!(button_action(Mode::Normal, 2), None); // 刷新钮已撤
    assert_eq!(button_action(Mode::Confirming, 0), None); // 确认走跳框
}

// ---- 状态机 ----

#[test]
fn spec_epoch_凡变更必进位() {
    let mut p = ParserPage::new();
    let e0 = p.epoch();
    p.set_loading();
    assert!(p.epoch() > e0);
    let e1 = p.epoch();
    p.set_sessions(ss(&["a", "b"]));
    assert!(p.epoch() > e1);
    let e2 = p.epoch();
    p.set_error("x".into());
    assert!(p.epoch() > e2);
    let e3 = p.epoch();
    p.begin_naming();
    assert!(p.epoch() > e3);
    let e4 = p.epoch();
    p.naming_push("甲");
    assert!(p.epoch() > e4);
    let e5 = p.epoch();
    p.naming_pop();
    assert!(p.epoch() > e5);
    // 无变更不 bump（set_attached 同值重喂）
    p.set_attached(Some("a".into()));
    let e6 = p.epoch();
    p.set_attached(Some("a".into()));
    assert_eq!(p.epoch(), e6, "同值 attached 重喂不许 bump（sig 鬼影）");
}

#[test]
fn spec_set_sessions_收确认态防下标悬空() {
    let mut p = ParserPage::new();
    p.set_sessions(ss(&["a", "b", "c"]));
    p.begin_confirm(2);
    assert_eq!(p.mode(), Mode::Confirming);
    assert_eq!(p.confirm_target(), Some("c".into()));
    // 行表刷新（可能行数变少）——确认态必须收，不许指到别的会话上
    p.set_sessions(ss(&["a"]));
    assert_eq!(p.mode(), Mode::Normal);
    assert_eq!(p.confirm_target(), None);
}

#[test]
fn spec_命名流转() {
    let mut p = ParserPage::new();
    assert_eq!(p.mode(), Mode::Normal);
    p.begin_naming();
    assert_eq!(p.mode(), Mode::Naming);
    assert!(p.naming_active());
    p.naming_push("work");
    assert_eq!(p.naming_take(), Some("work".into()));
    assert_eq!(p.mode(), Mode::Normal);
    assert!(!p.naming_active());
    // 取消路径
    p.begin_naming();
    p.naming_push("x");
    p.cancel_naming();
    assert_eq!(p.mode(), Mode::Normal);
}

#[test]
fn spec_确认越界不开() {
    let mut p = ParserPage::new();
    p.set_sessions(ss(&["a"]));
    p.begin_confirm(9); // 下标越界 = 不开确认态（杀错会话不可挽回）
    assert_eq!(p.mode(), Mode::Normal);
}

#[test]
fn spec_status_流转() {
    let mut p = ParserPage::new();
    assert_eq!(p.status(), &Status::Idle);
    p.set_loading();
    assert_eq!(p.status(), &Status::Loading);
    p.set_sessions(ss(&[]));
    assert_eq!(p.status(), &Status::Ready);
    p.set_error("超时".into());
    assert_eq!(p.status(), &Status::Error("超时".into()));
}
