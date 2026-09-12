//! dual_pool.rs — 双池骨架（主题宪法 §五「kfmv4 系列的灵魂」，核心层
//! 纯布局数学，A 档钉）。
//!
//! 条款兑现：首行布局区之下两个二级卡片，动态高度——
//!   上池高 = min(上池内容高, H/2)（内容几行跟几行；多了不越半，超出
//!     部分上池内滚动——滚动是内容的事，骨架只报 upper_scroll 旗）；
//!   下池高 = H − 上池高（恒撑满剩余，内容少也撑到卡底）；
//!   两池直接衔接无分隔线（§四 二层双框），与首行留 1 格标准间距；
//!   上池空也占位（A2 拍板）：空内容按 2 格占位高计——池页切换时双池
//!     高度生长动画需要它，骨架期占位就是全部内容。
//!
//! 下池内容高不参与布局（下池恒 = H − 上池高）——签名即契约。
//!
//! 时间戳/墙钟：本册零墙钟，纯函数+状态。

use crate::termview::{AI_PAGE_FRAME_MARGIN, AI_PAGE_FRAME_W, CELL_H};
use crate::ui::tab_bar::{TAB_ROW_H, content_origin, content_viewport_w};

/// 首行布局区 → 上池标准间距 = 1 格（§四 二层双框「与首行留标准间距」）
pub const POOL_TOP_GAP: u32 = CELL_H;
/// 上池空占位高 = 2 格（A2 拍板：占位不消失；2 格 = 一行池行高，
/// §五 池行双行结构 = 2 格同尺）
pub const POOL_EMPTY_H: u32 = CELL_H * 2;

/// 池框矩形（x/y 可随面板平移由涂装侧加偏移；涂装/命中共用——眼手同尺）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolRect {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// 双池可用区（首行之下 → 卡底内缘）：x/w 与标签栏内容带同源（眼手
/// 同尺：原点 43、右内缘 −37）；底内缘 = 屏底 − (MARGIN + 细缘 + 1 格)，
/// 与内容原点 y = 55 上下对称
pub fn pool_area(screen_w: u32, screen_h: u32) -> PoolRect {
    let (ox, oy) = content_origin();
    let y = oy + TAB_ROW_H + POOL_TOP_GAP;
    let bottom = screen_h.saturating_sub(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_H);
    PoolRect {
        x: i64::from(ox),
        y: i64::from(y),
        w: content_viewport_w(screen_w),
        h: bottom.saturating_sub(y),
    }
}

/// 双池布局状态：上池内容高 + 可用区。壳层持有一份（配置卡常驻，
/// 与标签栏同规）；内容高由池页内容侧喂（骨架期恒 0 = 空占位）
pub struct DualPool {
    upper_content_h: u32,
    area: PoolRect,
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
            area: pool_area(screen_w, screen_h),
        }
    }

    pub fn set_viewport(&mut self, screen_w: u32, screen_h: u32) {
        self.area = pool_area(screen_w, screen_h);
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

    /// 布局（数学钉主体）：上池 = min(max(内容, 空占位), H/2)，
    /// 下池 = H − 上池 直接衔接；upper_scroll = 内容被截断旗
    pub fn layout(&self) -> DualPoolSnap {
        let h = self.area.h;
        let half = h / 2;
        let content = self.upper_content_h.max(POOL_EMPTY_H);
        let upper_h = content.min(half);
        let upper = PoolRect {
            x: self.area.x,
            y: self.area.y,
            w: self.area.w,
            h: upper_h,
        };
        let lower = PoolRect {
            x: self.area.x,
            y: self.area.y + i64::from(upper_h),
            w: self.area.w,
            h: h - upper_h,
        };
        DualPoolSnap {
            upper_scroll: self.upper_content_h > upper_h,
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
