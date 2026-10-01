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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// 0x00RRGGBB → (h, s, l)（hsl_to_rgb 的逆；考题侧另有独立反解器互证，
/// 两侧错任何一侧 round-trip 必红）
pub fn rgb_to_hsl(rgb: u32) -> (f64, f64, f64) {
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

// ---- 淡彩六色家族（宪法 §2.5，2026-09-27 修宪：固定三色 → accent 派生，
// 用户拍板「根据随机色、固定角度」）----

/// 色环固定角度步进（°）：六色 = c1 色相 + Δ
pub const PASTEL_DELTAS: [f64; 6] = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0];
/// 淡彩化参数：饱和度 = accent 饱和度 × 0.55（钳 25–45%），亮度 75%
/// ——低饱和高明度家族，与示警三档（§2.4 高饱和）形态分家
pub const PASTEL_SAT_SCALE: f64 = 0.55;
pub const PASTEL_SAT_MIN: f64 = 25.0;
pub const PASTEL_SAT_MAX: f64 = 45.0;
pub const PASTEL_LIT: f64 = 75.0;

/// 淡彩六色家族：从本页随机 accent c1 的色相出发，固定角度步进取六色，
/// 每色淡彩化。随每次召唤换装（accent §2.2 每召唤重随，家族同源跟随）。
/// 槽位角色映射（粗体/H4/保留/行内码/H2/H3）在调用侧钉死（demo_page::
/// pastel_role），本函数只管色不管义
pub fn pastel_family(accent_c1: u32) -> [u32; 6] {
    let (h, s, _l) = rgb_to_hsl(accent_c1);
    let ps = (s * PASTEL_SAT_SCALE).clamp(PASTEL_SAT_MIN, PASTEL_SAT_MAX);
    PASTEL_DELTAS.map(|d| hsl_to_rgb((h + d).rem_euclid(360.0), ps, PASTEL_LIT))
}

/// 三页共享的卡片深底（宪法 §2.2：背景固定深底不随 accent——
/// kfmv4 rgba(20,16,32,0.92) 压平到不透明的事后色）
pub const CARD_PAGE_BG: u32 = 0x0014_1020;

// ---- 按发信人稳定取色（BAR-214 信箱页三级框，2026-10-01 用户拍板：
// 同一发信人永远随机到同一组双色做渐变，不同发信人不同色，且要跟
// 页面颜色拉开）----

/// 发信人色与页 accent c1 的最小色相距（°）：小于此距视为撞色要重 roll
pub const SENDER_HUE_MIN_DIST: f64 = 40.0;
/// 防撞重 roll 上限：序列推进确定性，8 次内找不到就整体色相 +180° 兜底
/// （色环上对撞区永远只占 2×MIN_DIST，8 次实际必中，兜底是死保险）
pub const SENDER_ROLL_MAX: u32 = 8;

/// FNV-1a 64 哈希（轻量无依赖；同串同值跨次运行稳定——这是「同一发信人
/// 同一色」的全部保证来源）。pub 理由：考题要拿它复算首发 roll 构造
/// 脱撞真咬案例（tests/accent_spec.rs BAR-214 钉）
pub fn fnv1a64(s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 色环距离（°，0–180]）
fn hue_dist(a: f64, b: f64) -> f64 {
    let d = (a - b).abs().rem_euclid(360.0);
    d.min(360.0 - d)
}

/// 按发信人名取稳定双色：种子 = FNV-1a（名），走 AccentRng 同一约束区间
/// （与页 accent 同族的 HSL 随机，观感同族）。与页 accent c1 色相距
/// < SENDER_HUE_MIN_DIST 时推进序列重 roll；SENDER_ROLL_MAX 次未脱撞
/// 则双色色相 +180° 强制脱撞（确定性，不引新随机源）。
pub fn accent_for_sender(sender: &str, page: AccentPair) -> AccentPair {
    let mut rng = AccentRng::new(fnv1a64(sender));
    let (page_h, _, _) = rgb_to_hsl(page.c1);
    for _ in 0..SENDER_ROLL_MAX {
        let pair = rng.generate();
        let (h, _, _) = rgb_to_hsl(pair.c1);
        if hue_dist(h, page_h) >= SENDER_HUE_MIN_DIST {
            return pair;
        }
    }
    // 死保险：重 roll 全撞（实测不可能），整体转 180° 必脱撞
    let pair = rng.generate();
    let (h1, s, l) = rgb_to_hsl(pair.c1);
    let (h2, _, _) = rgb_to_hsl(pair.c2);
    AccentPair {
        c1: hsl_to_rgb((h1 + 180.0).rem_euclid(360.0), s, l),
        c2: hsl_to_rgb((h2 + 180.0).rem_euclid(360.0), s, l),
    }
}

/// presence 不在场时的兜底 accent（旧配置青系——壳层早期帧/异常态
/// 用，正常运行永远走 presence 里的随机对）
pub const FALLBACK: AccentPair = AccentPair {
    c1: 0x0000_F0C8,
    c2: 0x0020_90D0,
};
