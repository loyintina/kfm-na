//! scroll_spec.rs — 触摸滚动手势状态机考题（A 档纯逻辑，答案 src/scroll.rs）
//!
//! 契约（手机终端的自然手感）：
//! - 位移 < TAP_SLOP_PX 松开 = 点按（唤软键盘），期间一行都不许滚
//! - 越过阈值进入滚动：手指向下拖 = 看更老的历史 = 行数为正
//! - 像素→行换算带余数挂账：半行半行慢拖必须累计成行（取整吞余数则慢滚哑）
//! - 拖下去再拖回来：行数可逆（净位移为零 → 净滚动为零）

use kfm_na::scroll::{TAP_SLOP_PX, TouchScroll};

const CELL: f64 = 30.0; // 格高 30px（与真机尖刺常量同量级）

#[test]
fn spec_轻点是点按不是滚动() {
    // 全程位移都在阈值内：不许出一行滚动，松手算点按
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(t.moved(505.0), 0);
    assert_eq!(t.moved(500.0 + TAP_SLOP_PX - 1.0), 0);
    assert_eq!(t.moved(490.0), 0);
    assert!(t.was_tap(), "没过阈值必须是点按");
}

#[test]
fn spec_越阈拖动出滚动不算点按() {
    // 一口气拖过阈值：进入滚动模式，松手不许当点按（不弹键盘）
    let mut t = TouchScroll::new(500.0, CELL);
    t.moved(500.0 + TAP_SLOP_PX + 1.0);
    assert!(!t.was_tap(), "越过阈值后松手不许当点按");
}

#[test]
fn spec_方向_向下拖看历史() {
    // 自然滚动：手指向下（y 增大）= 看更老的输出 = 行数为正
    // （alacritty Scroll::Delta 正数 = display_offset 增大）
    let mut t = TouchScroll::new(500.0, CELL);
    let lines = t.moved(500.0 + CELL * 3.0);
    assert_eq!(lines, 3, "向下拖三格高必须是 +3 行");
    let mut u = TouchScroll::new(500.0, CELL);
    let lines = u.moved(500.0 - CELL * 2.0);
    assert_eq!(lines, -2, "向上拖两格高必须是 -2 行");
}

#[test]
fn spec_余数挂账_慢拖累计成行() {
    // 病灶候选：每次 moved 各自取整会吞掉半行零头，慢速滚动永远出不了行。
    // 契约：三次 0.5 行（15px）的下拖 = 1 行 + 余数继续挂
    let mut t = TouchScroll::new(500.0, CELL);
    let half = CELL / 2.0; // 15px < TAP_SLOP？不——阈值内不滚！先一把越阈
    // 先越阈进入滚动模式（越阈那一下的位移也计入）
    let l0 = t.moved(500.0 + TAP_SLOP_PX + 1.0); // 25px → 0 行（余 25px）
    assert_eq!(l0, 0, "25px 不足一格高，0 行（余数挂账 25px）");
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + 1.0 + half),
        1,
        "再拖 15px：25+15=40px ≥ 30px → 1 行"
    );
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + 1.0 + half + half),
        0,
        "再 15px：余 10+15=25px 不足 → 0 行"
    );
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + 1.0 + half + half + half),
        1,
        "再 15px：25+15=40 → 1 行"
    );
}

#[test]
fn spec_拖下去再拖回来净滚动为零() {
    // 下拉 3 行再上拉 3 行：净位移 0，累计滚动也必须回到 0（余数符号不漂）
    let mut t = TouchScroll::new(500.0, CELL);
    let down = t.moved(500.0 + CELL * 3.0);
    let up = t.moved(500.0);
    assert_eq!(down, 3);
    assert_eq!(up, -3, "回原位必须是 -3 行，净滚动归零");
}

#[test]
fn spec_越阈当刻的位移也计入滚动() {
    // 契约细节：越阈那一下不许白吞——25px 越阈位移要挂进余数，
    // 否则「刚越阈就松手再慢拖」的手感会缺一段
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(
        t.moved(500.0 + TAP_SLOP_PX + CELL),
        1,
        "越阈位移必须计入：25+30=55px → 1 行"
    );
}

#[test]
fn spec_滚轮序列_sgr编码() {
    // 鼠标上报模式下滚屏翻成 SGR 1006 滚轮事件发 PTY：
    // 看历史（手指下拖）= wheel up = button 64；看最新 = wheel down = 65。
    // 格式 ESC [ < btn ; col ; row M，坐标 1-based（终端协议惯例）
    use kfm_na::scroll::wheel_seq;
    assert_eq!(wheel_seq(true, 1, 1), "\x1b[<64;1;1M");
    assert_eq!(wheel_seq(false, 1, 1), "\x1b[<65;1;1M");
    assert_eq!(wheel_seq(true, 72, 40), "\x1b[<64;72;40M");
}
