//! parser_page_spec.rs — 解析页 tmux 插件核考题（A 档：几何/命中/状态机）
//!
//! 判卷维度：
//! - layout：卡片在池区内、动态高 = 内容定、上限截断可见行；三种模式
//!   的按钮带个数与标签；按钮互不重叠且都在卡内
//! - hit：行命中（本体=Session / 行尾 × 带=Kill）、按钮命中、卡外=None；
//!   命中与涂装吃同一份 Layout（眼手同尺的本体）
//! - 状态机：set_sessions 收确认态防下标悬空；epoch 凡变更必 +1
//!   （涂装 sig 靠它——漏 bump = 鬼影）；命名/确认模式流转
//!
//! 变异抽检方向：hit 行尾 × 带判据改 >（少 1px）、layout 按钮带漏减
//! BTN_GAP（末钮出卡）、set_sessions 忘收 confirming——本文件必须红。

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
    assert_eq!(l.card.x, area.x);
    assert_eq!(l.card.y, area.y);
    assert!(l.card.w <= area.w);
    assert!(l.card.h <= area.h);
    assert_eq!(l.rows.len(), 3);
    assert_eq!(l.buttons.len(), 3); // 常态 [重排][+新窗][↻]
}

#[test]
fn spec_layout_动态高随内容长() {
    let l2 = parser_page::layout(W, H, INSET, 2, Mode::Normal);
    let l5 = parser_page::layout(W, H, INSET, 5, Mode::Normal);
    assert!(l5.card.h > l2.card.h, "5 行卡必须比 2 行卡高");
}

#[test]
fn spec_layout_模式按钮带() {
    assert_eq!(button_labels(Mode::Normal), ["重排", "+新窗", "↻"]);
    assert_eq!(button_labels(Mode::Naming), ["确定", "取消"]);
    assert_eq!(button_labels(Mode::Confirming), ["确定关闭", "取消"]);
    assert_eq!(
        parser_page::layout(W, H, INSET, 1, Mode::Naming)
            .buttons
            .len(),
        2
    );
    let ln = parser_page::layout(W, H, INSET, 1, Mode::Naming);
    assert!(ln.naming.is_some() && ln.confirm.is_none());
    let lc = parser_page::layout(W, H, INSET, 1, Mode::Confirming);
    assert!(lc.confirm.is_some() && lc.naming.is_none());
}

#[test]
fn spec_layout_按钮互不重叠且在卡内() {
    for mode in [Mode::Normal, Mode::Naming, Mode::Confirming] {
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
fn spec_layout_超池区截断可见行() {
    // 小屏高塞 100 行：卡高不许越池区，可见行数截断
    let l = parser_page::layout(600, 900, INSET, 100, Mode::Normal);
    let area = kfm_na::ui::dual_pool::pool_area(600, 900, INSET);
    assert!(l.card.h <= area.h);
    assert_eq!(l.rows.len(), l.visible_rows);
    assert!(l.visible_rows < 100);
    assert!(l.visible_rows >= 1);
}

#[test]
fn spec_layout_行在卡内() {
    let l = parser_page::layout(W, H, INSET, 4, Mode::Normal);
    for r in &l.rows {
        assert!(r.y >= l.card.y && r.y + r.h as i64 <= l.card.y + l.card.h as i64);
    }
}

// ---- hit ----

#[test]
fn spec_hit_行本体与kill带() {
    let l = parser_page::layout(W, H, INSET, 3, Mode::Normal);
    let r0 = &l.rows[0];
    // 行本体中点 = Session(0)
    assert_eq!(
        parser_page::hit(&l, r0.x + 10, r0.y + r0.h as i64 / 2),
        Some(Hit::Session(0))
    );
    // 行尾 × 带内 = Kill(0)
    let kx = r0.x + r0.w as i64 - parser_page::KILL_W as i64 + 2;
    assert_eq!(
        parser_page::hit(&l, kx, r0.y + r0.h as i64 / 2),
        Some(Hit::Kill(0))
    );
    // × 带左缘界上 = Kill（边界归属钉死：>= 含左缘）
    assert_eq!(
        parser_page::hit(&l, kx - 2, r0.y + r0.h as i64 / 2),
        Some(Hit::Kill(0))
    );
    // × 带左缘外 1px 仍是 Session（kx = 左缘+2，左缘外 1px = kx-3）
    assert_eq!(
        parser_page::hit(&l, kx - 3, r0.y + r0.h as i64 / 2),
        Some(Hit::Session(0))
    );
    // 第二行
    let r1 = &l.rows[1];
    assert_eq!(
        parser_page::hit(&l, r1.x + 10, r1.y + 2),
        Some(Hit::Session(1))
    );
}

#[test]
fn spec_hit_按钮与卡外() {
    let l = parser_page::layout(W, H, INSET, 2, Mode::Normal);
    let b0 = &l.buttons[0];
    assert_eq!(
        parser_page::hit(&l, b0.x + b0.w as i64 / 2, b0.y + b0.h as i64 / 2),
        Some(Hit::Button(0))
    );
    let b2 = &l.buttons[2];
    assert_eq!(
        parser_page::hit(&l, b2.x + 2, b2.y + 2),
        Some(Hit::Button(2))
    );
    // 卡外（池区空白 / 屏外）= None
    assert_eq!(parser_page::hit(&l, 0, 0), None);
    assert_eq!(
        parser_page::hit(&l, l.card.x + 2, l.card.y + l.card.h as i64 + 200),
        None
    );
}

#[test]
fn spec_button_action_全模式映射() {
    assert_eq!(button_action(Mode::Normal, 0), Some(Action::Reflow));
    assert_eq!(button_action(Mode::Normal, 1), Some(Action::New));
    assert_eq!(button_action(Mode::Normal, 2), Some(Action::Refresh));
    assert_eq!(button_action(Mode::Naming, 0), Some(Action::NamingOk));
    assert_eq!(button_action(Mode::Naming, 1), Some(Action::NamingCancel));
    assert_eq!(button_action(Mode::Confirming, 0), Some(Action::ConfirmOk));
    assert_eq!(
        button_action(Mode::Confirming, 1),
        Some(Action::ConfirmCancel)
    );
    assert_eq!(button_action(Mode::Normal, 3), None); // 越界 = None
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
