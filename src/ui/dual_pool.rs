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
//! 十五修增订（2026-09-14，宪法 §五 池区动画条款）：上池高度弹簧——
//! 目标高变化即从当前高度重定基续弹（fx_spring 欠阻尼同核 ≈350ms），
//! 下池随布局数学自动互补；首喂/冷启动直通。layout 吃 now_ms（壳喂
//! report::boot_ms 同钟）。
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

/// 双池布局状态：上池内容高 + 可用区 + 高度弹簧。壳层持有一份（配置卡
/// 常驻，与标签栏同规）；内容高由池页内容侧喂（骨架期恒 0 = 空占位）。
/// 高度弹簧（十五修宪法 §五 池区动画条款）：目标高变化即从当前高度
/// 重定基续弹（fx_spring 欠阻尼同核 ≈350ms 收敛），下池随布局数学自动
/// 互补；首喂/冷启动直通不补演
pub struct DualPool {
    upper_content_h: u32,
    area: PoolRect,
    /// 高度弹簧：起点高（重定基时 = 当时弹簧位置）
    h_from: f32,
    h_start_ms: u64,
    /// 上次布局的目标高（目标变化侦测 = 与 target_h() 比对）
    h_target: u32,
    /// 首喂直通旗（冷启动不补演一场）
    h_primed: bool,
}

/// 布局快照（涂装唯一读数口；upper_scroll = 上池内容被半高钳截断旗，
/// 骨架期只读不滚）
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
            h_from: 0.0,
            h_start_ms: 0,
            h_target: 0,
            h_primed: false,
        }
    }

    pub fn set_viewport(&mut self, screen_w: u32, screen_h: u32, bottom_inset: u32) {
        self.area = pool_area(screen_w, screen_h, bottom_inset);
    }

    pub fn set_upper_content_h(&mut self, h: u32) {
        self.upper_content_h = h;
    }

    pub fn upper_content_h(&self) -> u32 {
        self.upper_content_h
    }

    pub fn area(&self) -> &PoolRect {
        &self.area
    }

    /// 目标上池高（弹簧的靶）：min(max(内容, 空占位), H/2)
    fn target_h(&self) -> u32 {
        let content = self.upper_content_h.max(POOL_EMPTY_H);
        content.min(self.area.h / 2)
    }

    /// 高度弹簧活性探针（帧泵闸）：未收敛 = true（首喂前恒 false）
    pub fn h_fx_active(&self, now_ms: u64) -> bool {
        if !self.h_primed {
            return false;
        }
        let target = self.h_target as f32;
        crate::ui::fx_spring::spring_pos(
            self.h_from,
            target,
            now_ms.saturating_sub(self.h_start_ms),
        ) != target
    }

    /// 布局（数学钉主体）：上池高 = 弹簧当前值（靶 = min(max(内容, 空
    /// 占位), H/2)），下池 = H − 上池 − 池间距随动画自动互补；
    /// upper_scroll = 内容被目标高截断旗（吃靶不吃瞬时值——动画中途
    /// 不闪旗）。目标变化（内容进出/视口变）即从当前高度重定基续弹
    pub fn layout(&mut self, now_ms: u64) -> DualPoolSnap {
        let target = self.target_h();
        if !self.h_primed {
            // 首喂直通：冷启动不补演
            self.h_primed = true;
            self.h_from = target as f32;
            self.h_target = target;
            self.h_start_ms = now_ms;
        } else if target != self.h_target {
            // 重定基：从当前弹簧位置续弹（来回狂点不跳变）
            self.h_from = crate::ui::fx_spring::spring_pos(
                self.h_from,
                self.h_target as f32,
                now_ms.saturating_sub(self.h_start_ms),
            );
            self.h_target = target;
            self.h_start_ms = now_ms;
        }
        let upper_h = crate::ui::fx_spring::spring_pos(
            self.h_from,
            self.h_target as f32,
            now_ms.saturating_sub(self.h_start_ms),
        )
        .round()
        .max(0.0) as u32;
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
