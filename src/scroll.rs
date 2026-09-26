//! scroll.rs — 触摸滚动手势状态机（A 档纯逻辑，考题 tests/scroll_spec.rs）
//!
//! 职责：把一串触摸 y 坐标翻成两种结果——「点按」（唤软键盘）或
//! 「滚动 N 行」（终端 scrollback）。像素→行的换算带余数挂账：
//! 半行半行地慢拖也必须累计成行，不能每次取整吞掉余数（慢滚就哑）。
//!
//! 方向约定（自然滚动，同手机全局手感）：手指向下拖 = 看更老的历史 =
//! 行数 delta 为正（alacritty Scroll::Delta 正数 = display_offset 增大）。

/// 点按/拖动的分界（px）：位移没超过它，松开 = 点按（唤键盘）；
/// 超过了就进入滚动模式，松手不弹键盘
pub const TAP_SLOP_PX: f64 = 24.0;

/// 拖滚增益（2026-09-26 用户拍板「2 倍滚动比：我移动 10px 页面滚 20px」）：
/// 只乘手指位移，不动 slop 门（阈值是手指物理量）；像素/行级/滚轮三通道
/// 共用，甩尾速度采样吃增益后的位移故初速同倍跟随
pub const DRAG_GAIN: f64 = 2.0;

/// SGR 1006 滚轮事件序列（BAR-016）：全屏 TUI（tmux/kimicode 开了鼠标上报）
/// 时，滚屏不滚本地（alt screen 没历史），翻成滚轮事件发给 PTY 让对方滚。
/// view_older=true（手指下拖看历史）= wheel up = button 64；false = 65。
/// 坐标 1-based（终端协议惯例）。滚轮只有按下（M），没有抬起（m）
pub fn wheel_seq(view_older: bool, col: u32, row: u32) -> String {
    let btn = if view_older { 64 } else { 65 };
    format!("\x1b[<{btn};{col};{row}M")
}

/// 一次触摸的滚动状态机。cell_h 在建机时快照（会话期间格高不变）
pub struct TouchScroll {
    start_y: f64,
    last_y: f64,
    /// 已换算成行后剩下的零头（px，带符号）——慢拖的命根
    pending_px: f64,
    /// 滚轮 tick 挂账零头（鼠标上报通道专用，BAR-151）——与 pending_px
    /// 分账：行级通道 moved 吃前者，滚轮通道 wheel_ticks 吃后者
    wheel_pending_px: f64,
    /// 甩尾速度采样（px/帧，带符号；moved_px_at 逐事件更新）——抬手
    /// 交接 Fling 的初速
    vel: f64,
    /// 末次采样时刻（ms，调用方时钟——boot_ms 单源；0 = 未喂时钟）
    last_t_ms: f64,
    /// 是否已越过点按阈值进入滚动模式
    dragging: bool,
    cell_h: f64,
}

impl TouchScroll {
    pub fn new(start_y: f64, cell_h: f64) -> Self {
        Self {
            start_y,
            last_y: start_y,
            pending_px: 0.0,
            wheel_pending_px: 0.0,
            vel: 0.0,
            last_t_ms: 0.0,
            dragging: false,
            cell_h: cell_h.max(1.0),
        }
    }

    /// 手指移到 y：返回本次应滚动的行数（带符号，0 = 不到一行）。
    /// 手指向下（y 增大）= 看历史 = 正数
    pub fn moved(&mut self, y: f64) -> i32 {
        if !self.dragging {
            // 含边判定(AOSP ViewConfiguration touchSlop 惯例:恰好到位
            // 仍未开始滚动)——2026-08-27 变异抽检首只存活体实锤此处
            // 无边界钉(< vs <= 无人判卷),换含边语义+补钉
            if (y - self.start_y).abs() <= TAP_SLOP_PX {
                self.last_y = y;
                return 0; // 阈值内：还在点按嫌疑期，不滚
            }
            self.dragging = true;
        }
        self.pending_px += (y - self.last_y) * DRAG_GAIN;
        self.last_y = y;
        let lines = (self.pending_px / self.cell_h).trunc() as i32;
        self.pending_px -= f64::from(lines) * self.cell_h; // 余数挂账
        lines
    }

    /// 像素级滚动通道（2026-09-24 用户拍板「滚动的像素级」）：与 moved
    /// 同一只 slop 门、同一份 last_y，但**不取整不挂账**——本次位移原样
    /// 出 px（带符号，手指向下 = 正 = 看历史）。零头的累计/借还在
    /// TermView::scroll_px 的分数视口里。行级通道 moved 一行不动
    /// （旧保底，设置页可切回——一次触摸内不换道，切换发生在两次触摸间）
    pub fn moved_px(&mut self, y: f64) -> f64 {
        if !self.dragging {
            let off = y - self.start_y;
            if off.abs() <= TAP_SLOP_PX {
                self.last_y = y;
                return 0.0;
            }
            self.dragging = true;
            // 越阈第一笔：slop 段不计入（从阈值边界起算，与 moved 的
            // 挂账语义不同——像素通道一滴零头都是钱，slop 段是点按的）
            let d = (off - off.signum() * TAP_SLOP_PX) * DRAG_GAIN;
            self.last_y = y;
            return d;
        }
        let d = (y - self.last_y) * DRAG_GAIN;
        self.last_y = y;
        d
    }

    /// 手指抬起：true = 全程没过阈值，算点按（调用方唤键盘）
    pub fn was_tap(&self) -> bool {
        !self.dragging
    }

    /// 滚轮 tick 换算（鼠标上报通道，BAR-151 仪器定罪：旧实现逐事件
    /// trunc，慢拖每笔 <cell_h 的位移余数全吞——真机实录 d=16/20/20
    /// ticks 恒 0，tmux 滚动整只哑掉）：与 moved_px 同 slop 门同
    /// last_y，但位移先挂账再取整——余数不吞，慢拖累计成 tick；
    /// 往返借还对称（净位移多少就净出多少 tick，零头不造不吞）。
    pub fn wheel_ticks(&mut self, y: f64, cell_h: f64) -> i32 {
        let d = self.moved_px(y);
        if d == 0.0 {
            return 0;
        }
        let ch = cell_h.max(1.0);
        self.wheel_pending_px += d;
        let ticks = (self.wheel_pending_px / ch).trunc() as i32;
        self.wheel_pending_px -= f64::from(ticks) * ch; // 余数挂账
        ticks
    }

    /// 滚轮挂账零头读数（考题/[scroll] 遥测判卷用；带符号 px）
    pub fn wheel_pending(&self) -> f64 {
        self.wheel_pending_px
    }
}

// ---------- 惯性甩尾（2026-09-25 用户拍板「工业级滑动」）----------
//
// 参数直译 kfmv4 文件树 canvas-scroll.ts 的实测手感（358 行实机迭代
// 产物，不重新发明）：速度采样 = 事件位移/事件间隔 × 16 × 1.7（换算成
// px/帧@60fps 并加 1.7 倍甩尾增益）；抬手后每帧 ×0.96 衰减；启动阈
// 0.5 / 停止阈 0.3（px/帧）；新触摸落地即取消（归 android_app 接线）。

/// 甩尾增益（kfmv4 canvas-scroll 实测：裸速太「粘」，×1.7 才是手机手感）
pub const FLING_BOOST: f64 = 1.7;
/// 每帧衰减率（kfmv4 实测：0.96 = 滑得快衰减也快的自然阻尼）
pub const FLING_DECAY: f64 = 0.96;
/// 甩尾启动阈（px/帧）：低于它就是「停住了才松手」，不甩（kfmv4 同款 0.5）
pub const FLING_START_MIN: f64 = 0.5;
/// 甩尾燃尽阈（px/帧）：低于它剩余位移不足 1px/3 帧，停（kfmv4 同款 0.3）
pub const FLING_STOP_MIN: f64 = 0.3;
/// 帧尺（ms）：速度/衰减的基准帧长（60fps）
pub const FLING_FRAME_MS: f64 = 16.667;

/// 一次甩尾的燃尽状态机。v 带符号（与 moved_px 同约定：正 = 看历史）。
/// A 档纯逻辑，考题 tests/scroll_spec.rs
pub struct Fling {
    v: f64, // px/帧（FLING_FRAME_MS 基准）
}

impl Fling {
    /// 初速建机（px/帧，带符号；fling_on_release 与考题同用一入口）
    pub fn new(v: f64) -> Self {
        Self { v }
    }

    /// 当前速度读数（px/帧，带符号；考题/遥测用）
    pub fn velocity(&self) -> f64 {
        self.v
    }

    /// 推进 dt 毫秒（帧泵逐圈喂真实间隔，4ms 降频泵也等比折帧）：
    /// 本段位移 = 区间几何级数 v×(1-D^f)/(1-D)（D=DECAY f=帧数——
    /// 与逐帧「付位移再衰减」严格等值，双帧一步 ≡ 单帧两步），
    /// 随后速度 ×D^f；衰减后 |v| < STOP = 燃尽（None——本段位移
    /// 照付，尾巴不吞）
    pub fn step(&mut self, dt_ms: f64) -> Option<f64> {
        if dt_ms <= 0.0 {
            return Some(0.0);
        }
        let frames = dt_ms / FLING_FRAME_MS;
        let decay = FLING_DECAY.powf(frames);
        let d = self.v * (1.0 - decay) / (1.0 - FLING_DECAY);
        self.v *= decay;
        if self.v.abs() < FLING_STOP_MIN {
            return None;
        }
        Some(d)
    }
}

/// BAR-158 切入方向闸（2026-09-26 field-reports 实录定罪：追底态
/// 「切入(推流画布) 补滚 -167.1px → 触底自动回 live」成对连发 =
/// 用户实报「追底态继续下滚页面闪一下」的病灶本体）：浏览挂账
/// 只许朝历史方向（d>0）净积压。追底态朝 live 方向（手指上推
/// d<0）的位移钳到 0——旧制照单全收，切入补滚负值被 scroll_px
/// 贴底钳回 offset=0，下一笔移动立刻「触底自动回 live」退场，
/// 切入→闪退 = 一闪。返回挂账新值：调用方只在 >0 时才许切入浏览。
/// A 档纯逻辑，考题 tests/scroll_spec.rs
pub fn browse_pending_gate(pending: f64, d: f64) -> f64 {
    (pending + d).max(0.0)
}

impl TouchScroll {
    /// moved_px 的计时变体（惯性采样唯一入口）：同 slop 门同位移语义，
    /// 顺手采样甩尾速度 vel = d/dt × FRAME × BOOST（kfmv4 直译：逐事件
    /// 瞬时采样——16ms 帧周期下事件即帧，无需平滑窗）。dt≤0（同帧多
    /// 事件/未喂时钟）不采样，速度沿用上笔
    pub fn moved_px_at(&mut self, y: f64, t_ms: f64) -> f64 {
        let d = self.moved_px(y);
        let dt = t_ms - self.last_t_ms;
        if dt > 0.0 && self.dragging {
            self.vel = d / dt * FLING_FRAME_MS * FLING_BOOST;
        }
        self.last_t_ms = t_ms;
        d
    }

    /// 抬手交接甩尾（调用方：像素车道的帧泵）：点按/速度低于启动阈
    /// = None（「停住了才松手」不甩）；否则 Fling 带着末速出发
    pub fn fling_on_release(&self) -> Option<Fling> {
        if !self.dragging || self.vel.abs() < FLING_START_MIN {
            return None;
        }
        Some(Fling { v: self.vel })
    }
}
