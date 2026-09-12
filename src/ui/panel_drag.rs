//! panel_drag.rs — 面板跟手拖拽状态机（核心层纯逻辑，A 档钉）。
//!
//! 模式（interactive gesture-driven transition，2026-09-11 用户拍板手势
//! 升级）：拖拽期面板位置绑手指位移 ×DRAG_GAIN（手指不从屏缘起手，
//! 增益 2 补偿；不绑时间不绑曲线），松手瞬间按「进度+甩速」裁决完成/
//! 取消；收尾动画不在本册——壳层用缝 replay 踢
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
/// 跟手增益（2026-09-11 用户拍板）：手指不从屏边缘起手，1:1 全程要拖
/// 整个屏宽不现实——面板位移 = 手指位移 × 2（手指 10px 面板 20px，
/// 半程手势走完全程）
pub const DRAG_GAIN: f64 = 2.0;

/// 拖拽角色（锁定瞬间仲裁，四公民全表见考题钉②；2026-09-12 三缘语义——
/// 左缘家=文件树（路由），右缘家=解析页（解析器家族）。设置页（配置面板）
/// 是独立第四页：齿轮钮唯一召唤口、右滑唯一关闭口，永不走手势召唤——
/// 故 SummonConfig 变体删除，原位是 SummonParser）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragRole {
    /// 右滑推配置页回右缘：偏移从 0 → 屏宽（设置页唯一关闭路径）
    DismissConfig,
    /// 右滑拉文件树进场：偏移从 屏宽（屏外左）→ 0（靠泊）
    SummonFileTree,
    /// 左滑推文件树回左缘：偏移从 0 → 屏宽
    DismissFileTree,
    /// 左滑拉解析页进场：偏移从 屏宽（屏外右）→ 0（靠泊）
    SummonParser,
    /// 右滑推解析页回右缘：偏移从 0 → 屏宽
    DismissParser,
}

/// 锁定瞬间的栈顶读数（角色仲裁的唯一栈依赖）：四家面板或都不是。
/// 2026-09-12 四公民加 Parser（三缘语义：右缘=解析器家族）。
/// Ai 不参与抽屉仲裁（垂直缝光球家），与空栈同归 Other
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragTop {
    Config,
    FileTree,
    Parser,
    Other,
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
    /// top = 锁定瞬间的栈顶读数（角色仲裁的唯一栈依赖——四公民一滑一义：
    /// 左滑在文件树顶=推回文件树、在配置/解析顶=空操作、其余=召唤解析页；
    /// 右滑在配置/解析顶=推回、在文件树顶=空操作、其余=召唤文件树）。
    /// 偏移语义统一为「距靠泊的px距离」∈[0,w]：壳层按家
    /// 折算符号（右缘家 +off 屏外右 / 文件树家 -off 屏外左）
    pub fn on_move(
        &mut self,
        x: f64,
        y: f64,
        ms: u64,
        screen_w: f32,
        top: DragTop,
    ) -> Option<(DragRole, f32)> {
        self.push_sample(ms, x);
        if self.role.is_none() {
            let dx = x - self.start_x;
            let dy = y - self.start_y;
            if dx.abs() < DRAG_LOCK_PX || dx.abs() <= DRAG_DIR_LOCK * dy.abs() {
                return None; // 未过阈值或斜率不够横——让路
            }
            // 角色仲裁（一滑一义 §五B 四公民·三缘语义）
            let role = if dx < 0.0 {
                match top {
                    DragTop::FileTree => DragRole::DismissFileTree,
                    DragTop::Config | DragTop::Parser => return None, // 右缘本家已在顶：左滑空操作
                    DragTop::Other => DragRole::SummonParser,
                }
            } else {
                match top {
                    DragTop::Config => DragRole::DismissConfig,
                    DragTop::Parser => DragRole::DismissParser,
                    DragTop::FileTree => return None, // 本家已在顶：右滑空操作
                    DragTop::Other => DragRole::SummonFileTree,
                }
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

    /// 跟手映射：位移先乘 DRAG_GAIN（手指不从屏缘起手的补偿），再折算成
    /// 「距靠泊的距离」：召唤 = 从 w 递减（手指朝来向的反方向拖 = 拉近），
    /// 推回 = 从 0 递增。钳 [0, w]：面板不许越过靠泊位飞出去，也不许拖
    /// 出屏外更深。四家共用同一把尺，符号折算在壳层（右缘家 +/文件树家 -）
    fn map_offset(&self, x: f64, w: f32) -> f32 {
        let d = ((x - self.lock_x) * DRAG_GAIN) as f32;
        match self.role {
            Some(DragRole::SummonParser) => (w + d).clamp(0.0, w),
            Some(DragRole::DismissConfig) | Some(DragRole::DismissParser) => d.clamp(0.0, w),
            Some(DragRole::SummonFileTree) => (w - d).clamp(0.0, w),
            Some(DragRole::DismissFileTree) => (-d).clamp(0.0, w),
            None => 0.0,
        }
    }

    /// 完成方向进度（0..1）：召唤=开度（w-off)/w；推回=推出度 off/w
    fn completion_progress(&self, w: f32) -> f32 {
        if w <= 0.0 {
            return 0.0;
        }
        match self.role {
            Some(DragRole::SummonFileTree) | Some(DragRole::SummonParser) => {
                ((w - self.offset) / w).clamp(0.0, 1.0)
            }
            Some(DragRole::DismissConfig)
            | Some(DragRole::DismissFileTree)
            | Some(DragRole::DismissParser) => (self.offset / w).clamp(0.0, 1.0),
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
            Some(DragRole::SummonParser) => -v, // 左移 = 朝完成
            Some(DragRole::DismissConfig) | Some(DragRole::DismissParser) => v,
            Some(DragRole::SummonFileTree) => v, // 右移 = 朝完成
            Some(DragRole::DismissFileTree) => -v,
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
