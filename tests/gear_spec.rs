//! gear_spec.rs — 设置钮控件考题（2026-09-12 配置池卡按钮入口，A 档纯逻辑，
//! 答案 src/ui/gear.rs）。
//!
//! 契约：①几何单源——hit_rect 同喂命中与涂装（改一处另一处自动跟）；
//! ②命中盒贴终卡内右上且比字形大（触控补偿）；③涂装掩码纪律——环带/
//! 齿带着墨，中孔与盒外一字不动（终端文字透出）；④小缓冲早退不 panic。
//! 变异抽检：hit 与 paint 各自私算中心 → 题①必红；掩码中孔删了 → 题③红。

use kfm_na::ui::gear::{GEAR_GLYPH_PX, GEAR_HIT_PX, GEAR_INK, hit, hit_rect, paint};

const W: u32 = 800;
const H: u32 = 600;
const BG: u32 = 0x000D_0F13; // TERM_CARD_BG

#[test]
fn spec_gear_几何单源与命中() {
    let (hx, hy, hw, hh) = hit_rect(W);
    // 命中盒 = GEAR_HIT_PX 见方，字形盒同心
    assert_eq!((hw, hh), (GEAR_HIT_PX, GEAR_HIT_PX));
    // 贴右上：右缘距卡内右缘（W - MARGIN_X）不超过触控补偿半径
    let card_inner_r = W - kfm_na::termview::MARGIN_X;
    assert!(
        hx + hw >= card_inner_r,
        "命中盒右缘必须够到卡内右缘（现 {} < {})",
        hx + hw,
        card_inner_r
    );
    // 命中：盒心必中
    let (cx, cy) = (hx + hw / 2, hy + hh / 2);
    assert!(hit(f64::from(cx), f64::from(cy), W), "盒心必中");
    // 盒外不中：左 1px、上 1px、屏右缘外
    assert!(!hit(f64::from(hx) - 1.0, f64::from(cy), W), "盒左外不中");
    assert!(!hit(f64::from(cx), f64::from(hy) - 1.0, W), "盒上外不中");
    // 边界咬死：盒右缘最后一列中、再右一列不中
    assert!(hit(f64::from(hx + hw) - 0.5, f64::from(cy), W));
    assert!(!hit(f64::from(hx + hw) + 0.5, f64::from(cy), W));
}

#[test]
fn spec_gear_涂装掩码纪律() {
    let mut buf = vec![BG; (W * H) as usize];
    paint(&mut buf, W, H);
    let (hx, hy, hw, _) = hit_rect(W);
    let (cx, cy) = (hx + hw / 2, hy + hw / 2);
    let at = |x: u32, y: u32| buf[(y * W + x) as usize];
    // ① 环带着墨：环带中点半径点（rn≈0.5 处环连续，与角域无关）——
    //    向墨色 GEAR_INK 方向混合（必变且不等于原底）
    let r_out = f64::from(GEAR_GLYPH_PX) / 2.0 - 1.0;
    let ring_x = cx + (r_out * 0.5) as u32;
    assert_ne!(at(ring_x, cy), BG, "环带必须着墨");
    // ② 齿尖着墨：正上方 0.85 半径处（0° 是齿区中心）
    let tooth_y = cy - (r_out * 0.85) as u32;
    assert_ne!(at(cx, tooth_y), BG, "齿尖必须着墨");
    // ③ 中孔全透：圆心一字不动
    assert_eq!(at(cx, cy), BG, "中孔必须透出底色");
    // ④ 字形盒外一字不动：盒左外 4px 同行
    let gx0 = cx - GEAR_GLYPH_PX / 2;
    assert_eq!(at(gx0 - 4, cy), BG, "盒外不许着墨");
    // ⑤ 着墨方向是朝 GEAR_INK 混合（不是乱色）：环带像素应介于底与墨之间
    let p = at(ring_x, cy);
    let ink_b = GEAR_INK & 0xFF;
    let bg_b = BG & 0xFF;
    let p_b = p & 0xFF;
    assert!(
        p_b > bg_b && p_b < ink_b + 20,
        "环带色应介于底({bg_b})与墨({ink_b})之间，得 {p_b}"
    );
}

#[test]
fn spec_gear_小缓冲早退() {
    // 比字形盒还小的缓冲：不 panic 不写越界（直接全零缓冲走一遍）
    let mut tiny = vec![0u32; 16];
    paint(&mut tiny, 4, 4);
    assert!(tiny.iter().all(|&p| p == 0), "小缓冲早退一字不写");
}
