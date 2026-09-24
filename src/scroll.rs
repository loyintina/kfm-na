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
        self.pending_px += y - self.last_y;
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
            let d = off - off.signum() * TAP_SLOP_PX;
            self.last_y = y;
            return d;
        }
        let d = y - self.last_y;
        self.last_y = y;
        d
    }

    /// 手指抬起：true = 全程没过阈值，算点按（调用方唤键盘）
    pub fn was_tap(&self) -> bool {
        !self.dragging
    }
}
