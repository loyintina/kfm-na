//! ai_page.rs — AI 对话页视口状态机（期 0④：滚动/追底，2026-09-04）。
//!
//! 语义三件套（微信/GPT 对话页同款直觉）：
//! - 默认追底：新内容（AI 流式输出）来了自动贴底；
//! - 上滑（看更早的消息）取消追底，视口定在原地不动；
//! - 下滑回到最底 → 自动恢复追底。
//!
//! 单位是「展示行」（折行后的行，不是消息条）——渲染方每帧把布局
//! （总行数/一屏行数）写回，手势钳制与渲染用同一份布局（眼手同尺）。
//! 本模块是纯逻辑（A 档），不碰字体不碰像素——行高换算在手势侧
//! （android_app 用 termview::AI_PAGE_LINE_H），布局测量在渲染侧
//! （termview build_ai_rows）。

/// 视口状态：距底行数 + 追底标志 + 布局缓存
#[derive(Default)]
pub struct AiPageScroll {
    /// 距底行数（0 = 贴底）。!follow 时新内容来了视口不动（消息
    /// 从底部顶出去——与所有主流对话 App 一致）
    offset_rows: u32,
    /// 追底态：true = 渲染恒贴底（offset 视为 0）
    follow_tail: bool,
    /// 布局缓存（渲染方 sync_layout 写回）：钳制上界的数据源
    total_rows: u32,
    fit_rows: u32,
}

impl AiPageScroll {
    pub fn new() -> Self {
        Self {
            offset_rows: 0,
            follow_tail: true,
            total_rows: 0,
            fit_rows: 0,
        }
    }

    /// 手指拖动换算来的行增量（有符号：正 = 看更早 = 内容上移）。
    /// 拖过 0 钳住；回到 0 即恢复追底。
    pub fn drag_rows(&mut self, delta: i32) {
        let max = self.total_rows.saturating_sub(self.fit_rows);
        let next = self.offset_rows as i64 + i64::from(delta);
        self.offset_rows = next.clamp(0, i64::from(max)) as u32;
        self.follow_tail = self.offset_rows == 0;
    }

    /// 布局写回（渲染方每帧）：总行数缩水时 offset 不许悬空；
    /// 追底态恒贴底（新内容自动跟随）
    pub fn sync_layout(&mut self, total: u32, fit: u32) {
        self.total_rows = total;
        self.fit_rows = fit;
        let max = total.saturating_sub(fit);
        if self.offset_rows > max {
            self.offset_rows = max;
        }
        if self.follow_tail {
            self.offset_rows = 0;
        }
    }

    /// 渲染用距底行数（追底态恒 0）
    pub fn offset(&self) -> u32 {
        if self.follow_tail {
            0
        } else {
            self.offset_rows
        }
    }

    /// 追底态读数（观测/判卷用）
    pub fn follow(&self) -> bool {
        self.follow_tail
    }
}

/// 思考块可见窗（期 0④½，2026-09-04 用户拍板：思考限制在两到三行里
/// 自己滚动——块高恒 ≤3 行，流式增长时窗口跟尾 = 内容自动向上滚）。
/// 入 = 思考折行后的总行数，出 = 可见行区间（尾部 ≤3 行）。
/// 纯函数（A 档）：钳制计数与「尾随非头部」都在这里钉。
pub fn thinking_window(total_wrapped: usize) -> std::ops::Range<usize> {
    let start = total_wrapped.saturating_sub(3);
    start..total_wrapped
}
