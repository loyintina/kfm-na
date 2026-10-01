//! accent_spec.rs — 随机 accent 生成器考题（主题宪法 §2.2，A 档）。
//!
//! 钉的是 kfmv4 约束区间的逐项兑现：色相偏差 30°–120°、sat/lit 区间
//! 且双色共享、生成器确定性（同种子同序列——可复现是判卷前提）。
//! 变异抽检（已实测咬人）：
//! ① 偏差下界 30 改 0 → 钉①红色相差过小的对被抓；
//! ② sat 共享破（c2 独立抽） → 钉②红（双色 sat 不等）；
//! ③ rem_euclid 改 %（负色相回绕错） → 钉①③红（负偏移时色相出错）。

use kfm_na::ui::accent::{AccentRng, hsl_to_rgb};

/// rgb → (h, s, l)（h ∈ [0,360)，s/l ∈ [0,100]；考题侧反解器，
/// 与实现互证——实现 hsl_to_rgb 错时反解回来必对不上）
fn rgb_to_hsl(rgb: u32) -> (f64, f64, f64) {
    let r = ((rgb >> 16) & 0xFF) as f64 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f64 / 255.0;
    let b = (rgb & 0xFF) as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-9 {
        return (0.0, 0.0, l * 100.0);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if (max - r).abs() < 1e-9 {
        60.0 * (((g - b) / d) % 6.0)
    } else if (max - g).abs() < 1e-9 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s * 100.0, l * 100.0)
}

/// 钉①：大批采样下，c2 与 c1 的色相差恒 ∈ [30°,120°]（双向），
/// sat ∈ [45,70]、lit ∈ [50,65]（kfmv4 区间原样）
#[test]
fn spec_accent_区间钉() {
    let mut rng = AccentRng::new(42);
    for _ in 0..2000 {
        let pair = rng.generate();
        let (h1, s1, l1) = rgb_to_hsl(pair.c1);
        let (h2, s2, l2) = rgb_to_hsl(pair.c2);
        let diff = (h2 - h1).abs().min(360.0 - (h2 - h1).abs());
        assert!(
            (29.0..=121.0).contains(&diff), // 1° 量化容差（round-trip 精度）
            "色相差 {diff} 越界（h1={h1} h2={h2}）"
        );
        assert!((44.0..=71.0).contains(&s1), "sat {s1} 越界");
        assert!((49.0..=66.0).contains(&l1), "lit {l1} 越界");
        assert!(
            (s1 - s2).abs() < 1.0 && (l1 - l2).abs() < 1.0,
            "双色 sat/lit 不共享：({s1},{l1}) vs ({s2},{l2})"
        );
    }
}

/// 钉②：确定性——同种子同序列（可复现），异种子异序列
#[test]
fn spec_accent_确定性钉() {
    let mut a1 = AccentRng::new(7);
    let mut a2 = AccentRng::new(7);
    let mut b = AccentRng::new(8);
    for _ in 0..50 {
        assert_eq!(a1.generate(), a2.generate(), "同种子必须同序列");
    }
    let seq_a: Vec<_> = (0..10).map(|_| a1.generate()).collect();
    let seq_b: Vec<_> = (0..10).map(|_| b.generate()).collect();
    assert_ne!(seq_a, seq_b, "异种子 10 连撞同序列 = 生成器坏了");
}

/// 钉③：hsl_to_rgb 端点互证（红/绿/蓝/白/黑 + 中间色 round-trip）
#[test]
fn spec_hsl_to_rgb_端点钉() {
    assert_eq!(hsl_to_rgb(0.0, 100.0, 50.0), 0x00FF_0000);
    assert_eq!(hsl_to_rgb(120.0, 100.0, 50.0), 0x0000_FF00);
    assert_eq!(hsl_to_rgb(240.0, 100.0, 50.0), 0x0000_00FF);
    assert_eq!(hsl_to_rgb(0.0, 0.0, 100.0), 0x00FF_FFFF);
    assert_eq!(hsl_to_rgb(0.0, 0.0, 0.0), 0x0000_0000);
    // round-trip：约束区间内的随机点，反解回去 h/s/l 各自偏差 < 1
    let mut rng = AccentRng::new(99);
    for _ in 0..500 {
        let h = rng.next_f64() * 360.0;
        let s = 45.0 + rng.next_f64() * 25.0;
        let l = 50.0 + rng.next_f64() * 15.0;
        let (h2, s2, l2) = rgb_to_hsl(hsl_to_rgb(h, s, l));
        let hd = (h2 - h).abs().min(360.0 - (h2 - h).abs());
        assert!(hd < 1.5 && (s2 - s).abs() < 1.5 && (l2 - l).abs() < 1.5);
    }
}

/// 钉④：零种子兜底（xorshift 全零死锁防御——换固定非零种子继续出活）
#[test]
fn spec_accent_零种子兜底钉() {
    let mut rng = AccentRng::new(0);
    let a = rng.generate();
    let b = rng.generate();
    assert_ne!(a, b, "零种子兜底后必须正常出随机序列");
}

/// 钉⑤：淡彩六色家族（宪法 §2.5，2026-09-27 修宪——「根据随机色、固定
/// 角度」）：六色 = c1 色相 + {0,60,…,300}°（考题侧独立反解器互证），
/// 饱和度 = accent sat × 0.55 钳 [25,45]，亮度 75%；slot0 与 c1 同色相
/// （accent 本相淡彩）；六色互异（60° 步进不撞车）。
/// **角度表/亮度必须考题侧写字面量**——import 实现侧常量 = 自指盲钉
/// （2026-09-27 变异抽检实录：DELTAS 改 45° 步进、LIT 改 60 两咬全空，
/// 改字面量后两咬全中）
#[test]
fn spec_pastel_六色家族角度钉() {
    use kfm_na::ui::accent::pastel_family;
    const DELTAS: [f64; 6] = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0]; // 宪法字面量，不许 import
    const LIT: f64 = 75.0; // 同上
    let mut rng = AccentRng::new(2026);
    for _ in 0..500 {
        let c1 = rng.generate().c1;
        let (h1, s1, _) = rgb_to_hsl(c1);
        let fam = pastel_family(c1);
        assert_eq!(fam.len(), 6, "家族必须六色");
        for (i, c) in fam.iter().enumerate() {
            let (h, s, l) = rgb_to_hsl(*c);
            // 色相 = h1 + Δ[i]（环形距离容差 2.5°——淡彩档 sat 低，
            // RGB 量化粒度 ≈1.9°，容差须盖过量化噪声仍咬得住 60° 变异）
            let expect = (h1 + DELTAS[i]).rem_euclid(360.0);
            let hd = (h - expect).abs().min(360.0 - (h - expect).abs());
            assert!(hd < 2.5, "slot{i} 色相差 {hd}°（h={h} expect={expect}）");
            // 淡彩化：sat = accent sat × 0.55 钳 [25,45]、lit = 75
            let expect_s = (s1 * 0.55).clamp(25.0, 45.0);
            assert!(
                (s - expect_s).abs() < 2.0,
                "slot{i} sat {s} 偏离淡彩化期望 {expect_s}"
            );
            assert!((l - LIT).abs() < 1.5, "slot{i} lit {l} 必须是淡彩档 {LIT}");
        }
        // 六色互异（60° 步进同 sat/lit 下不撞车）
        for i in 0..6 {
            for j in i + 1..6 {
                assert_ne!(fam[i], fam[j], "slot{i} 与 slot{j} 撞色");
            }
        }
    }
}

/// 钉⑥：淡彩家族确定性 + 随 accent 换装（同 c1 同家族；异 c1 异家族——
/// 「每次召唤颜色都随机」的兑现钉）
#[test]
fn spec_pastel_确定性与换装钉() {
    use kfm_na::ui::accent::pastel_family;
    let a = pastel_family(0x00E0_6030);
    assert_eq!(a, pastel_family(0x00E0_6030), "同 c1 必须同家族");
    let b = pastel_family(0x0030_E0A0);
    assert_ne!(a, b, "异 c1 必须异家族（随召唤换装）");
}

/// 钉⑦：淡彩常量表值钉 + rgb_to_hsl 互证（编译期钉值——运行期常量断言
/// 会撞 clippy assertions_on_constants，const 块编译期咬更硬；棘轮
/// 覆盖同源：符号名必须在 tests/ 出现）
#[test]
fn spec_pastel_常量表值钉与反解互证() {
    use kfm_na::ui::accent::{
        PASTEL_DELTAS, PASTEL_LIT, PASTEL_SAT_MAX, PASTEL_SAT_MIN, PASTEL_SAT_SCALE,
        rgb_to_hsl as impl_rgb_to_hsl,
    };
    const {
        assert!(PASTEL_DELTAS[0] == 0.0);
        assert!(PASTEL_DELTAS[1] == 60.0);
        assert!(PASTEL_DELTAS[2] == 120.0);
        assert!(PASTEL_DELTAS[3] == 180.0);
        assert!(PASTEL_DELTAS[4] == 240.0);
        assert!(PASTEL_DELTAS[5] == 300.0);
        assert!(PASTEL_SAT_SCALE == 0.55);
        assert!(PASTEL_SAT_MIN == 25.0);
        assert!(PASTEL_SAT_MAX == 45.0);
        assert!(PASTEL_LIT == 75.0);
    }
    // 实施侧 rgb_to_hsl 与考题侧独立反解器逐点对表（同名不同源，
    // 一侧写错必对不上）
    let mut rng = AccentRng::new(77);
    for _ in 0..300 {
        let c = rng.generate().c1;
        let (h1, s1, l1) = impl_rgb_to_hsl(c);
        let (h2, s2, l2) = rgb_to_hsl(c);
        let hd = (h1 - h2).abs().min(360.0 - (h1 - h2).abs());
        assert!(hd < 1e-9 && (s1 - s2).abs() < 1e-9 && (l1 - l2).abs() < 1e-9);
    }
}

/// 钉⑧（BAR-214）：按发信人稳定取色——同名同色/异名异色/与页色脱撞。
/// 变异咬：种子改固定值（同名不同色必红）/ MIN_DIST 归 0（脱撞钉必红）/
/// 重 roll 不推进序列（死循环或同色必红）。
#[test]
fn spec_bar214_按发信人稳定取色() {
    use kfm_na::ui::accent::{AccentPair, SENDER_HUE_MIN_DIST, accent_for_sender, rgb_to_hsl};
    let page = AccentPair {
        c1: 0x0000_F0C8,
        c2: 0x0020_90D0,
    };
    // 同名同色（跨「次运行」稳定 = 纯函数无时间源）
    let a1 = accent_for_sender("观澜", page);
    let a2 = accent_for_sender("观澜", page);
    assert_eq!(a1, a2, "同一发信人必须永远同一组双色");
    // 异名异色（六个真名册名两两不许全同）
    let names = ["观澜", "清和", "承影", "闻灯", "白露", "蔚然"];
    for (i, x) in names.iter().enumerate() {
        for y in &names[i + 1..] {
            assert_ne!(
                accent_for_sender(x, page),
                accent_for_sender(y, page),
                "{x} 与 {y} 撞色"
            );
        }
    }
    // 脱撞守卫：任何发信人结果与页 c1 色相距 ≥ MIN_DIST
    let (ph, _, _) = rgb_to_hsl(page.c1);
    for n in names {
        let (h, _, _) = rgb_to_hsl(accent_for_sender(n, page).c1);
        let d = (h - ph).abs().min(360.0 - (h - ph).abs());
        assert!(
            d >= SENDER_HUE_MIN_DIST - 1.0, // hsl round-trip 量化容差 1°
            "{n} 与页色相距 {d}° < {SENDER_HUE_MIN_DIST}°"
        );
    }
    // 页色换装后同名仍稳定（页色只参与脱撞不参与播种，脱撞路径确定性）
    let page2 = AccentPair {
        c1: 0x00E0_6030,
        c2: 0x0030_E0A0,
    };
    assert_eq!(
        accent_for_sender("观澜", page2),
        accent_for_sender("观澜", page2)
    );
    // 脱撞路径真咬：把页色钉成首发 roll（种子只问发信人，考题可复算）
    // ——守卫必须推进序列脱撞。变异：守卫归 0 恒收首发 → d=0 本断言红
    const {
        assert!(
            kfm_na::ui::accent::SENDER_ROLL_MAX >= 4,
            "重 roll 上限不许归零（脱撞守卫的弹药）"
        );
    }
    let first = kfm_na::ui::accent::AccentRng::new(kfm_na::ui::accent::fnv1a64("观澜")).generate();
    let collide = AccentPair {
        c1: first.c1,
        c2: first.c2,
    };
    let escaped = accent_for_sender("观澜", collide);
    let (eh, _, _) = rgb_to_hsl(escaped.c1);
    let (ch, _, _) = rgb_to_hsl(collide.c1);
    let d = (eh - ch).abs().min(360.0 - (eh - ch).abs());
    assert!(
        d >= SENDER_HUE_MIN_DIST - 1.0,
        "页色撞首发 roll 时必须脱撞（相距 {d}° < {SENDER_HUE_MIN_DIST}°）"
    );
}
