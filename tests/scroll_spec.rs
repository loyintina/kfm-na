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
    // 边界钉（2026-08-27 变异抽检存活体：恰好到阈值=仍算轻点，
    // AOSP touchSlop 含边惯例——此前 < vs <= 无人判卷）
    assert_eq!(t.moved(500.0 + TAP_SLOP_PX), 0, "恰好到位仍是点按期");
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

// ---------- 像素级滚动（2026-09-24 用户拍板「下面的方向就是滚动的像素级」） ----------
// 契约：moved_px 与 moved 同一只 slop 门，但不取整不挂账——位移原样
// 出 px（带符号）。旧行级通道 moved 一行不动（降级保底，设置里可切回）

#[test]
fn spec_像素滚动_moved_px零头原样不取整() {
    let mut t = TouchScroll::new(500.0, CELL);
    // slop 门同尺：阈值内零位移
    assert_eq!(t.moved_px(505.0), 0.0);
    assert_eq!(t.moved_px(500.0 + TAP_SLOP_PX), 0.0, "恰好到位仍是点按期");
    assert!(t.was_tap());
    // 越阈后：位移原样出——半行零头不取整不挂账（moved 在这里出 0 行）
    let d = t.moved_px(500.0 + TAP_SLOP_PX + 15.5);
    assert_eq!(d, 15.5, "越阈后第一笔位移原样出（slop 段不计入，同 moved）");
    assert!(!t.was_tap());
    // 连续小步：每次出当次位移，无累计无吞零
    assert_eq!(t.moved_px(500.0 + TAP_SLOP_PX + 15.5 + 3.25), 3.25);
    // 反向：负位移原样出
    assert_eq!(t.moved_px(500.0), -(TAP_SLOP_PX + 18.75));
}

#[test]
fn spec_像素滚动_双通道互不污染() {
    // 同一只状态机的两条通道：slop/last_y 共享——行级保底与像素级
    // 切换发生在两次触摸之间（设置页里点开关），一次触摸内不换道
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(t.moved(500.0 + TAP_SLOP_PX + CELL), 1, "行级通道照旧");
    let mut u = TouchScroll::new(500.0, CELL);
    let d = u.moved_px(500.0 + TAP_SLOP_PX + CELL);
    assert_eq!(d, CELL, "像素通道同位移出原样 px");
}

// ---------- BAR-151 滚轮路余数挂账（2026-09-24 仪器定罪：tmux 滚动 ticks 恒 0） ----------
// 病灶：像素通道滚轮换算逐事件 trunc——慢拖每笔位移 <cell_h 恒 0 tick，
// 余数全吞（真机实录 d=16/20/20 ticks=0，tmux 滚动整只哑掉）。
// 契约：滚轮 tick 换算必须带余数挂账——与行级 moved 同一把尺：
// 半行半行慢拖必须累计成 tick；往返净位移为零 → 净 tick 为零。

#[test]
fn spec_bar151_滚轮挂账_慢拖累计成tick() {
    let mut t = TouchScroll::new(500.0, CELL);
    // 越阈：30px 累计位移（slop 24 不计入，第一笔有效位移 6px）
    assert_eq!(t.wheel_ticks(530.0, CELL), 0, "6px 不足一行不许出 tick");
    // 每笔 +10px：6→16→26→36→46——第 5 笔过一行才许出 1 tick
    assert_eq!(t.wheel_ticks(540.0, CELL), 0);
    assert_eq!(t.wheel_ticks(550.0, CELL), 0);
    assert_eq!(t.wheel_ticks(560.0, CELL), 1, "累计 36px 过一行出 1 tick");
    assert_eq!(t.wheel_ticks(570.0, CELL), 0);
    assert_eq!(t.wheel_pending(), 16.0, "零头挂账不吞（6+10×4−30=16）");
}

#[test]
fn spec_bar151_滚轮挂账_往返净tick为零() {
    let mut t = TouchScroll::new(500.0, CELL);
    let mut net = 0;
    // 下去 100px（有效 76px = 2 tick + 16px 零头）
    for y in [530.0, 550.0, 570.0, 590.0, 600.0] {
        net += t.wheel_ticks(y, CELL);
    }
    // 回拖 106px（= 下去的有效 76px + 30px，净计 −30px = 恰好 −1 tick）：
    // 挂账借还必须对称——吞零头/造 tick 都会让净账或零头对不上
    for y in [570.0, 550.0, 530.0, 510.0, 494.0] {
        net += t.wheel_ticks(y, CELL);
    }
    assert_eq!(net, -1, "净位移 −30px 必须恰好 −1 tick（挂账造假即破）");
    assert_eq!(t.wheel_pending().abs(), 0.0, "净 −30px 后零头必须归零");
}

#[test]
fn spec_bar151_滚轮挂账_slop段不计入() {
    // 点按嫌疑期（slop 内）的位移一滴都不许进滚轮挂账——
    // 否则轻点带微抖也攒零头，下一次真拖起步就白送 tick
    let mut t = TouchScroll::new(500.0, CELL);
    assert_eq!(t.wheel_ticks(510.0, CELL), 0);
    assert_eq!(t.wheel_ticks(520.0, CELL), 0);
    assert_eq!(t.wheel_pending(), 0.0, "slop 段位移不许进挂账");
    assert!(t.was_tap(), "全程没过阈值仍是点按");
}
