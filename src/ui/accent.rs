//! accent.rs — 随机 accent 生成器（主题宪法 §2.2，2026-09-12 修宪拍板）。
//!
//! kfmv4 `card-stack.ts _generateRandomAccents` 约束区间 HSL 随机的
//! 原样移植（不策展不抽板）：
//!   c1 色相 = 随机 0–360°；c2 色相 = c1 ± 30°–120°（避开同色与正对撞色）；
//!   饱和度 45%–70%、亮度 50%–65%（双色共享同一组 sat/lit 保明度协调）。
//! 背景固定深底不随 accent（颜色信息全由边框/装饰承载）。
//!
//! 纳管范围：文件树/解析/配置三个可召唤页面，**每次召唤重新生成**；
//! AI 页不纳入（主题色蓝紫，宪法 §2.1）。反向重开（关到一半再拉开）
//! 不重新生成——那是栈内事件不经过召唤，accent 自然沿用（宪法同款
//! BAR-CARD-ACCENT-01 条款的 NA 结构性兑现）。
//!
//! 纯逻辑 A 档：考题 tests/accent_spec.rs（区间钉/共享钉/确定性钉 +
//! 变异抽检）。

/// 一对 accent（c1 = 渐变起点，c2 = 渐变终点；0x00RRGGBB 与全局色约定同）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentPair {
    pub c1: u32,
    pub c2: u32,
}

/// xorshift64* 轻量随机源（避免引 rand 依赖；种子由壳层注时间戳，
/// 每召唤推进一次）。纯确定性：同种子同序列——考题可复现。
pub struct AccentRng(u64);

impl AccentRng {
    /// 种子不许为 0（xorshift 全零死锁），壳层注时间戳天然非零，
    /// 这里再兜一次底
    pub fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// [0, 1) 均匀浮点
    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        // xorshift64*：乘魔数后取高 53 位压进 [0,1)
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        v as f64 / (1u64 << 53) as f64
    }

    /// [lo, hi) 均匀浮点
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }

    /// 生成一对 accent（kfmv4 约束区间逐项对应，见模块头）
    pub fn generate(&mut self) -> AccentPair {
        let h1 = self.range(0.0, 360.0);
        // c2 在色环上与 c1 保持 30°–120° 偏差，双向随机（kfmv4 原注释：
        // 避免过于接近或完全随机撞色）
        let offset = self.range(30.0, 120.0) * if self.next_f64() > 0.5 { 1.0 } else { -1.0 };
        let h2 = (h1 + offset).rem_euclid(360.0);
        let sat = self.range(45.0, 70.0);
        let lit = self.range(50.0, 65.0);
        AccentPair {
            c1: hsl_to_rgb(h1, sat, lit),
            c2: hsl_to_rgb(h2, sat, lit),
        }
    }
}

/// HSL → 0x00RRGGBB（h ∈ [0,360)，s/l ∈ [0,100]；CSS hsl() 语义直译）
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> u32 {
    let s = s / 100.0;
    let l = l / 100.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to255 = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (to255(r) << 16) | (to255(g) << 8) | to255(b)
}

/// 三页共享的卡片深底（宪法 §2.2：背景固定深底不随 accent——
/// kfmv4 rgba(20,16,32,0.92) 压平到不透明的事后色）
pub const CARD_PAGE_BG: u32 = 0x0014_1020;

/// presence 不在场时的兜底 accent（旧配置青系——壳层早期帧/异常态
/// 用，正常运行永远走 presence 里的随机对）
pub const FALLBACK: AccentPair = AccentPair {
    c1: 0x0000_F0C8,
    c2: 0x0020_90D0,
};
