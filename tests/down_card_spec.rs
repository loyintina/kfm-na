//! 断线状态卡考题（A 档）：几何单源 + 命中语义——涂装与命中同读
//! ui::down_card，这里钉死宪法尺寸与命中归属。
//!
//! 变异抽检：①hit 的 Retry/Local 归属写反必须咬；②card_rect 漏了
//! 最小宽闸（窄屏画半张卡）必须咬。

use kfm_na::termview::{CELL_H, CELL_W, MARGIN_X, MARGIN_Y};
use kfm_na::ui::down_card::{
    BTN_GAP, BTN_W, CARD_H, CARD_TOP_OFF, DownHit, ROW_H, btn_rects, card_rect, hit, row_rect,
};

const W: u32 = 1080; // 典型手机屏宽

#[test]
fn spec_断线卡_宪法尺寸() {
    // 卡带 4 格高（内行 3 格 + 上下各半格净空）
    assert_eq!(CARD_H, CELL_H * 4);
    assert_eq!(ROW_H, CELL_H * 3);
    // 钮 6 格宽 3 格高、钮距 1 格
    assert_eq!(BTN_W, CELL_W * 6);
    assert_eq!(BTN_GAP, CELL_W);
    // 卡顶 = 齿轮带（6+72）下再留 12 缝
    assert_eq!(CARD_TOP_OFF, 6 + 72 + 12);
}

#[test]
fn spec_断线卡_几何单源() {
    let (cx, cy, cw, ch) = card_rect(W).expect("1080 宽必须出卡");
    assert_eq!(cx, i64::from(MARGIN_X + CELL_W), "左右各让内容缘 1 格");
    assert_eq!(cy, i64::from(MARGIN_Y + CARD_TOP_OFF));
    assert_eq!(cw, W - 2 * (MARGIN_X + CELL_W));
    assert_eq!(ch, CARD_H);
    // 内行垂直居中：上下净空相等（半格）
    let (_, ry, _, rh) = row_rect(W).unwrap();
    assert_eq!(rh, ROW_H);
    assert_eq!(
        ry - cy,
        cy + i64::from(ch) - (ry + i64::from(rh)),
        "上下净空同尺"
    );
    // 双钮右簇：切本地右缘 = 卡内右缘 - 1 格内垫；重试左邻间隔 1 格
    let (retry, local) = btn_rects(W).unwrap();
    assert_eq!(
        local.0 + i64::from(local.2),
        cx + i64::from(cw) - 3 - i64::from(CELL_W)
    );
    assert_eq!(retry.0 + i64::from(retry.2) + i64::from(BTN_GAP), local.0);
    assert_eq!(retry.1, ry);
    assert_eq!(local.1, ry, "双钮与文本同行带");
}

#[test]
fn spec_断线卡_窄屏不画() {
    // 两钮+钮距+6 格文本区摆不下的宽度 = None（不画不命中，不挤烂）
    let min = 2 * (MARGIN_X + CELL_W) + BTN_W * 2 + BTN_GAP + CELL_W * 6;
    assert!(card_rect(min - 1).is_none());
    assert!(card_rect(min).is_some());
}

#[test]
fn spec_断线卡_命中归属() {
    let (retry, local) = btn_rects(W).unwrap();
    // 钮心命中各归各
    let rc = (
        retry.0 as f64 + retry.2 as f64 / 2.0,
        retry.1 as f64 + retry.3 as f64 / 2.0,
    );
    let lc = (
        local.0 as f64 + local.2 as f64 / 2.0,
        local.1 as f64 + local.3 as f64 / 2.0,
    );
    assert_eq!(hit(rc.0, rc.1, W), Some(DownHit::Retry));
    assert_eq!(hit(lc.0, lc.1, W), Some(DownHit::Local));
    // 卡带文本区不吞触摸（穿透给终端手势——点信息行没语义就别拦路）
    let (cx, _, _, _) = card_rect(W).unwrap();
    let mid = (f64::from(MARGIN_X) + 40.0, retry.1 as f64 + 10.0);
    assert!(mid.0 > cx as f64);
    assert_eq!(hit(mid.0, mid.1, W), None);
    // 卡外（卡顶之上/卡带之下）不命中
    assert_eq!(hit(lc.0, 5.0, W), None);
    assert_eq!(hit(lc.0, local.1 as f64 + local.3 as f64 + 5.0, W), None);
}

#[test]
fn spec_bar135_状态行_三态文本() {
    // 优先级：重连在途 > 有暂存 > 裸断开
    let t = kfm_na::ui::down_card::status_text;
    assert!(
        t(true, 0).contains("重连中"),
        "connecting 相: {}",
        t(true, 0)
    );
    assert!(
        t(true, 42).contains("重连中"),
        "connecting 优先于暂存: {}",
        t(true, 42)
    );
    let held = t(false, 42);
    assert!(held.contains("42"), "暂存字节数必须显形: {held}");
    assert!(held.contains("暂存"), "暂存相: {held}");
    assert_eq!(t(false, 0), "连接已断开");
}
