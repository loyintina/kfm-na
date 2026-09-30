//! mail_list_spec.rs — 信件列表页级卡 A 档考题（BAR-212，2026-09-30
//! 用户拍板：页面级卡片不盖底栏 AI 输入栏；条目 = 三级框卡（时间 +
//! 从 → 致 / 标题 / 摘要）；最新信在最下方、视口默认追底；摘要懒加载
//! 窗 = 视口 + 上方两屏）。
//!
//! 判卷维度：卡几何安全带（底缘在输入栏带之上）/ 内容高与滚动上限 /
//! 追底状态机（默认贴底、上滑取消、回底恢复、缩水钳回）/ 底锚条目
//! 几何（最新信贴视口底）/ 可见窗与摘要窗裁决 / 命中（关闭/条目/
//! 卡内吞/卡外收）。
//!
//! 变异抽检：①追底语义反（新信来了视口不动 = offset 悬空）必须咬
//! ——实证留档：首版摘 sync_layout 的 follow 归零臂**未咬住**（不变式
//! 「follow ⇒ offset=0」下该臂本就冗余，语义不变钉正确不咬），换真
//! 语义变异「回底不恢复追底」（scroll_by follow 恒 false）后钉红；
//! ②条目顶锚（最新信贴视口顶 = 拍板语义反）必须咬；③滚动上限忘
//! max(0)（短列表出负窗）必须咬；④摘要窗只盖视口不带上方向（滚上去
//! 才取 = 每滚必等）必须咬；⑤视口带外条目可点（画出带外的半卡误触）
//! 必须咬。五咬全中（cp 备份复原）。

use kfm_na::input_bar;
use kfm_na::mail_feed::MailKey;
use kfm_na::termview::CELL_H;
use kfm_na::ui::mail_list as ml;
use kfm_na::ui::modal;

const W: u32 = 1080;
const H: u32 = 2280;

#[test]
fn spec_bar212_卡几何安全带不盖输入栏() {
    let c = ml::card_rect(W, H);
    // 顶 4 格、底 = 输入栏带 + 2 格（BAR-163 翻案安全带同族）
    assert_eq!(c.y, i64::from(modal::MODAL_MAX_MARGIN_TOP));
    assert_eq!(
        c.y + i64::from(c.h),
        i64::from(H) - i64::from(input_bar::HEIGHT_PX + CELL_H * 2)
    );
    assert_eq!(c.x, i64::from(modal::MODAL_SIDE_MARGIN));
}

#[test]
fn spec_bar212_内容高与滚动上限() {
    assert_eq!(ml::total_h(0), 0);
    assert_eq!(ml::total_h(1), ml::ITEM_H as i64);
    assert_eq!(
        ml::total_h(3),
        ml::ITEM_STRIDE as i64 * 3 - i64::from(ml::ITEM_GAP)
    );
    // 装得下 = 0（不可滚）；超出 = 差值
    assert_eq!(ml::scroll_max(2, ml::total_h(2) + 100), 0);
    assert_eq!(ml::scroll_max(3, 100), ml::total_h(3) - 100);
}

#[test]
fn spec_bar212_追底状态机() {
    let mut v = ml::MailListView::new(MailKey::MainBook);
    // 默认追底贴底
    assert!(v.follow());
    assert_eq!(v.offset_bottom(), 0);
    // 布局写回：100 条目、视口 400px → 上限 = total − 400
    v.sync_layout(100, 400);
    let max = ml::scroll_max(100, 400);
    // 上滑看更旧：dy>0 离底，追底取消
    assert!(v.scroll_by(300));
    assert_eq!(v.offset_bottom(), 300);
    assert!(!v.follow());
    // 非追底态新内容进来（条目变多）视口不动点
    v.sync_layout(101, 400);
    assert_eq!(v.offset_bottom(), 300);
    // 追底态新内容进来恒贴底
    assert!(v.scroll_by(-300));
    assert_eq!(v.offset_bottom(), 0);
    assert!(v.follow());
    v.sync_layout(102, 400);
    assert_eq!(v.offset_bottom(), 0);
    // 钳上限：dy 超 max 收回
    v.scroll_by(i64::MAX / 2);
    assert_eq!(v.offset_bottom(), max + ml::ITEM_STRIDE as i64 * 2);
    // 条目缩水钳回（删信场景）
    v.sync_layout(2, 400);
    assert_eq!(v.offset_bottom(), ml::scroll_max(2, 400));
    // 无变化不 bump（epoch 鬼影纪律）
    let e = v.epoch();
    assert!(!v.scroll_by(0));
    assert_eq!(v.epoch(), e);
}

#[test]
fn spec_bar212_条目底锚几何() {
    let c = ml::card_rect(W, H);
    let (vp_top, vp_bot) = ml::viewport_of(&c);
    let n = 10;
    // 追底（offset=0）：末条（最新）卡底贴视口底
    let last = ml::item_rect(&c, n, n - 1, 0);
    assert_eq!(last.y + i64::from(last.h), vp_bot);
    // 首条在视口上方远处（内容长于视口时）
    let first = ml::item_rect(&c, n, 0, 0);
    assert!(first.y < vp_top);
    assert_eq!(first.h, ml::ITEM_H);
    // 相邻条目 stride
    let second = ml::item_rect(&c, n, n - 2, 0);
    assert_eq!(last.y - second.y, ml::ITEM_STRIDE as i64);
}

#[test]
fn spec_bar212_可见窗与摘要窗() {
    let c = ml::card_rect(W, H);
    let (_, vp_bot) = ml::viewport_of(&c);
    let vp_h = vp_bot - ml::viewport_of(&c).0;
    let n = 200;
    // 追底态可见窗含末条
    let vr = ml::visible_range(n, vp_h, 0);
    assert_eq!(vr.end, n);
    assert!(vr.len() >= (vp_h / ml::ITEM_STRIDE as i64) as usize);
    // 摘要窗 = 可见窗上沿再上扩两屏（下沿同视口底）
    let sw = ml::summary_window(n, vp_h, 0);
    assert_eq!(sw.end, n);
    assert!(sw.start <= vr.start);
    assert!(vr.start - sw.start >= (vp_h / ml::ITEM_STRIDE as i64) as usize);
    // 滚到顶部：窗上沿钳 0 不出负
    let max = ml::scroll_max(n, vp_h);
    let sw_top = ml::summary_window(n, vp_h, max);
    assert_eq!(sw_top.start, 0);
    // 空表/零视口 = 空窗（不炸）
    assert_eq!(ml::visible_range(0, vp_h, 0), 0..0);
    assert_eq!(ml::summary_window(n, 0, 0), 0..0);
}

#[test]
fn spec_bar212_命中分区() {
    let c = ml::card_rect(W, H);
    let (vp_top, _) = ml::viewport_of(&c);
    let n = 50;
    // 关闭钮
    let cb = ml::close_btn_rect(&c);
    assert_eq!(
        ml::hit(&c, n, 0, cb.x + 5, cb.y + 5),
        ml::MailListHit::Close
    );
    // 末条（追底贴底那条）
    let last = ml::item_rect(&c, n, n - 1, 0);
    assert_eq!(
        ml::hit(&c, n, 0, last.x + 5, last.y + 5),
        ml::MailListHit::Item(n - 1)
    );
    // 视口带外画出的条目不許点：把 0 条滚到「顶沿在视口带上沿上方
    // 10px」的半卡位，点它露出在标题带的那截 = Card（带闸拦截）
    let vp_h = ml::viewport_of(&c).1 - vp_top;
    let off = ml::total_h(n) - vp_h - 10;
    let out = ml::item_rect(&c, n, 0, off);
    assert_eq!(out.y, vp_top - 10);
    assert_eq!(
        ml::hit(&c, n, off, out.x + 5, vp_top - 5),
        ml::MailListHit::Card
    );
    // 同一条露出在视口带内的下沿可点
    assert_eq!(
        ml::hit(&c, n, off, out.x + 5, vp_top + 2),
        ml::MailListHit::Item(0)
    );
    // 卡外 = 收起
    assert_eq!(ml::hit(&c, n, 0, 2, 2), ml::MailListHit::Outside);
}
