//! dual_pool.rs — 双池骨架（主题宪法 §五「kfmv4 系列的灵魂」，核心层
//! 纯布局数学，A 档钉）。
//!
//! 条款兑现：首行布局区之下两个二级卡片，动态高度——
//!   上池高 = min(上池内容高, H/2)（内容几行跟几行；多了不越半，超出
//!     部分上池内滚动——滚动是内容的事，骨架只报 upper_scroll 旗）；
//!   下池高 = H − 上池高 − 池间距（恒撑满剩余，内容少也撑到卡底）；
//!   两池间距 1 格（2026-09-12 实测修订：直接衔接太密），与首行留
//!     1 格标准间距；
//!   上池空也占位（A2 拍板）：空内容按 4 格占位高计（二标 2026-09-12：
//!     2 格太矮）——池页切换时双池高度生长动画需要它。
//!
//! 下池内容高不参与布局（下池恒 = H − 上池 − 间距）——签名即契约。
//!
//! 十五修增订（2026-09-14，宪法 §五 池区动画条款）：上池高度动画——
//! 欠阻尼弹簧（BAR-094 废除）→ **BAR-095 分域律缓动核**（2026-09-15
//! 用户拍板「不要过冲，不是把动画搞没——光标到位池高也到位」）：
//! `glide_upper_content_h` = 250ms ease-in-out cubic 缓动（PAN_MS
//! 同钟同曲线，与光标滑行/上池平移严格同步，Upper 域点下池用）；
//! `set_upper_content_h` = 目标值直通（Page 域切标签——新页自己的
//! 池高起步帧就位，与非平移变更的基座语义）。layout 吃 now_ms。
//!
//! bottom_inset：池区底缘 = 页环底内缘，必须含键盘+输入栏带——漏算
//! 下池顶穿页环/压键行（2026-09-12 真机实踩）。
//!
//! 时间戳/墙钟：本册零墙钟，纯函数+状态。

use crate::termview::{AI_PAGE_FRAME_MARGIN, AI_PAGE_FRAME_W, CELL_H, CELL_W};
use crate::ui::tab_bar::{TAB_ROW_H, content_origin};

/// 首行布局区 → 上池标准间距 = 1 格（§四 二层双框「与首行留标准间距」）
pub const POOL_TOP_GAP: u32 = CELL_H;
/// 两池间距 = 1 格（2026-09-12 实测修订：0 格直接衔接太密）
pub const POOL_GAP: u32 = CELL_H;
/// 池区左右内边距各 = 2 格（2026-09-12 实测拍板：1 格太窄——池是卡片，
/// 不是标签栏那种贴缘内容带）
pub const POOL_SIDE_PAD: u32 = CELL_W * 2;
/// 上池空占位高 = 4 格（A2 拍板占位不消失；二标 2026-09-12：2 格太矮，
/// 4 格 = 两行池行高）
pub const POOL_EMPTY_H: u32 = CELL_H * 4;

/// 池框矩形（x/y 可随面板平移由涂装侧加偏移；涂装/命中共用——眼手同尺）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolRect {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// 双池可用区（首行之下 → 页环底内缘）：左右各让 2 格内边距（左起自
/// 粗环内缘、右止自细环内缘）；底内缘 = 屏底 − bottom_inset −
/// (MARGIN + 细缘 + 1 格)——与页环底缘同尺（bottom_inset = 键盘 +
/// 输入栏带，漏算 = 下池顶穿页环）
pub fn pool_area(screen_w: u32, screen_h: u32, bottom_inset: u32) -> PoolRect {
    let (_ox, oy) = content_origin();
    let x = AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W * 3 + POOL_SIDE_PAD;
    let y = oy + TAB_ROW_H + POOL_TOP_GAP;
    let bottom = screen_h
        .saturating_sub(bottom_inset)
        .saturating_sub(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_H);
    let right = screen_w.saturating_sub(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + POOL_SIDE_PAD);
    PoolRect {
        x: i64::from(x),
        y: i64::from(y),
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

/// 双池布局状态：上池内容高 + 可用区 + 池高缓动账。壳层持有一份（配置卡
/// 常驻，与标签栏同规）；内容高由池页内容侧喂（骨架期恒 0 = 空占位）。
/// 池高动画 = BAR-095 分域律：Upper 域 glide 250ms ease-in-out（与光标/
/// 平移同钟同曲线零过冲——BAR-094 弹簧废除不复辟）；Page 域/非平移
/// 变更 set 直通（新页面自己的池高起步帧就位）
pub struct DualPool {
    upper_content_h: u32,
    area: PoolRect,
    /// 池高缓动账：Some((起点高, 目标高, start_ms))，贴死即清
    glide: Option<(u32, u32, u64)>,
    /// 最近一次 layout 的瞬时池高（重定基起点——滑行中再喂目标从
    /// 当前位置续滑不跳变）
    last_upper_h: u32,
}

/// 布局快照（涂装唯一读数口；upper_scroll = 上池内容被半高钳截断旗，
/// 骨架期只读不滚）。十七修：Clone/Debug——旧代冻结快照要封存整份几何
#[derive(Debug, Clone)]
pub struct DualPoolSnap {
    pub upper: PoolRect,
    pub lower: PoolRect,
    pub upper_scroll: bool,
}

impl DualPool {
    pub fn new(screen_w: u32, screen_h: u32) -> Self {
        DualPool {
            upper_content_h: 0,
            area: pool_area(screen_w, screen_h, 0),
            glide: None,
            last_upper_h: 0,
        }
    }

    pub fn set_viewport(&mut self, screen_w: u32, screen_h: u32, bottom_inset: u32) {
        self.area = pool_area(screen_w, screen_h, bottom_inset);
    }

    /// 目标值直通（BAR-095 分域律 Page 域/非平移变更）：池高立即落
    /// 目标（缓动账清掉——「新页面就是新页面」，起步帧就位零动画）
    pub fn set_upper_content_h(&mut self, h: u32) {
        self.upper_content_h = h;
        self.glide = None;
    }

    /// 缓动喂入（BAR-095 分域律 Upper 域）：目标变化即从当前瞬时值
    /// 重定基续滑 250ms ease-in-out（与光标/平移同钟同曲线，零过冲）；
    /// 目标未变幂等（平移期壳层每帧喂同一目标不重演）
    pub fn glide_upper_content_h(&mut self, h: u32, now_ms: u64) {
        self.upper_content_h = h;
        let target = self.target_h();
        match &self.glide {
            Some((_, to, _)) if *to == target => {} // 幂等：滑行中同目标
            _ => {
                self.glide = Some((self.last_upper_h, target, now_ms));
            }
        }
    }

    /// 池高缓动活性探针（帧泵闸）：账未贴死 = true
    pub fn glide_fx_active(&self, now_ms: u64) -> bool {
        self.glide
            .is_some_and(|(_, _, start)| now_ms.saturating_sub(start) < crate::ui::cfg_page::PAN_MS)
    }

    pub fn upper_content_h(&self) -> u32 {
        self.upper_content_h
    }

    pub fn area(&self) -> &PoolRect {
        &self.area
    }

    /// 目标上池高：min(max(内容, 空占位), H/2)
    fn target_h(&self) -> u32 {
        let content = self.upper_content_h.max(POOL_EMPTY_H);
        content.min(self.area.h / 2)
    }

    /// 布局（数学钉主体）：上池高 = 缓动瞬时值（有账）或目标值直通
    /// （无账），下池 = H − 上池 − 池间距随布局数学自动互补；
    /// upper_scroll = 内容被**目标**高截断旗（吃靶不吃瞬时值——动画
    /// 中途不闪旗）
    pub fn layout(&mut self, now_ms: u64) -> DualPoolSnap {
        let target = self.target_h();
        let upper_h = match &mut self.glide {
            Some((from, to, start)) => {
                let elapsed = now_ms.saturating_sub(*start);
                if elapsed >= crate::ui::cfg_page::PAN_MS {
                    let to = *to;
                    self.glide = None; // 贴死清账
                    to
                } else {
                    let t = elapsed as f32 / crate::ui::cfg_page::PAN_MS as f32;
                    // 有符号中间量（BAR-095 咬：u32 相减在变小方向下溢
                    // 成 4e9 级巨值 → upper.h 天文数字 → band/池框错乱，
                    // 真机遥测实录 t=0.678 band=(97,175,1169,-1385067369)）
                    let from_i = i64::from(*from);
                    let to_i = i64::from(*to);
                    (from_i as f32
                        + (to_i - from_i) as f32 * crate::ui::fx_ease::ease_in_out_cubic(t))
                    .round()
                    .clamp(0.0, to_i.max(from_i) as f32) as u32
                }
            }
            None => target,
        };
        self.last_upper_h = upper_h;
        let h = self.area.h;
        let upper = PoolRect {
            x: self.area.x,
            y: self.area.y,
            w: self.area.w,
            h: upper_h,
        };
        let lower = PoolRect {
            x: self.area.x,
            y: self.area.y + i64::from(upper_h + POOL_GAP),
            w: self.area.w,
            h: h.saturating_sub(upper_h).saturating_sub(POOL_GAP),
        };
        DualPoolSnap {
            upper_scroll: self.upper_content_h > target,
            upper,
            lower,
        }
    }
}

// ---- 共享句柄（D9 同源：gate 值守倒帧与前台帧同一份双池读数）----

use std::sync::{Arc, Mutex, RwLock};

pub type SharedDualPool = Arc<Mutex<DualPool>>;

static DUAL_POOL_HANDLE: RwLock<Option<SharedDualPool>> = RwLock::new(None);

/// 注册（android_app 装配时调一次）；重注册 = 覆盖（热更核新实例）
pub fn register_dual_pool(pool: SharedDualPool) {
    *DUAL_POOL_HANDLE.write().unwrap() = Some(pool);
}

/// 读句柄（gate 值守倒帧取快照用；未注册 = None 兜底不画双池）
pub fn dual_pool_handle() -> Option<SharedDualPool> {
    DUAL_POOL_HANDLE.read().unwrap().clone()
}
