//! demo_icon_spec.rs — 打样 demo 钮控件考题（A 档纯逻辑，答案
//! src/ui/demo_icon.rs；模板 = tests/gear_spec.rs）。
//!
//! 契约：①几何单源——hit_rect 同喂命中与涂装，位置 = 齿轮命中盒正下方
//! 0.5 格、同中轴，尺寸同族缩小；②命中盒判定咬边；③涂装掩码纪律——
//! 烧瓶轮廓（圆底环/细颈线/肩线）着 accent 渐变墨，圆底内腔与盒外
//! 一字不动；④小缓冲早退不 panic；⑤paint ≡ paint_at（掩码唯一来源）。
//! 变异抽检：hit 与 paint 各自私算位置 → 题①红；圆底环删了 → 题③红；
//! 0.5 格间距改 0 → 题①红。

use kfm_na::ui::accent::AccentPair;
use kfm_na::ui::demo_icon::{DEMO_GLYPH_PX, DEMO_HIT_PX, hit, hit_rect, paint, paint_at};
use kfm_na::ui::gear;

const W: u32 = 800;
const H: u32 = 900;
const BG: u32 = 0x000D_0F13; // TERM_CARD_BG
const ACC: AccentPair = AccentPair {
    c1: 0x00C0_5080,
    c2: 0x0050_80C0,
};

#[test]
fn spec_demo_icon_几何单源与命中() {
    let (hx, hy, hw, hh) = hit_rect(W);
    assert_eq!((hw, hh), (DEMO_HIT_PX, DEMO_HIT_PX));
    // 齿轮正下方 0.5 格、同中轴（变异：间距改 0 / 中轴跑偏即红）
    let (ghx, ghy, ghw, ghh) = gear::hit_rect(W);
    assert_eq!(
        hy,
        ghy + ghh + kfm_na::termview::CELL_H / 2,
        "齿轮下 0.5 格"
    );
    assert_eq!(hx + hw / 2, ghx + ghw / 2, "与齿轮同中轴");
    // 命中：盒心必中；盒外 1px 不中
    let (cx, cy) = (hx + hw / 2, hy + hh / 2);
    assert!(hit(f64::from(cx), f64::from(cy), W));
    assert!(!hit(f64::from(hx) - 1.0, f64::from(cy), W));
    assert!(!hit(f64::from(cx), f64::from(hy) - 1.0, W));
    assert!(hit(f64::from(hx + hw) - 0.5, f64::from(cy), W));
    assert!(!hit(f64::from(hx + hw) + 0.5, f64::from(cy), W));
    // 尺寸同族缩小：比齿轮命中盒小（const 块 = 编译期钉，跌破即编不过）
    const {
        assert!(DEMO_HIT_PX < gear::GEAR_HIT_PX);
    }
}

#[test]
fn spec_demo_icon_涂装掩码纪律() {
    let mut buf = vec![BG; (W * H) as usize];
    paint(&mut buf, W, H, ACC);
    let (hx, hy, hw, _) = hit_rect(W);
    let (cx, cy) = (hx + hw / 2, hy + hw / 2);
    let at = |x: u32, y: u32| buf[(y * W + x) as usize];
    let g = f64::from(DEMO_GLYPH_PX);
    let r_body = g * 0.30;
    let ccy = f64::from(cy) + g * 0.14;
    // ① 圆底环着墨：圆底正下缘点（|dy|≈r_body）
    let ring_y = (ccy + r_body) as u32;
    assert_ne!(at(cx, ring_y), BG, "圆底环必须着墨");
    // ② 细颈竖线着墨：颈半宽处、颈高中段
    let neck_x = cx + (g * 0.09) as u32;
    let neck_y = cy - (g * 0.30) as u32;
    assert_ne!(at(neck_x, neck_y), BG, "细颈竖线必须着墨");
    // ③ 肩线着墨：颈底到圆底的中途点（取肩线参数 t≈0.5 处）
    let mid_x = cx + ((g * 0.09 + r_body * 0.92) / 2.0) as u32;
    let mid_y = (f64::from(cy) - g * 0.18
        + (ccy - r_body * 0.35 - (f64::from(cy) - g * 0.18)) / 2.0) as u32;
    assert_ne!(at(mid_x, mid_y), BG, "肩线必须着墨");
    // ④ 圆底内腔全透：圆心底心一字不动
    assert_eq!(at(cx, ccy as u32), BG, "圆底内腔必须透出底色");
    // ⑤ 字形盒外一字不动
    let gx0 = cx - DEMO_GLYPH_PX / 2;
    assert_eq!(at(gx0 - 4, cy), BG, "盒外不许着墨");
    // ⑥ 着墨方向朝 accent 渐变（不是灰不是白）：着墨像素应带 accent 色相
    let p = at(cx, ring_y);
    assert_ne!(p & 0xFF, (p >> 16) & 0xFF, "渐变着墨不许是无色相灰");
}

#[test]
fn spec_demo_icon_小缓冲早退() {
    let mut tiny = vec![0u32; 16];
    paint(&mut tiny, 4, 4, ACC);
    assert!(tiny.iter().all(|&p| p == 0), "小缓冲早退一字不写");
}

#[test]
fn spec_demo_icon_paint与paint_at同一掩码() {
    let mut a = vec![BG; (W * H) as usize];
    let mut b = vec![BG; (W * H) as usize];
    paint(&mut a, W, H, ACC);
    let (hx, hy, hw, hh) = hit_rect(W);
    paint_at(&mut b, W, H, hx + hw / 2, hy + hh / 2, ACC);
    assert_eq!(a, b, "paint ≡ paint_at(命中盒心)——掩码唯一来源");
    let mut c = vec![BG; (W * H) as usize];
    paint_at(&mut c, W, H, W / 2, H / 2, ACC);
    assert_ne!(a, c, "异心位必须画出不同结果");
}
