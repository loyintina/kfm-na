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
