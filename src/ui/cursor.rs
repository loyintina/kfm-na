//! cursor.rs — 功能光标（主题宪法 §三 功能光标条款 + §四 标签栏条款，
//! 2026-09-12 三修用户拍板）。核心层纯逻辑 A 档，只产几何，不碰像素。
//!
//! kfmv4 `canvas-cursor.ts` 开口框复刻：左强调线 3px（向左突出 1.65px，
//! 上下跳过圆角区）+ 左上/左下圆角 R=4 线宽沿弧渐变（3→1 / 1→3）+
//! 顶线 1px 锚左 + 底线 1px 锚左，**无右边**；底色绿青半透明垫整个
//! 框体。颜色固定纯蓝 `#00D4FF` α0.7——光标是主题色的一部分，永不
//! 随机（不吃 accent、不吃渐变，宪法 §三 功能光标 vs 装修框分家）。
//!
//! **NA 特有：线长随机机制**（用户拍板）：kfmv4 顶线长 = 文字宽、底线
//! 长 = 剩余——标签不像文件行够宽且字数随机，跟随文字会让上下线近似
//! 对齐。改为**每次选中切换重掷**：顶/底线长各自随机 ∈ [20, 框宽−10]，
//! 且保证 |顶−底| ≥ min(1 格, 域幅/2)（上下不对齐是观感本体，恒真
//! 不变量——构造保证非事后修补）。随机源复用 accent 的 xorshift64*
//! （种子壳层注时间戳；考题喂定种子可复现）。
//!
//! 呼吸/液体光**不移植**（kfmv4 模式联动特殊选中态专属，NA 无此场景，
//! 用户拍板）。
//!
//! 纯逻辑 A 档：考题 tests/cursor_spec.rs（域钉/不齐钉/确定性钉/窄框
//! 退化钉 + 变异抽检）。

use crate::termview::CELL_W;
use crate::ui::accent::AccentRng;

/// 框线 α（kfmv4 rgba(0,212,255,0.7) 的 0.7 压 255 档）
pub const LINE_ALPHA: u32 = 178;
/// 底垫 α（kfmv4 rgba(46,213,163,0.15) 的 0.15 压 255 档）
pub const BG_ALPHA: u32 = 38;
/// 左强调线宽 px（kfmv4 EW=3）
pub const EMPHASIS_W: i64 = 3;
/// 细线宽 px（kfmv4 NW=1，顶/底线与角尾）
pub const HAIR_W: i64 = 1;
/// 圆角半径 px（kfmv4 R=4）
pub const CORNER_R: i64 = 4;
/// 左线向左突出 px（kfmv4 x-1.65；亚像素由涂装侧覆盖率兑现）
pub const PROTRUDE: f64 = 1.65;
/// 线长随机域下限 px（kfmv4 topLineW 下限 20 同款）
pub const LINE_MIN: i64 = 20;
/// 线长随机域上限 = 框宽 − 本值（kfmv4 totalW-10 同款）
pub const LINE_MARGIN: i64 = 10;
/// 上下线最小不齐差 = 1 格（用户拍板：上下不对齐是观感本体）
pub const MIN_SKEW: i64 = CELL_W as i64;

/// 开口框一次投掷的几何结果（顶/底线长，px；0 = 框太窄不画线）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCursorGeom {
    pub top_w: i64,
    pub bot_w: i64,
}

/// 投掷一付线长（w = 光标框宽 px）。域 [min(20, w−10), w−10]；
/// w−10 ≤ 0 时双零（框太窄只画左线+圆角，钉⑤）。
/// 不齐闸：skew = min(1格, 域幅/2)，底线从合法域
/// [lo, 顶−skew] ∪ [顶+skew, hi] 两侧按幅加权挑侧再域内均匀——
/// skew 钳半幅保证合法域恒非空（2·skew ≤ 域幅 ⇒ 顶线贴中也有一侧
/// 可去），因此 |顶−底| ≥ skew 是**恒真不变量**不是事后修补
pub fn roll(w: u32, rng: &mut AccentRng) -> OpenCursorGeom {
    let hi = i64::from(w) - LINE_MARGIN;
    if hi <= 0 {
        return OpenCursorGeom { top_w: 0, bot_w: 0 };
    }
    let lo = LINE_MIN.min(hi);
    let span = hi - lo;
    let pick_in = |rng: &mut AccentRng, a: i64, b: i64| {
        a + ((rng.next_f64() * (b - a + 1) as f64) as i64).min(b - a)
    };
    let top = pick_in(rng, lo, hi);
    let skew = MIN_SKEW.min(span / 2);
    let bot = if skew == 0 {
        pick_in(rng, lo, hi) // 域幅 0/1：不齐无可追求，自由落
    } else {
        let (v1, v2) = (top - skew - lo + 1, hi - (top + skew) + 1); // 两侧候选数
        let (v1, v2) = (v1.max(0), v2.max(0));
        let t = (rng.next_f64() * (v1 + v2) as f64) as i64;
        if t < v1 {
            pick_in(rng, lo, top - skew)
        } else {
            pick_in(rng, top + skew, hi)
        }
    };
    OpenCursorGeom {
        top_w: top,
        bot_w: bot,
    }
}
