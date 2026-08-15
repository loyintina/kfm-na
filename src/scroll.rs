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
            if (y - self.start_y).abs() < TAP_SLOP_PX {
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

    /// 手指抬起：true = 全程没过阈值，算点按（调用方唤键盘）
    pub fn was_tap(&self) -> bool {
        !self.dragging
    }
}
