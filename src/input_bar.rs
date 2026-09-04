//! input_bar.rs — 全局输入栏状态核（期 0 组件三；A 档纯逻辑，考题
//! tests/input_bar_spec.rs）。规格书 docs/active/ai-presence.md §二/§五。
//!
//! 常驻 chrome：压底紧贴键盘（快捷键行上移一层让位），任何会话下都在。
//! 焦点二态：终端 / 输入栏——点文本区聚焦（壳层顺带弹键盘），Esc 或点
//! 终端区失焦；聚焦时键盘按键全归输入栏（分流在壳层 drain_ime_inject），
//! Enter = 栏内换行（2026-09-04 用户拍板：多逻辑行排版，发送只走 ▶ 钮
//! 或 gate submit 注入）。
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

/// 文本区垂直内衬（px，BAR-049）：文字/高亮不贴 field 上下沿——2026-09-03
/// 用户对照其他输入框实拍指正：kfmv4 `.ai-input` padding 14px CSS ≈ 40 物理
/// （1260 屏 3x DPI），全选时高亮也不挨边框，滚到边界的行在内衬带里被裁掉
/// 而不是顶着框线。渲染/点按换算/滚动钳制/菜单锚共用这一把尺（眼手同尺）。
pub const TEXT_PAD_Y: u32 = 40;

/// field 高 → 文本视口高（上下各收 TEXT_PAD_Y）
pub fn text_view_h(field_h: u32) -> u32 {
    field_h.saturating_sub(2 * TEXT_PAD_Y)
}

/// 选择菜单气泡尺寸（px，2026-09-03 用户拍板放大）：原 420×72（≈24dp 高，
/// 实拍「感觉太小」）→ 640×120（40dp 高、每格 160 宽，拇指舒适击区）。
/// 渲染/触摸几何/考题共用（眼手同尺）。
pub const MENU_W: u32 = 640;
pub const MENU_H: u32 = 120;
/// 菜单按钮标签字号（与输入栏正文同号 40px——原 30 实拍偏小）
pub const MENU_TEXT_PX: f32 = 40.0;

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

/// 多逻辑行折行（2026-09-04 Enter 换行拍板）：与 termview::wrap_starts
/// 同贪心断行算法，但遇 '\n' 无条件断行——'\n' 是它终止行的最后一字
/// （零宽条目，termview::measure_bar_items 保留在 items 里，保
/// 「item 下标 == char 下标 1:1」全家假设）。chars/widths 必须等长 1:1。
/// 无 '\n' 时与 wrap_starts 逐字节一致（旧软折行为不退化）；
/// 连续/行尾 '\n' 产空行（starts 可等于 items.len()，切片 [len..len]
/// 安全）。A 档纯逻辑，考题 spec_multiline_starts_* 在
/// tests/input_bar_spec.rs（含变异抽检：删 '\n' 分支考题必须红）。
pub fn multiline_starts(chars: &[char], widths: &[f32], max_w: f32) -> Vec<usize> {
    debug_assert_eq!(chars.len(), widths.len(), "chars/widths 必须 1:1");
    let mut starts = vec![0usize];
    let mut acc = 0.0f32;
    for (i, (&c, &w)) in chars.iter().zip(widths.iter()).enumerate() {
        if c == '\n' {
            starts.push(i + 1);
            acc = 0.0;
            continue;
        }
        if i > *starts.last().unwrap() && acc + w > max_w {
            starts.push(i);
            acc = 0.0;
        }
        acc += w;
    }
    starts
}

/// 光标闪烁半周期（ms）：Android 系统输入光标节拍——亮 530 灭 530。
/// 调用方按 (boot_ms / CARET_BLINK_MS) % 2 算相位传渲染
pub const CARET_BLINK_MS: u64 = 530;

/// 长按进入选择模式的时间阈值（ms）：与 Android 系统默认值一致。
pub const SELECT_LONG_PRESS_MS: u64 = 400;

/// 长按选词词跨度（BAR-053）：is_word_char 连续段（与终端侧同字符集
/// termview——CJK 连续句读段、ascii/路径串整段拎出）；落点非词字符 →
/// 该字单选；pos 越界（按在文本尾后）→ 末词。空文本 → None。
/// char 下标 [start, end) 半开。
pub fn word_span_at(text: &str, pos: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let p = pos.min(chars.len() - 1);
    if !crate::termview::is_word_char(chars[p]) {
        return Some((p, p + 1));
    }
    let mut s = p;
    while s > 0 && crate::termview::is_word_char(chars[s - 1]) {
        s -= 1;
    }
    let mut e = p + 1;
    while e < chars.len() && crate::termview::is_word_char(chars[e]) {
        e += 1;
    }
    Some((s, e))
}

/// 词枢轴拖动扩选（BAR-053）：词恒整选 + 扩向指头一侧；指头入词内 →
/// 回词本体（同次拖动可缩回）。产出恒 start ≤ pivot.0 ≤ pivot.1 ≤ end。
/// 2026-09-03 BAR-056：调用方须用 set_selection_span 落跨度（双端原子），
/// 不能拆成 set_selection_start/end 两发——换锚语义会截胡第二发。
pub fn pivot_drag_span(pivot: (usize, usize), idx: usize) -> (usize, usize) {
    let (ps, pe) = pivot;
    if idx < ps {
        (idx, pe)
    } else if idx > pe {
        (ps, idx)
    } else {
        (ps, pe)
    }
}

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

/// 窗口坐标钳进文本框矩形（拖动态专用，BAR-055）：框内原样；框外按
/// 最近边——拖锚点/枢轴扩选时指头滑出框界（上下飘、抓柄时指心在框
/// 下沿外）按最近行列换算，不再 None 冻结断触。点按/命中判定不许用
/// 这把钳制尺（会误中），只准拖动连续态用。
pub fn clamp_to_field(x: f64, y: f64, left: u32, top: u32, w: u32, h: u32) -> (f64, f64) {
    let right = f64::from(left.saturating_add(w).saturating_sub(1));
    let bottom = f64::from(top.saturating_add(h).saturating_sub(1));
    (
        x.clamp(f64::from(left), right),
        y.clamp(f64::from(top), bottom),
    )
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

    /// finish 组合态（拼音落字）——进入选择/点按定位等动作前的共用前奏
    fn commit_composing(g: &mut Inner) {
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
    }

    /// 进入选择模式：先 finish 组合态，光标/定位柄转双锚点
    pub fn enter_selection(&self, pos: usize) {
        let mut g = self.inner.lock().unwrap();
        Self::commit_composing(&mut g);
        let len = g.text.chars().count();
        let pos = pos.min(len);
        g.selecting = true;
        g.selection_start = pos;
        g.selection_end = pos;
        g.sel_anchor = SelAnchor::None;
        g.handle = false;
        g.follow = true;
    }

    /// 长按选词进入选择模式（BAR-053）：落点词整段选中——非空可见高亮、
    /// 双锚点、菜单的活选区（空选区刚召唤即不可见、抬手又被点按分路
    /// 清掉，等于没有长按入口）。活动锚 = 右锚（续滑扩选从词尾起）。
    /// 返回词跨度供壳层登记枢轴；空文本 → None 不进选择态。
    pub fn enter_selection_word(&self, pos: usize) -> Option<(usize, usize)> {
        let mut g = self.inner.lock().unwrap();
        Self::commit_composing(&mut g);
        let (s, e) = word_span_at(&g.text, pos)?;
        g.selecting = true;
        g.selection_start = s;
        g.selection_end = e;
        g.sel_anchor = SelAnchor::Right;
        g.handle = false;
        g.follow = true;
        Some((s, e))
    }

    /// 拖动左锚点（触摸拖柄专用，BAR-056 换锚语义）：拖过右锚点不钳死，
    /// 两锚交换——原右锚变新左锚，指头继续拖着新右锚走（Android/浏览器
    /// 标准行为；旧钳制语义会把选区压成零宽，实拍「选择框消失」）。
    /// 返回指头此刻持有的锚（未交叉=Left，交叉换锚=Right），调用方据此
    /// 更新拖动状态。非选择态 = 无操作（返回 None 保持调用方原锚）。
    pub fn set_selection_start(&self, pos: usize) -> SelAnchor {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting {
            return SelAnchor::Left;
        }
        let len = g.text.chars().count();
        let pos = pos.min(len);
        if pos > g.selection_end {
            // 交叉：换锚——旧右锚定身为新左锚，指头改持新右锚
            g.selection_start = g.selection_end;
            g.selection_end = pos;
            g.sel_anchor = SelAnchor::Right;
        } else {
            g.selection_start = pos;
            g.sel_anchor = SelAnchor::Left;
        }
        g.follow = true;
        g.sel_anchor
    }

    /// 拖动右锚点（触摸拖柄专用，BAR-056 换锚语义）：拖过左锚点两锚
    /// 交换，指头改持新左锚。语义与返回值约定同 set_selection_start。
    pub fn set_selection_end(&self, pos: usize) -> SelAnchor {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting {
            return SelAnchor::Right;
        }
        let len = g.text.chars().count();
        let pos = pos.min(len);
        if pos < g.selection_start {
            // 交叉：换锚——旧左锚定身为新右锚，指头改持新左锚
            g.selection_end = g.selection_start;
            g.selection_start = pos;
            g.sel_anchor = SelAnchor::Left;
        } else {
            g.selection_end = pos;
            g.sel_anchor = SelAnchor::Right;
        }
        g.follow = true;
        g.sel_anchor
    }

    /// 程序侧原子设选区（枢轴扩选/闸门注入用）：恒 start ≤ end 直接落，
    /// 不走 BAR-056 换锚——换锚是单锚拖动的交互语义，双端同设被它截胡
    /// 会得到错误跨度（钉 spec_bar056_程序侧双端同设不换锚）。
    pub fn set_selection_span(&self, start: usize, end: usize) {
        let mut g = self.inner.lock().unwrap();
        if !g.selecting {
            return;
        }
        let len = g.text.chars().count();
        let (s, e) = (start.min(end), start.max(end));
        g.selection_start = s.min(len);
        g.selection_end = e.min(len);
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

    /// 光标前 n 字（IME getTextBeforeCursor 用，BAR-054）：选择态取选区
    /// 起点之前（Android 契约：selecting 时 before-cursor = 选区开始前），
    /// 否则光标前；不足 n 字全给。IME 的内部删除/替换逻辑靠它算范围，
    /// 答空它就算出 0 长度删个寂寞。
    pub fn text_before_cursor(&self, n: usize) -> String {
        let g = self.inner.lock().unwrap();
        let edge = if g.selecting {
            g.selection_start
        } else {
            g.cursor
        };
        let before: Vec<char> = g.text.chars().take(edge).collect();
        before.iter().skip(before.len().saturating_sub(n)).collect()
    }

    /// 光标后 n 字（IME getTextAfterCursor 用，BAR-054）：选择态取选区
    /// 终点之后，否则光标后；不足 n 字全给。
    pub fn text_after_cursor(&self, n: usize) -> String {
        let g = self.inner.lock().unwrap();
        let edge = if g.selecting {
            g.selection_end
        } else {
            g.cursor
        };
        g.text.chars().skip(edge).take(n).collect()
    }

    /// IME setSelection/replaceText 直设（BAR-054）：start==end = 光标定位
    /// （退出选择态），不等 = 程序侧选区原子落（不走 BAR-056 换锚——换锚
    /// 是触摸拖柄的交互语义，IME 直设是全文坐标系）。
    pub fn set_caret_or_selection(&self, start: usize, end: usize) {
        let mut g = self.inner.lock().unwrap();
        Self::commit_composing(&mut g);
        let len = g.text.chars().count();
        let (s, e) = (start.min(end).min(len), start.max(end).min(len));
        if s == e {
            g.selecting = false;
            g.selection_start = 0;
            g.selection_end = 0;
            g.sel_anchor = SelAnchor::None;
            g.cursor = s;
        } else {
            g.selecting = true;
            g.selection_start = s;
            g.selection_end = e;
            g.sel_anchor = SelAnchor::None;
            g.cursor = e;
        }
        g.follow = true;
    }

    /// IME replaceText 直改（BAR-054）：[start,end) 区间替换为 text，
    /// 光标落插入文本尾，退出选择态。越界钳到全文。
    pub fn replace_range(&self, start: usize, end: usize, text: &str) {
        let mut g = self.inner.lock().unwrap();
        Self::commit_composing(&mut g);
        let len = g.text.chars().count();
        let (s, e) = (start.min(end).min(len), start.max(end).min(len));
        let bs = g
            .text
            .char_indices()
            .nth(s)
            .map(|(b, _)| b)
            .unwrap_or(g.text.len());
        let be = g
            .text
            .char_indices()
            .nth(e)
            .map(|(b, _)| b)
            .unwrap_or(g.text.len());
        g.text.replace_range(bs..be, text);
        g.cursor = s + text.chars().count();
        g.selecting = false;
        g.selection_start = 0;
        g.selection_end = 0;
        g.sel_anchor = SelAnchor::None;
        g.follow = true;
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
