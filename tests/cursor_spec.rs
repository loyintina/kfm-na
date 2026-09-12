//! tests/cursor_spec.rs — 功能光标开口框几何考题（src/ui/cursor.rs，
//! 2026-09-12 三修，宪法 §三/§四）
//!
//! 契约真相源：kfmv4 canvas-cursor 开口框规格（左粗 3px/细 1px/R4/
//! 突出 1.65px/线域 [20, 框宽−10]）+ NA 随机线长机制（每次选中切换
//! 重掷，|顶−底| ≥ 1 格上下不对齐；窄框退化域内最大差）。
//! 判卷点：①规格常量钉值（防手滑）；②线域钉（上下限咬合）；
//! ③不齐钉（200 种子扫场，去闸变异必红——抽检靶）；④确定性钉
//! （同种子同付）；⑤窄框退化钉（域空→双零不画线；域幅不足 1 格→
//! 退化为域内差）。
//! 纪律：先验证红，答案生成到绿。本文件是考题，生成器不许改。

use kfm_na::ui::accent::AccentRng;
use kfm_na::ui::cursor::{
    BG_ALPHA, CORNER_R, EMPHASIS_W, HAIR_W, LINE_ALPHA, LINE_MARGIN, LINE_MIN, MIN_SKEW,
    OpenCursorGeom, PROTRUDE, roll,
};

// ========== ①规格常量钉值（kfmv4 复刻契约，防手滑） ==========

#[test]
fn spec_cursor_规格常量钉() {
    assert_eq!(LINE_ALPHA, 178, "框线 α = kfmv4 0.7 压 255 档");
    assert_eq!(BG_ALPHA, 38, "底垫 α = kfmv4 0.15 压 255 档");
    assert_eq!(EMPHASIS_W, 3, "左强调线宽 = kfmv4 EW=3");
    assert_eq!(HAIR_W, 1, "发丝线宽 = kfmv4 NW=1");
    assert_eq!(CORNER_R, 4, "圆角半径 = kfmv4 R=4");
    assert_eq!(PROTRUDE, 1.65, "左线突出 = kfmv4 x-1.65");
    assert_eq!(LINE_MIN, 20, "线域下限 = kfmv4 topLineW 下限 20");
    assert_eq!(LINE_MARGIN, 10, "线域上限余量 = kfmv4 totalW-10");
    assert_eq!(MIN_SKEW, 18, "上下最小不齐差 = 1 格（CELL_W）");
}

// ========== ②线域钉：顶/底线长各自 ∈ [min(20, w−10), w−10] ==========

#[test]
fn spec_cursor_线域钉() {
    for (w, lo) in [
        (90u32, 20i64),
        (180, 20),
        (40, 20),
        (30, 20),
        (28, 18),
        (12, 2),
    ] {
        let hi = w as i64 - LINE_MARGIN;
        let want_lo = lo.min(hi);
        for seed in 1..=50u64 {
            let mut rng = AccentRng::new(seed.wrapping_mul(0x9E37_79B9));
            let g = roll(w, &mut rng);
            assert!(
                g.top_w >= want_lo && g.top_w <= hi,
                "w={w} 顶线 {} 越域 [{want_lo}, {hi}]",
                g.top_w
            );
            assert!(
                g.bot_w >= want_lo && g.bot_w <= hi,
                "w={w} 底线 {} 越域 [{want_lo}, {hi}]",
                g.bot_w
            );
        }
    }
}

// ========== ③不齐钉：|顶−底| ≥ min(1格, 域幅)——去闸变异必红 ==========

#[test]
fn spec_cursor_上下不对齐钉() {
    // 宽域（标签典型宽 90~180）：必须满 1 格不齐
    for seed in 1..=200u64 {
        let mut rng = AccentRng::new(seed.wrapping_mul(0x2545_F491));
        let g = roll(180, &mut rng);
        assert!(
            (g.top_w - g.bot_w).abs() >= MIN_SKEW,
            "seed={seed} 上下线近乎对齐（|{}−{}| < {MIN_SKEW}）——不齐闸丢了？",
            g.top_w,
            g.bot_w
        );
    }
    // 窄域（w=40：域 [20,30] 幅 10）：skew = min(1格, 幅/2) = 5，
    // 构造保证恒可达（合法域恒非空）
    for seed in 1..=200u64 {
        let mut rng = AccentRng::new(seed.wrapping_mul(0x1B69_36B5));
        let g = roll(40, &mut rng);
        assert!(
            (g.top_w - g.bot_w).abs() >= 5,
            "w=40 半幅 skew=5 恒真（seed={seed} 得 |{}−{}|）",
            g.top_w,
            g.bot_w
        );
    }
}

// ========== ④确定性钉：同种子同付（考题复现/跨端一致的前提） ==========

#[test]
fn spec_cursor_确定性钉() {
    for seed in [1u64, 42, 0xDEAD_BEEF] {
        let g1 = roll(126, &mut AccentRng::new(seed));
        let g2 = roll(126, &mut AccentRng::new(seed));
        assert_eq!(g1, g2, "同种子必须同付（seed={seed:#x}）");
    }
    // 异种子大概率异付（随机的存在理由：每 boot 换装）
    let pays: std::collections::HashSet<(i64, i64)> = (1..=20u64)
        .map(|s| {
            let g = roll(126, &mut AccentRng::new(s));
            (g.top_w, g.bot_w)
        })
        .collect();
    assert!(pays.len() >= 15, "20 种子至少 15 种付（{pays:?}）");
}

// ========== ⑤窄框退化钉：域空双零不画线，底线恒在界内 ==========

#[test]
fn spec_cursor_窄框退化钉() {
    for w in [0u32, 5, 10] {
        let g = roll(w, &mut AccentRng::new(7));
        assert_eq!(
            g,
            OpenCursorGeom { top_w: 0, bot_w: 0 },
            "w={w} 框太窄必须双零（只画左线+圆角，不画发丝线）"
        );
    }
    // w−10 ≤ 20 的边界带：下限塌到 hi，域幅 0 时两线相等是唯一解
    let g = roll(30, &mut AccentRng::new(7));
    assert_eq!(g.top_w, 20, "w=30 域塌成单点 [20,20]");
    assert_eq!(g.bot_w, 20, "w=30 域幅 0：相等是域内唯一解（不算违齐）");
}

// ========== TabBar 集成钉：选中切换重掷，同标重按不重掷 ==========

#[test]
fn spec_cursor_tabbar集成_移动才换装() {
    let mut bar = kfm_na::ui::tab_bar::TabBar::new_seeded(&["系统管理", "API"], 720, 99);
    let s0 = bar.snap(0);
    let (t0, b0) = (s0.cursor_top_w, s0.cursor_bot_w);
    assert!(t0 > 0 && b0 > 0, "初态必须有一付线长（标签 0 宽 90）");
    // 同标重按：没移动不换装
    bar.select(0, 100);
    let s1 = bar.snap(100);
    assert_eq!(
        (s1.cursor_top_w, s1.cursor_bot_w),
        (t0, b0),
        "同标重按不许重掷（没移动不换装）"
    );
    // 真换标：重掷（宽度不同域不同，值几乎必变；且不齐闸仍成立）
    bar.select(1, 200);
    let s2 = bar.snap(200);
    assert!(
        (s2.cursor_top_w - s2.cursor_bot_w).abs() >= MIN_SKEW,
        "换标后新付也必须过不齐闸（API 宽 90：skew = min(18, 60/2) = 18）"
    );
    assert_ne!(
        (s2.cursor_top_w, s2.cursor_bot_w),
        (t0, b0),
        "seed=99 换标必须重掷出新付（定种子钉死这付快照）"
    );
    // 快照字段 = 状态单源（涂装照抄不许自算的眼手同尺钉）
    assert_eq!(s2.cursor_top_w, bar.snap(300).cursor_top_w, "快照必须稳定");
}
