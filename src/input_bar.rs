//! input_bar.rs — 全局输入栏状态核（期 0 组件三；A 档纯逻辑，考题
//! tests/input_bar_spec.rs）。规格书 docs/active/ai-presence.md §二/§五。
//!
//! 常驻 chrome：压底紧贴键盘（快捷键行上移一层让位），任何会话下都在。
//! 焦点二态：终端 / 输入栏——点文本区聚焦（壳层顺带弹键盘），Esc 或点
//! 终端区失焦；聚焦时键盘按键全归输入栏（分流在壳层 drain_ime_inject），
//! Enter = 发送（壳层把 enter() 取走的文本推进 AiSendSink）。
//!
//! v1 从简：无选区无横滚，编辑 = 光标插入点（2026-08-31 升级：点按定位 +
//! 插入 + 定位柄，浏览器 textarea 行为对齐）；发送后保持聚焦（手机聊天惯例）。
//! 形态判别同 AiPresenceState：Sync 内部可变（Mutex），共享实例直挂服务键。

use std::sync::Mutex;

/// 栏带高（px，物理像素）= 文本区 156 + 上下留白（kfmv4 参考样式复刻：
/// 文本区浮在带内，不贴带边——2026-08-31 样式修订，参考图实测比）
/// 这是单行（默认）带高；多行 textarea 用 height_for_lines() 算当前带高。
pub const HEIGHT_PX: u32 = 220;
/// textarea 行数上限：超出行数时栏不再长高，文本区内部滚动（尾锚显最后几行）。
/// kfmv4 实测参照：浏览器里能见到 6~7 行（用户 2026-08-31 指认「na 只有 3 行
/// 就超了」）；na 字号大（~40px 物理 vs kfmv4 有效 ~22px），取 5 行 = 带高
/// 472px 封顶，再多就要吃掉半屏终端了
pub const MAX_LINES: u32 = 5;
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

/// 光标闪烁半周期（ms）：Android 系统输入光标节拍——亮 530 灭 530。
/// 调用方按 (boot_ms / CARET_BLINK_MS) % 2 算相位传渲染
pub const CARET_BLINK_MS: u64 = 530;

/// 长按进入选择模式的时间阈值（ms）：与 Android 系统默认值一致。
pub const SELECT_LONG_PRESS_MS: u64 = 400;

/// 选择锚点视觉尺寸（px，物理像素）：比光标定位柄小，与选区同族。
pub const ANCHOR_VISUAL_SIZE: u32 = 28;
/// 选择锚点触摸热区（px）：以锚点中心为原点的正方形边长，单指易拖。
pub const ANCHOR_HIT_SIZE: u32 = 48;

/// 视口几何（BAR-042 像素滚动；渲染与点按换算共用的纯函数——眼手同尺）。
/// 给定折行行数与 field 高，输出：条带高（N×行高）、有效像素偏移
/// （follow=尾锚=条带底贴 field 底；否则 scroll_px 钳制到
/// [0, 条带高-field_h]）、文本顶相对 field_top 的留白（条带不足一屏时居中）
pub fn viewport_geometry(
    n_lines: u32,
    field_h: u32,
    follow: bool,
    scroll_px: i32,
) -> (u32, i32, u32) {
    let strip_h = n_lines * LINE_STEP_PX;
    let max_eff = strip_h.saturating_sub(field_h) as i32;
    let eff = if follow {
        max_eff
    } else {
        scroll_px.clamp(0, max_eff)
    };
    let top_off = if strip_h < field_h {
        (field_h - strip_h) / 2
    } else {
        0
    };
    (strip_h, eff, top_off)
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

/// 当前被拖动的选择锚点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelAnchor {
    None,
    Left,
    Right,
}

/// 选择态屏幕几何（BAR-046）：锚点柄视觉中心 + 菜单气泡边界。
/// 渲染与触摸命中共用此结构，眼手同尺。
/// （2026-09-03：left/right_anchor 语义从「柄左缘 tip 点」改为「柄视觉
/// 中心」——热区中心与视觉中心同点，指按在看得见的柄上必中）
#[derive(Debug, Clone, Copy)]
pub struct BarSelectionGeometry {
    pub left_anchor: (f64, f64),
    pub right_anchor: (f64, f64),
    pub menu_x: u32,
    pub menu_y: u32,
    pub menu_w: u32,
    pub menu_h: u32,
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
    /// 光标插入点（char 下标，0=最前，len=最后）
    pub cursor: usize,
    /// 定位柄可见（点按定位后 true，打字/清空/发送收起——浏览器控件行为）
    pub handle: bool,
    /// IME 组合态文本（拼音预编辑；空串 = 无组合态）
    pub composing: String,
    /// 视口像素偏移（raw 值，可负可超；渲染侧钳制。follow=true 时忽略
    /// 此值尾锚显示）——像素级滚动：内容 1:1 跟手
    pub scroll_px: i32,
    /// 视口跟随模式：true = 尾锚（显示最后几行，打字态）；false = 用户拖动
    /// 后的固定视口
    pub follow: bool,
    /// 是否处于文本选择模式
    pub selecting: bool,
    /// 选区左锚点（char 下标，含）
    pub selection_start: usize,
    /// 选区右锚点（char 下标，不含）
    pub selection_end: usize,
}

struct Inner {
    text: String,
    focused: bool,
    /// 当前折行数（渲染层量宽断行后写回；触摸命中/闸门 dump 读同一份）
    lines: u32,
    /// 光标插入点（char 下标；插入/退格围绕它转）
    cursor: usize,
    /// IME 组合态（拼音预编辑，虚拟文本——插在 cursor 处显示但未入 text）
    composing: Option<String>,
    /// 视口像素偏移（raw；渲染钳制）与跟随模式。任何编辑操作回跟随
    scroll_px: i32,
    follow: bool,
    /// 定位柄可见（点按定位 → true；打字/清空/发送 → false）
    handle: bool,
    /// 文本选择状态（MVP：单段连续选区）
    selecting: bool,
    selection_start: usize,
    selection_end: usize,
    /// 当前被拖动的锚点（仅 selecting=true 时有效）
    sel_anchor: SelAnchor,
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
                cursor: 0,
                handle: false,
                composing: None,
                scroll_px: 0,
                follow: true,
                selecting: false,
                selection_start: 0,
                selection_end: 0,
                sel_anchor: SelAnchor::None,
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
            cursor: g.cursor,
            handle: g.handle,
            composing: g.composing.clone().unwrap_or_default(),
            scroll_px: g.scroll_px,
            follow: g.follow,
            selecting: g.selecting,
            selection_start: g.selection_start,
            selection_end: g.selection_end,
        }
    }

    // ========== 文本选择系统（BAR-046，2026-09-02） ==========

    /// 进入选择模式：先 finish 组合态，光标/定位柄转双锚点
    pub fn enter_selection(&self, pos: usize) {
        let mut g = self.inner.lock().unwrap();
        if let Some(cs) = g.composing.take() {
            let len = g.text.chars().count();
            let cur = g.cursor.min(len);
            let at = g
                .text
                .char_indices()
                .nth(cur)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            g.text.insert_str(at, &cs);
            g.cursor = cur + cs.chars().count();
        }
        let len = g.text.chars().count();
        let pos = pos.min(len);
        g.selecting = true;
        g.selection_start = pos;
        g.selection_end = pos;
        g.sel_anchor = SelAnchor::None;
        g.handle = false;
        g.follow = true;
    }

    /// 设置左锚点；禁止越过右锚点
    pub fn set_selection_start(&self, pos: usize) {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting {
            return;
        }
        let len = g.text.chars().count();
        let pos = pos.min(len);
        g.selection_start = pos.min(g.selection_end);
        g.sel_anchor = SelAnchor::Left;
        g.follow = true;
    }

    /// 设置右锚点；禁止越过左锚点
    pub fn set_selection_end(&self, pos: usize) {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting {
            return;
        }
        let len = g.text.chars().count();
        let pos = pos.min(len);
        g.selection_end = pos.max(g.selection_start);
        g.sel_anchor = SelAnchor::Right;
        g.follow = true;
    }

    /// 设置当前被拖动的锚点（触摸 Down 时调用）
    pub fn set_sel_anchor(&self, anchor: SelAnchor) {
        let mut g = self.inner.lock().unwrap();
        g.sel_anchor = anchor;
    }

    /// 退出选择模式，光标落在选区尾
    pub fn clear_selection(&self) {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting {
            return;
        }
        g.cursor = g.selection_end;
        g.selecting = false;
        g.selection_start = 0;
        g.selection_end = 0;
        g.sel_anchor = SelAnchor::None;
        g.handle = false;
    }

    /// 全选
    pub fn select_all(&self) {
        let mut g = self.inner.lock().unwrap();
        // 组合态先落字
        if let Some(cs) = g.composing.take() {
            let len = g.text.chars().count();
            let cur = g.cursor.min(len);
            let at = g
                .text
                .char_indices()
                .nth(cur)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            g.text.insert_str(at, &cs);
            g.cursor = cur + cs.chars().count();
        }
        let len = g.text.chars().count();
        g.selecting = true;
        g.selection_start = 0;
        g.selection_end = len;
        g.sel_anchor = SelAnchor::None;
        g.handle = false;
        g.follow = true;
    }

    /// 当前选区文本；无选区或空选区返回 None
    pub fn selected_text(&self) -> Option<String> {
        let g = self.inner.lock().unwrap();
        if !g.selecting || g.selection_start >= g.selection_end {
            return None;
        }
        Some(
            g.text
                .chars()
                .skip(g.selection_start)
                .take(g.selection_end - g.selection_start)
                .collect(),
        )
    }

    /// 删除选区文字；返回是否发生了删除
    pub fn delete_selection(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting || g.selection_start >= g.selection_end {
            return false;
        }
        let start = g
            .text
            .char_indices()
            .nth(g.selection_start)
            .map(|(b, _)| b)
            .unwrap_or(g.text.len());
        let end = g
            .text
            .char_indices()
            .nth(g.selection_end)
            .map(|(b, _)| b)
            .unwrap_or(g.text.len());
        g.text.replace_range(start..end, "");
        g.cursor = g.selection_start;
        g.selecting = false;
        g.selection_start = 0;
        g.selection_end = 0;
        g.sel_anchor = SelAnchor::None;
        g.handle = false;
        g.follow = true;
        true
    }

    /// 在 cursor/选区处插入文本；有选区先替换选区
    pub fn insert_or_replace(&self, s: &str) {
        let had_selection = self.delete_selection();
        let mut g = self.inner.lock().unwrap();
        g.composing = None;
        g.follow = true;
        let len = g.text.chars().count();
        g.cursor = g.cursor.min(len);
        if g.cursor >= len {
            g.text.push_str(s);
            g.cursor += s.chars().count();
        } else {
            let at = g
                .text
                .char_indices()
                .nth(g.cursor)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            g.text.insert_str(at, s);
            g.cursor += s.chars().count();
        }
        g.handle = false;
        if had_selection {
            // 替换后选区落在插入文本尾部（某些编辑器保留选区，这里先简化）
            g.selecting = false;
        }
    }

    /// 视图拖动滚动·行单位（gate scroll 指令用；1 行 = LINE_STEP_PX）。
    /// 拖动 = 脱离跟随（不自动回锚），任何编辑操作回跟随
    pub fn scroll_by(&self, lines: i32, field_h: u32) {
        self.scroll_by_px(lines * crate::input_bar::LINE_STEP_PX as i32, field_h);
    }

    /// 视口拖动滚动·像素单位（真指 1:1 跟手：dy 直接进偏移）。
    /// px 正 = 往尾部，负 = 往头部。**写入即钳制**到
    /// [0, 条带高-field_h]（BAR-041 同族教训：raw 越界累积 = 死区，
    /// 「第一下失效/比例失真」的根因）——钳制需要 field_h，调用方给
    pub fn scroll_by_px(&self, px: i32, field_h: u32) {
        let mut g = self.inner.lock().unwrap();
        let strip_h = g.lines * crate::input_bar::LINE_STEP_PX;
        let max_eff = strip_h.saturating_sub(field_h) as i32;
        if g.follow {
            // BAR-043:尾锚→手动交接必须播种。raw 语义=距头顶偏移,
            // 尾锚显示位=raw max_eff;不播种则首笔从 raw=0(头)起算瞬移
            g.scroll_px = max_eff;
        }
        g.follow = false;
        g.scroll_px = (g.scroll_px + px).clamp(0, max_eff);
    }

    /// 组合态显示文本（渲染/量行/点按换算的统一原料）：text 在 cursor 处
    /// 拼入组合文本——「所见」的单源定义，眼手同尺从这里出
    pub fn display_text(snap: &BarSnap) -> String {
        match snap.composing.is_empty() {
            true => snap.text.clone(),
            false => {
                let mut out = String::new();
                for (i, c) in snap.text.chars().enumerate() {
                    if i == snap.cursor {
                        out.push_str(&snap.composing);
                    }
                    out.push(c);
                }
                if snap.cursor >= snap.text.chars().count() {
                    out.push_str(&snap.composing);
                }
                out
            }
        }
    }

    /// IME 组合态文本入栏（setComposingText；随打随变）。空串 = 组合清空。
    /// 组合文本是虚拟的——不进 text，插在光标处显示（display_text 拼接）
    pub fn set_composing(&self, s: &str) {
        self.inner.lock().unwrap().follow = true; // 组合变化也是编辑
        self.inner.lock().unwrap().composing = if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        };
    }

    /// 组合结束（finishComposingText）：组合文本落为真字（插在光标处，
    /// 光标跟进）。无组合态 = no-op
    pub fn finish_composing(&self) {
        let mut g = self.inner.lock().unwrap();
        g.follow = true; // 落字回跟随
        if let Some(cs) = g.composing.take() {
            let len = g.text.chars().count();
            g.cursor = g.cursor.min(len);
            if g.cursor >= len {
                g.cursor = len;
                g.text.push_str(&cs);
                g.cursor = g.text.chars().count();
            } else {
                let at = g
                    .text
                    .char_indices()
                    .nth(g.cursor)
                    .map(|(b, _)| b)
                    .unwrap_or(g.text.len());
                g.text.insert_str(at, &cs);
                g.cursor += cs.chars().count();
            }
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

    /// 光标插入点（char 下标，读数）
    pub fn cursor(&self) -> usize {
        self.inner.lock().unwrap().cursor
    }

    /// 点按定位光标（触摸端把点按换算成 char 下标后走这里；越界钳到末尾）。
    /// 定位柄亮起——浏览器控件行为：点到的位置出现光标+下坠柄，打字才收。
    /// 选择模式下短按选区外 = 退出选择。
    pub fn set_cursor(&self, pos: usize) {
        let mut g = self.inner.lock().unwrap();
        // 点按定位先收组合（Android 惯例：点别处 = finishComposingText）
        if let Some(cs) = g.composing.take() {
            let len = g.text.chars().count();
            let cur = g.cursor.min(len);
            let at = g
                .text
                .char_indices()
                .nth(cur)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            g.text.insert_str(at, &cs);
        }
        g.cursor = pos.min(g.text.chars().count());
        g.handle = true;
        // 退出选择模式
        g.selecting = false;
        g.selection_start = 0;
        g.selection_end = 0;
        g.sel_anchor = SelAnchor::None;
    }

    /// 在光标/选区处插文本（IME commitText / 物理字符键）。有选区先替换选区；
    /// 打字收起定位柄；cursor 恒指「下一个字的落点」
    pub fn insert_text(&self, s: &str) {
        let mut g = self.inner.lock().unwrap();
        g.composing = None; // 组合态下来 commit：虚拟区由落字取代（IME 语义）
        g.follow = true; // 编辑回跟随
        // 有选区先删选区
        if g.selecting && g.selection_start < g.selection_end {
            let start = g
                .text
                .char_indices()
                .nth(g.selection_start)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            let end = g
                .text
                .char_indices()
                .nth(g.selection_end)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            g.text.replace_range(start..end, "");
            g.cursor = g.selection_start;
            g.selecting = false;
            g.selection_start = 0;
            g.selection_end = 0;
            g.sel_anchor = SelAnchor::None;
        }
        let len = g.text.chars().count();
        if g.cursor >= len {
            g.cursor = len; // 自愈
            g.text.push_str(s);
            g.cursor += s.chars().count();
        } else {
            let byte_at = |i: usize| {
                g.text
                    .char_indices()
                    .nth(i)
                    .map(|(b, _)| b)
                    .unwrap_or(g.text.len())
            };
            let at = byte_at(g.cursor);
            g.text.insert_str(at, s);
            g.cursor += s.chars().count();
        }
        g.handle = false; // 打字收起定位柄（浏览器控件行为）
    }

    /// 退格：有选区删选区；无选区删光标前一个字符（char 边界安全）；
    /// 组合态退格删拼音尾
    pub fn backspace(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(cs) = g.composing.take() {
            // 组合态退格删组合尾（删拼音字母，不碰已上屏字）
            g.follow = true;
            let mut cs = cs;
            cs.pop();
            g.composing = if cs.is_empty() { None } else { Some(cs) };
            return;
        }
        // 有选区：整体删除
        if g.selecting && g.selection_start < g.selection_end {
            let start = g
                .text
                .char_indices()
                .nth(g.selection_start)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            let end = g
                .text
                .char_indices()
                .nth(g.selection_end)
                .map(|(b, _)| b)
                .unwrap_or(g.text.len());
            g.text.replace_range(start..end, "");
            g.cursor = g.selection_start;
            g.selecting = false;
            g.selection_start = 0;
            g.selection_end = 0;
            g.sel_anchor = SelAnchor::None;
            g.handle = false;
            g.follow = true;
            return;
        }
        if g.cursor == 0 {
            return;
        }
        let idx = g.cursor - 1;
        let start = g
            .text
            .char_indices()
            .nth(idx)
            .map(|(b, _)| b)
            .unwrap_or(g.text.len());
        let end = g
            .text
            .char_indices()
            .nth(idx + 1)
            .map(|(b, _)| b)
            .unwrap_or(g.text.len());
        g.text.replace_range(start..end, "");
        g.cursor = idx;
        g.handle = false; // 打字（含删除）收起定位柄
        g.follow = true; // 编辑回跟随
    }

    /// 清空栏（注入通道 clear；保留聚焦态，光标/定位柄/选区一并复位）
    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap();
        g.text.clear();
        g.cursor = 0;
        g.handle = false;
        g.composing = None;
        g.follow = true;
        g.selecting = false;
        g.selection_start = 0;
        g.selection_end = 0;
        g.sel_anchor = SelAnchor::None;
    }

    /// Enter = 发送：取走文本（清空栏），空文本 = None（无发送）。
    /// 保持聚焦（发送后继续聊，手机聊天惯例）
    pub fn enter(&self) -> Option<String> {
        let mut g = self.inner.lock().unwrap();
        if g.text.is_empty() {
            return None;
        }
        g.cursor = 0;
        g.handle = false;
        g.composing = None; // 发送时组合文本不跟去下游（半截拼音不算话）
        g.selecting = false;
        g.selection_start = 0;
        g.selection_end = 0;
        g.sel_anchor = SelAnchor::None;
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
            g.cursor = 0;
            g.handle = false;
            g.composing = None;
            g.selecting = false;
            g.selection_start = 0;
            g.selection_end = 0;
            g.sel_anchor = SelAnchor::None;
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
