//! panel_drag.rs — 面板跟手拖拽状态机（核心层纯逻辑，A 档钉）。
//!
//! 模式（interactive gesture-driven transition，2026-09-11 用户拍板手势
//! 升级）：拖拽期面板位置绑手指位移（不绑时间不绑曲线），松手瞬间按
//! 「进度+甩速」裁决完成/取消；收尾动画不在本册——壳层用缝 replay 踢
//! （BAR-079 原语）从当前偏移重定基续播。
//!
//! 分层：本册只管几何与裁决（位移→锁定→偏移→裁决），栈操作（召唤/
//! 推回）归 ai_presence 状态核，壳层 android_app 按本册输出机械中继。
//! 拖拽期渲染偏移由壳层旁路缝采样直取 current_offset（直接操纵不是
//! 动画——手指停画面停，零插值滞后）。
//!
//! 时间戳：壳层喂 report::boot_ms 同钟毫秒；本册零墙钟（考题可喂假钟）。

use std::collections::VecDeque;

/// 拖拽方向锁阈值（px）：比 SWIPE_MIN_PX(90) 小——拖拽要尽早接管手势，
/// 又大于点按 slop（TAP_SLOP）不误伤点按
pub const DRAG_LOCK_PX: f64 = 24.0;
/// 方向锁斜率（与 decide_swipe 同规：|dx| > 1.8|dy| 才算横向——
/// 纵向滚屏/AI 页滚行不冲突）
pub const DRAG_DIR_LOCK: f64 = 1.8;
/// 松手裁决进度阈值：跟手进度（完成方向）过半 = 完成
pub const RELEASE_PROGRESS: f32 = 0.5;
/// 甩速阈值（px/ms，100ms 窗）：朝完成方向甩够快无视进度直接完成，
/// 反甩够快强制取消（反悔回拉的快甩场景）
pub const FLING_PX_PER_MS: f64 = 0.8;
/// 甩速采样窗（ms）：窗太长发呆期的老样本拖慢读数，太短抖
pub const VELOCITY_WINDOW_MS: u64 = 100;

/// 拖拽角色（锁定瞬间仲裁，四象限见考题钉②）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragRole {
    /// 左滑拉配置页进场：偏移从 屏宽（屏外右）→ 0（靠泊）
    SummonConfig,
    /// 右滑推配置页回右缘：偏移从 0 → 屏宽
    DismissConfig,
}

/// 松手裁决
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseDecision {
    /// 完成当前方向（召唤=留下 / 推回=出局）
    Complete,
    /// 取消（回起点：召唤拖=推回去 / 推回拖=拉回来）
    Cancel,
}

/// 一次手指接触的面板拖拽会话。未锁定 = 旁观者（只攒速度样本，
/// 不回拉面板）；锁定后逐 move 出跟手偏移
pub struct PanelDrag {
    start_x: f64,
    start_y: f64,
    /// 锁定点的 x（跟手零点：锁定阈值位移被吃掉，面板从基位起跟，
    /// 不在锁定瞬跳变 24px）
    lock_x: f64,
    role: Option<DragRole>,
    /// 当前跟手偏移 px（0=靠泊，w=屏外右）；未锁定时无意义
    offset: f32,
    /// 甩速采样窗 (ms, x)：down 起全量记（快甩手势可能从没锁定期
    /// 就起速），窗龄滑动
    samples: VecDeque<(u64, f64)>,
}

impl PanelDrag {
    pub fn new(start_x: f64, start_y: f64, ms: u64) -> Self {
        let mut samples = VecDeque::new();
        samples.push_back((ms, start_x));
        Self {
            start_x,
            start_y,
            lock_x: start_x,
            role: None,
            offset: 0.0,
            samples,
        }
    }

    /// 已锁定（接管手势中）
    pub fn locked(&self) -> bool {
        self.role.is_some()
    }

    /// 锁定角色（壳层松手裁决后的栈操作分路用）
    pub fn role(&self) -> Option<DragRole> {
        self.role
    }

    /// 当前跟手偏移（锁定期壳层旁路缝采样直读；未锁定返回 None）
    pub fn current_offset(&self) -> Option<f32> {
        self.role.map(|_| self.offset)
    }

    /// 手指移动。返回 Some((角色, 新偏移)) = 面板跟手（含锁定瞬）；
    /// None = 不是面板拖拽（壳层走原分路：纵向滚屏/点按等）。
    /// top_is_config = 当前栈顶是配置面板（角色仲裁的唯一栈依赖——
    /// v1 配置页独占横向手势；右滑家文件树落地时此参升枚举）
    pub fn on_move(
        &mut self,
        x: f64,
        y: f64,
        ms: u64,
        screen_w: f32,
        top_is_config: bool,
    ) -> Option<(DragRole, f32)> {
        self.push_sample(ms, x);
        if self.role.is_none() {
            let dx = x - self.start_x;
            let dy = y - self.start_y;
            if dx.abs() < DRAG_LOCK_PX || dx.abs() <= DRAG_DIR_LOCK * dy.abs() {
                return None; // 未过阈值或斜率不够横——让路
            }
            // 角色仲裁（一滑一义 §五B）
            let role = if dx < 0.0 {
                if top_is_config {
                    return None; // 顶已是配置：左滑 v1 空操作
                }
                DragRole::SummonConfig
            } else {
                if !top_is_config {
                    return None; // 顶非配置：右滑留给右滑家（v1 未装）
                }
                DragRole::DismissConfig
            };
            // 锁点 = 阈值跨越点（方向上的 start ± DRAG_LOCK_PX）：
            // 锁定瞬偏移 = 基位 + 超出阈值的零头，不跳变
            self.lock_x = self.start_x
                + if dx < 0.0 {
                    -DRAG_LOCK_PX
                } else {
                    DRAG_LOCK_PX
                };
            self.role = Some(role);
            self.offset = self.map_offset(x, screen_w);
            return self.role.map(|r| (r, self.offset));
        }
        self.offset = self.map_offset(x, screen_w);
        self.role.map(|r| (r, self.offset))
    }

    /// 跟手映射：召唤 = 屏宽 +（负位移）→ 减；推回 = 0 +（正位移）→ 增。
    /// 钳 [0, w]：面板不许越过靠泊位飞出左缘，也不许拖出屏外更深
    fn map_offset(&self, x: f64, w: f32) -> f32 {
        let d = (x - self.lock_x) as f32;
        match self.role {
            Some(DragRole::SummonConfig) => (w + d).clamp(0.0, w),
            Some(DragRole::DismissConfig) => d.clamp(0.0, w),
            None => 0.0,
        }
    }

    /// 完成方向进度（0..1）：召唤=开度（w-off)/w；推回=推出度 off/w
    fn completion_progress(&self, w: f32) -> f32 {
        if w <= 0.0 {
            return 0.0;
        }
        match self.role {
            Some(DragRole::SummonConfig) => ((w - self.offset) / w).clamp(0.0, 1.0),
            Some(DragRole::DismissConfig) => (self.offset / w).clamp(0.0, 1.0),
            None => 0.0,
        }
    }

    /// 末段甩速（px/ms，朝完成方向为正）：以松手时刻为准取 100ms 窗内
    /// 首末样本位移/时长（停下再松手 = 窗外老样本滤掉，读数归零退进度
    /// 判）；窗内不足两样本或零时长 = 0
    fn velocity_toward(&self, now_ms: u64) -> f64 {
        let cutoff = now_ms.saturating_sub(VELOCITY_WINDOW_MS);
        let pts: Vec<(u64, f64)> = self
            .samples
            .iter()
            .copied()
            .filter(|&(t, _)| t >= cutoff)
            .collect();
        let Some(&(t0, x0)) = pts.first() else {
            return 0.0;
        };
        let Some(&(t1, x1)) = pts.last() else {
            return 0.0;
        };
        if t1 <= t0 {
            return 0.0;
        }
        let v = (x1 - x0) / (t1 - t0) as f64; // px/ms，右为正
        match self.role {
            Some(DragRole::SummonConfig) => -v, // 左移 = 朝完成
            Some(DragRole::DismissConfig) => v,
            None => 0.0,
        }
    }

    fn push_sample(&mut self, ms: u64, x: f64) {
        self.samples.push_back((ms, x));
        while self
            .samples
            .front()
            .is_some_and(|&(t, _)| ms.saturating_sub(t) > VELOCITY_WINDOW_MS)
        {
            self.samples.pop_front();
        }
    }

    /// 松手裁决（仅锁定期有意义；未锁定被调 = Cancel 兜底）。
    /// 甩速优先于进度：物理直觉 = 用力甩过去的面板半路松手也会飞到位
    pub fn on_release(&self, now_ms: u64, screen_w: f32) -> ReleaseDecision {
        if self.role.is_none() {
            return ReleaseDecision::Cancel;
        }
        let v = self.velocity_toward(now_ms);
        if v >= FLING_PX_PER_MS {
            return ReleaseDecision::Complete;
        }
        if v <= -FLING_PX_PER_MS {
            return ReleaseDecision::Cancel;
        }
        if self.completion_progress(screen_w) >= RELEASE_PROGRESS {
            ReleaseDecision::Complete
        } else {
            ReleaseDecision::Cancel
        }
    }
}
