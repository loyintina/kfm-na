//! input_bar.rs — 全局输入栏状态核（期 0 组件三；A 档纯逻辑，考题
//! tests/input_bar_spec.rs）。规格书 docs/active/ai-presence.md §二/§五。
//!
//! 常驻 chrome：压底紧贴键盘（快捷键行上移一层让位），任何会话下都在。
//! 焦点二态：终端 / 输入栏——点文本区聚焦（壳层顺带弹键盘），Esc 或点
//! 终端区失焦；聚焦时键盘按键全归输入栏（分流在壳层 drain_ime_inject），
//! Enter = 发送（壳层把 enter() 取走的文本推进 AiSendSink）。
//!
//! v1 从简：无光标移动，文本只追加+退格；发送后保持聚焦（手机聊天惯例）。
//! 形态判别同 AiPresenceState：Sync 内部可变（Mutex），共享实例直挂服务键。

use std::sync::Mutex;

/// 栏带高（px，物理像素）= 文本区 156 + 上下留白（kfmv4 参考样式复刻：
/// 文本区浮在带内，不贴带边——2026-08-31 样式修订，参考图实测比）
/// 这是单行（默认）带高；多行 textarea 用 height_for_lines() 算当前带高。
pub const HEIGHT_PX: u32 = 220;
/// textarea 行数上限：超出行数时栏不再长高，文本区内部滚动（尾锚显最后几行）
pub const MAX_LINES: u32 = 3;
/// 每多一行带高增量（px）= 行高：字号 ~40px × 1.5（kfmv4 line-height 直译）
pub const LINE_STEP_PX: u32 = 63;

/// 行数 → 带高（px）。0 行按 1 行计（空栏也是一行高）；超 MAX_LINES 封顶。
/// 覆盖式悬浮：带长高只把栏带向上浮盖终端底部行，终端网格几何不动。
pub fn height_for_lines(lines: u32) -> u32 {
    let n = lines.clamp(1, MAX_LINES);
    HEIGHT_PX + (n - 1) * LINE_STEP_PX
}

/// 文本可用宽（px）：屏宽 → 文本区宽（减左右留白/发送钮/缝隙）→ 减左内缩
/// 40 + 右留白 12 + 起笔 18。量行（termview::bar_text_lines）与渲染折行
/// 共用这一把尺（眼手同尺）。窗太窄画不下 = None
pub fn text_avail_w(buf_w: u32) -> Option<f32> {
    let field_w = buf_w.checked_sub(2 * MARGIN_X_PX + SEND_W_PX + GAP_PX)?;
    Some(field_w.saturating_sub(70) as f32)
}
/// 发送钮宽（px）：右端固定宽圆角方块，拇指可击
pub const SEND_W_PX: u32 = 140;
/// 栏左右离屏边留白（px）——参考样式：文本区/发送钮都不贴屏边
pub const MARGIN_X_PX: u32 = 60;
/// 文本区与发送钮之间的缝隙（px）
pub const GAP_PX: u32 = 40;

/// 命中部位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHit {
    /// 文本区（点 = 聚焦+弹键盘）
    Field,
    /// 发送钮（点 = enter 等价：取文发送）
    Send,
}

/// 栏带 = 屏底 - 键盘 inset 之上一条带（keybar 同一把尺，在其之下一层）。
/// bar_h = 当前带高（随行数长高的 textarea：调用方传 height_for_lines(当前行数)）
pub fn in_bar(y: f64, win_h: u32, ime_bottom: u32, bar_h: u32) -> bool {
    let Some(bottom) = win_h.checked_sub(ime_bottom) else {
        return false;
    };
    let Some(top) = bottom.checked_sub(bar_h) else {
        return false;
    };
    y >= f64::from(top) && y < f64::from(bottom)
}

/// 窗口坐标 → 命中部位；栏外（上方终端区/被键盘盖住的屏底）→ None。
/// 发送钮带 = 右端留白内推 MARGIN_X_PX 的 SEND_W_PX 一条；其余栏内都算
/// 文本区（缝隙/留白给拇指容错，点了聚焦不亏）
pub fn hit(x: f64, y: f64, win_w: u32, win_h: u32, ime_bottom: u32, bar_h: u32) -> Option<BarHit> {
    if !in_bar(y, win_h, ime_bottom, bar_h) || x < 0.0 || x >= f64::from(win_w) {
        return None;
    }
    let send_left = win_w.checked_sub(MARGIN_X_PX)?.checked_sub(SEND_W_PX)?;
    if x >= f64::from(send_left) {
        Some(BarHit::Send)
    } else {
        Some(BarHit::Field)
    }
}

/// 状态快照（绘制/stats/探针回执的同源读数）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarSnap {
    pub text: String,
    pub focused: bool,
    /// 当前折行数（渲染层量宽断行后写回，眼手同尺单源）
    pub lines: u32,
}

struct Inner {
    text: String,
    focused: bool,
    /// 当前折行数（渲染层量宽断行后写回；触摸命中/闸门 dump 读同一份）
    lines: u32,
    /// 发送出口（壳层装配时装入：接脑 + run_start/run_end）。
    /// 触摸发送钮 / IME Enter / 闸门注入 submit 全走这一个口（D9 同源）
    sender: Option<Sender>,
}

/// 发送回调：取走的文本推进 AiSendSink（期 0 = 壳层脑装配闭包）
pub type Sender = std::sync::Arc<dyn Fn(String) + Send + Sync>;

/// 全局输入栏状态核。共享实例直挂服务键（插件 src/plugins/input_bar.rs），
/// 人走触摸、AI 走注入通道，同一状态核同一套考题（D9 同源）。
pub struct InputBarState {
    inner: Mutex<Inner>,
}

impl InputBarState {
    pub fn new() -> Self {
        InputBarState {
            inner: Mutex::new(Inner {
                text: String::new(),
                focused: false,
                lines: 1,
                sender: None,
            }),
        }
    }

    pub fn snap(&self) -> BarSnap {
        let g = self.inner.lock().unwrap();
        BarSnap {
            text: g.text.clone(),
            focused: g.focused,
            lines: g.lines,
        }
    }

    /// 渲染层写回当前折行数（量宽断行的唯一落点；0 按 1 计）
    pub fn set_lines(&self, lines: u32) {
        self.inner.lock().unwrap().lines = lines.max(1);
    }
    pub fn lines(&self) -> u32 {
        self.inner.lock().unwrap().lines
    }

    pub fn focus(&self) {
        self.inner.lock().unwrap().focused = true;
    }
    pub fn unfocus(&self) {
        self.inner.lock().unwrap().focused = false;
    }
    pub fn is_focused(&self) -> bool {
        self.inner.lock().unwrap().focused
    }

    /// 追加文本（IME commitText / 物理字符键；v1 无光标只追加）
    pub fn insert_text(&self, s: &str) {
        self.inner.lock().unwrap().text.push_str(s);
    }

    /// 退格删一整字符（char 边界安全——中文不是撕字节）
    pub fn backspace(&self) {
        self.inner.lock().unwrap().text.pop();
    }

    /// 清空栏（注入通道 clear；保留聚焦态）
    pub fn clear(&self) {
        self.inner.lock().unwrap().text.clear();
    }

    /// Enter = 发送：取走文本（清空栏），空文本 = None（无发送）。
    /// 保持聚焦（发送后继续聊，手机聊天惯例）
    pub fn enter(&self) -> Option<String> {
        let mut g = self.inner.lock().unwrap();
        if g.text.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut g.text))
    }

    /// 装入发送出口（壳层装配插件后调一次；重复装入覆盖——
    /// 热更换脑时后装的就是要盖的）
    pub fn install_sender(&self, sender: Sender) {
        self.inner.lock().unwrap().sender = Some(sender);
    }

    /// 提交 = enter + 推进发送口。空文本/未装出口都只取文不派送
    /// （未装出口 = 脑没装配好，文本照收不丢——发送方负责兜底呈现）
    pub fn submit(&self) -> Option<String> {
        let (text, sender) = {
            let mut g = self.inner.lock().unwrap();
            if g.text.is_empty() {
                return None;
            }
            (std::mem::take(&mut g.text), g.sender.clone())
        };
        if let Some(s) = sender {
            s(text.clone());
        }
        Some(text)
    }
}

impl Default for InputBarState {
    fn default() -> Self {
        Self::new()
    }
}
