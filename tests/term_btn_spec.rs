//! term_btn_spec.rs — 终端钮控件考题（2026-09-30 BAR-208 阅读页退出口，
//! A 档纯逻辑，答案 src/ui/term_btn.rs）。
//!
//! 契约：①几何单源——hit_rect 同喂命中与涂装，且锚定齿轮 hit_rect
//! （正左 GAP 处同中轴同尺，两钮永不相撞）；②命中盒内必中盒外必不中，
//! 边界咬死；③涂装掩码纪律——chevron 尖/底横着墨，远点与盒外一字
//! 不动（页面内容透出）；④小缓冲早退不 panic；⑤paint ≡ paint_at(心)。
//! 变异抽检：hit_rect 私算不锚齿轮 → 题①红；掩码删底横段 → 题③红；
//! paint 私算心位 → 题⑤红。

use kfm_na::ui::term_btn::{
    TERM_BTN_GAP_PX, TERM_BTN_GLYPH_PX, TERM_BTN_HIT_PX, TERM_BTN_INK, hit, hit_rect, paint,
    paint_at,
};

const W: u32 = 800;
const H: u32 = 600;
const BG: u32 = 0x000D_0F13; // TERM_CARD_BG

#[test]
fn spec_bar208_term_btn_几何锚定齿轮() {
    let (hx, hy, hw, hh) = hit_rect(W);
    let (ghx, ghy, ghw, ghh) = kfm_na::ui::gear::hit_rect(W);
    // 命中盒见方同尺、同中轴、正左 GAP 净距——两钮永不重叠
    assert_eq!((hw, hh), (TERM_BTN_HIT_PX, TERM_BTN_HIT_PX));
    assert_eq!(hy, ghy, "与齿轮同中轴（y 一致）");
    assert_eq!(hh, ghh, "与齿轮同尺");
    assert_eq!(
        hx + hw + TERM_BTN_GAP_PX,
        ghx,
        "终端钮右缘 + GAP 必须正好抵齿轮左缘（锚定单源，私算即红）"
    );
    assert!(hx + hw <= ghx, "两钮命中盒不许重叠");
    let _ = ghw;
}

#[test]
fn spec_bar208_term_btn_命中边界咬死() {
    let (hx, hy, hw, hh) = hit_rect(W);
    let (cx, cy) = (hx + hw / 2, hy + hh / 2);
    assert!(hit(f64::from(cx), f64::from(cy), W), "盒心必中");
    assert!(!hit(f64::from(hx) - 1.0, f64::from(cy), W), "盒左外不中");
    assert!(!hit(f64::from(cx), f64::from(hy) - 1.0, W), "盒上外不中");
    assert!(hit(f64::from(hx + hw) - 0.5, f64::from(cy), W));
    assert!(!hit(f64::from(hx + hw) + 0.5, f64::from(cy), W));
    // 齿轮盒心不许落进终端钮（阅读页顶两钮分流的前提）
    let (ghx, ghy, ghw, ghh) = kfm_na::ui::gear::hit_rect(W);
    assert!(
        !hit(f64::from(ghx + ghw / 2), f64::from(ghy + ghh / 2), W),
        "齿轮心不许中终端钮"
    );
}

#[test]
fn spec_bar208_term_btn_涂装掩码纪律() {
    let mut buf = vec![BG; (W * H) as usize];
    paint(&mut buf, W, H);
    let (hx, hy, hw, _) = hit_rect(W);
    let (cx, cy) = (hx + hw / 2, hy + hw / 2);
    let at = |x: u32, y: u32| buf[(y * W + x) as usize];
    // ① chevron 尖着墨：盒归一 (0.46, 0.50) ≈ 心左 3px 同心高
    assert_ne!(at(cx - 3, cy), BG, "chevron 尖必须着墨");
    // ② 底横着墨：盒归一 (0.71, 0.82) ≈ 心右 15px 下心 23px
    assert_ne!(at(cx + 15, cy + 23), BG, "底横必须着墨");
    // ③ 远点全透：盒归一 (0.80, 0.20) 远离三线段
    assert_eq!(at(cx + 22, cy - 22), BG, "远点必须透出底色");
    // ④ 字形盒外一字不动
    let gx0 = cx - TERM_BTN_GLYPH_PX / 2;
    assert_eq!(at(gx0 - 4, cy), BG, "盒外不许着墨");
    // ⑤ 着墨方向朝 TERM_BTN_INK 混合
    let p = at(cx - 3, cy);
    let (ink_b, bg_b, p_b) = (TERM_BTN_INK & 0xFF, BG & 0xFF, p & 0xFF);
    assert!(
        p_b > bg_b && p_b < ink_b + 20,
        "chevron 色应介于底({bg_b})与墨({ink_b})之间，得 {p_b}"
    );
}

#[test]
fn spec_bar208_term_btn_小缓冲早退() {
    let mut tiny = vec![0u32; 16];
    paint(&mut tiny, 4, 4);
    assert!(tiny.iter().all(|&p| p == 0), "小缓冲早退一字不写");
}

#[test]
fn spec_bar208_term_btn_paint与paint_at同一掩码() {
    let mut a = vec![BG; (W * H) as usize];
    let mut b = vec![BG; (W * H) as usize];
    paint(&mut a, W, H);
    let (hx, hy, hw, hh) = hit_rect(W);
    paint_at(&mut b, W, H, hx + hw / 2, hy + hh / 2);
    assert_eq!(a, b, "paint ≡ paint_at(命中盒心)——掩码唯一来源");
    let mut c = vec![BG; (W * H) as usize];
    paint_at(&mut c, W, H, W / 2, H / 2);
    assert_ne!(a, c, "异心位必须画出不同结果");
}
