//! termview.rs — 终端视图：alacritty_terminal 网格 + fontdue 光栅 + softbuffer 直推
//!
//! 职责：包装 Term（转义序列/网格/滚屏全交给它），把当前可见网格软渲染进
//! u32 帧缓冲（XRGB）。零 I/O、零平台依赖——host 单测与 Android 壳共用一份。
//!
//! 判卷方式：
//! - A 档考题 tests/termview_spec.rs：布局数学纯函数（grid_dims / cell_origin）
//!   与颜色映射（ANSI 表 / indexed 256 色 / 反色交换）钉死，含变异抽检
//! - B 档冒烟钉（同文件）：feed 字节进真 Term，render_into 后断言帧缓冲
//!   出现非背景像素（字形真画出来了）、红色转义真出红像素、光标格真反色
//! - C 档实拍：手机终端画面（立项.md 尖刺验收 2/3）
//!
//! 已知留白（尖刺期不处理）：
//! - fallback 只有一节（主字体 + 一个 CJK 备用，prefer_cjk 按字形覆盖挑）；
//!   备用也缺的画 tofu（.notdef 方框），不 panic。多级链等实拍再议
//! - 每次 render_into 全量重绘，无 damage 增量（alacritty_terminal 自带
//!   damage 追踪，性能成为问题再接）

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};

/// 单元格像素尺寸（尖刺期常量，字体大小可配化是后话）
/// 2026-08-13 实拍「字太小」：12x24 → 15x30（1080 屏 72 列，等宽字体可读性下限）
pub const CELL_W: u32 = 15;
pub const CELL_H: u32 = 30;

/// 画面边距（BAR-005）：网格不贴边，边缘字符不再被屏幕圆角/曲面切半。
/// 纯黑带，不画框——框是装饰，等中央页面定稿再议
pub const MARGIN_X: u32 = 12;
pub const MARGIN_Y: u32 = 12;

/// 顶边距（BAR-010）：圆角屏吃掉首行首字符（2026-08-13 实拍）——
/// 顶部在常规边距之上再下探一整行
pub const MARGIN_TOP: u32 = MARGIN_Y + CELL_H;

/// 按字形覆盖挑备用字体（A 档考题钉死）：主字体缺该字（glyph_index=0）
/// 且备用字体有才换。字形存在性问 lookup_glyph_index——光栅有没有墨
/// 靠不住（DejaVu 缺字也画 tofu，有墨但不是对的字，host 实测 '中'
/// idx=0 ink=150）。盲文圆点（U+2800 盲文块，kimi code 转动点同款）
/// 就是这条链救的：DejaVuSansMono 没盲文，BBK fallback 顶班
pub fn prefer_cjk(primary: &fontdue::Font, cjk: &fontdue::Font, c: char) -> bool {
    primary.lookup_glyph_index(c) == 0 && cjk.lookup_glyph_index(c) != 0
}

/// 默认前景白 / 背景黑（softbuffer XRGB：高字节不用）
pub const DEFAULT_FG: u32 = 0x00FF_FFFF;
pub const DEFAULT_BG: u32 = 0x0000_0000;

/// ANSI 前 16 色表（VGA 经典配色，XRGB）：0-7 普通，8-15 高亮
pub const ANSI_16: [u32; 16] = [
    0x0000_0000, // 黑
    0x00AA_0000, // 红
    0x0000_AA00, // 绿
    0x00AA_5500, // 黄（VGA 棕）
    0x0000_00AA, // 蓝
    0x00AA_00AA, // 品红
    0x0000_AAAA, // 青
    0x00AA_AAAA, // 白
    0x0055_5555, // 亮黑（灰）
    0x00FF_5555, // 亮红
    0x0055_FF55, // 亮绿
    0x00FF_FF55, // 亮黄
    0x0055_55FF, // 亮蓝
    0x00FF_55FF, // 亮品红
    0x0055_FFFF, // 亮青
    0x00FF_FFFF, // 亮白
];

/// 字体加载候选（按序取第一个及格的）：设备 CJK 优先，host 测试用 DejaVu
/// （12:09 真机普查补充：DroidSansFallbackBBK = vivo 的 fallback 字体，
/// DroidSansMono = 设备自带等宽——usable/monospaced 双判定会把关，
/// 不及格的自动跳过，最后落内嵌 DejaVuSansMono）
pub const FONT_CANDIDATES: &[&str] = &[
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/DroidSansFallbackBBK.ttf",
    "/system/fonts/DroidSansMono.ttf",
    "/system/fonts/Roboto-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];

/// 编译期内嵌的等宽兜底字体（BAR-003）：真机字体三连坑——NotoSansCJK.ttc
/// 空光栅（BAR-002）、Roboto 比例字体间距错乱、DroidSansFallbackFull 不存在。
/// 嵌一份及格的等宽字体进包，任何设备都有下限（选型/许可见 assets/fonts/README.md）
pub static VENDORED_MONO_FONT: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// 字体可用性判定（A 档考题钉死）：光栅化探针字符，空字形（尺寸 0 或
/// 位图零覆盖）判不合格。背景：2026-08-13 真机实拍「只见光标不见字」——
/// NotoSansCJK-Regular.ttc from_bytes 成功却疑似光栅全空：能载 ≠ 能画
pub fn font_usable(font: &fontdue::Font, probe: char) -> bool {
    let (m, bmp) = font.rasterize(probe, CELL_H as f32);
    m.width > 0 && m.height > 0 && bmp.iter().any(|&a| a > 0)
}

/// 等宽判定（A 档考题钉死，BAR-003）：终端网格按定宽格摆字形，比例字体
/// （i 窄 m 宽）摆进去间距忽近忽远。'i' 与 'M' 步进宽相等才算终端可用
pub fn font_monospaced(font: &fontdue::Font) -> bool {
    let (mi, _) = font.rasterize('i', CELL_H as f32);
    let (mm, _) = font.rasterize('M', CELL_H as f32);
    (mi.advance_width - mm.advance_width).abs() < 0.5
}

/// 按候选顺序加载第一个可读、fontdue 认得、能画出字、且等宽的字体，
/// 返回 (来源路径, 字体)。路径候选全灭时落内嵌等宽字体（路径标记
/// "<内嵌>"）；内嵌也废（不可能，有钉）才返回 None。本函数不 panic。
pub fn load_font(candidates: &[&str]) -> Option<(String, fontdue::Font)> {
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Ok(font) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
            // 能载不能画的（BAR-002 NotoSansCJK.ttc）与比例字体（BAR-003 Roboto）
            // 都跳过，给后面的候选机会
            if font_usable(&font, 'M') && font_monospaced(&font) {
                return Some((path.to_string(), font));
            }
        }
    }
    let font =
        fontdue::Font::from_bytes(VENDORED_MONO_FONT, fontdue::FontSettings::default()).ok()?;
    Some(("<内嵌>".to_string(), font))
}

/// CJK 备用字体候选（按序取第一个真能画出 '中' 的）：
/// HYQiHei = vivo 汉仪旗黑（12:09 真机普查实见），BBK/Monster = 国产 ROM
/// fallback 系；NotoSansCJK.ttc 空光栅（BAR-002）会被 usable 判定自动跳过；
/// 末位 host DejaVu 只供 host 测试（tofu 也有墨，链路可验证）。
/// 注意：usable 探针分不出 tofu 和真字形——所以主字体（内嵌 DejaVuSansMono）
/// 绝不能进这份清单，否则设备永远停在豆腐块
pub const CJK_FONT_CANDIDATES: &[&str] = &[
    "/system/fonts/HYQiHei-40_vivo-Design-02.ttf",
    "/system/fonts/DroidSansFallbackBBK.ttf",
    "/system/fonts/DroidSansFallbackMonster.ttf",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];

/// 按候选顺序加载第一个真能画出 '中' 的 CJK 备用字体。全灭返回 None
/// （主字体的 tofu 顶班，不 panic）
pub fn load_cjk_font(candidates: &[&str]) -> Option<(String, fontdue::Font)> {
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Ok(font) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            && font_usable(&font, '中')
        {
            return Some((path.to_string(), font));
        }
    }
    None
}

/// 候选体检（诊断用）：一个字体一行结论——读不到/fontdue 不认/三项判定结果。
/// 真机「为什么偏偏选中它」的判卷依据（12:09 实录：DroidSansMono 明明在
/// 目录里却落选 <内嵌>，没有这行就只能猜）
pub fn diagnose_candidate(path: &str) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return format!("{path}: 读不到");
    };
    match fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
        Err(_) => format!("{path}: fontdue 不认"),
        Ok(f) => format!(
            "{path}: usable={} mono={} cjk={} braille={}",
            font_usable(&f, 'M'),
            font_monospaced(&f),
            font_usable(&f, '中'),
            f.lookup_glyph_index('⠋') != 0 // U+280B 盲文：TUI 转动点覆盖判据
        ),
    }
}

/// 布局数学（A 档考题钉死）：窗口 px 尺寸 + 单元格 px 尺寸 → (cols, rows)。
/// 任一边为 0（窗口未出/单元格非法）或装不下一个格子 → 对应维度为 0。
pub fn grid_dims(win_w: u32, win_h: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    if cell_w == 0 || cell_h == 0 {
        return (0, 0);
    }
    (win_w / cell_w, win_h / cell_h)
}

/// 布局数学（A 档考题钉死）：格坐标 → 帧缓冲像素原点（左上角）。
pub fn cell_origin(col: u32, row: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    (col * cell_w, row * cell_h)
}

/// xterm 256 色索引 → XRGB（A 档考题钉死边界）：
/// 0-15 走 ANSI 表；16-231 是 6×6×6 色立方；232-255 是 24 级灰阶。
pub fn indexed_color(n: u8) -> u32 {
    const LEVELS: [u32; 6] = [0, 95, 135, 175, 215, 255];
    match n {
        0..=15 => ANSI_16[n as usize],
        16..=231 => {
            let n = u32::from(n) - 16;
            let r = LEVELS[(n / 36) as usize];
            let g = LEVELS[((n / 6) % 6) as usize];
            let b = LEVELS[(n % 6) as usize];
            (r << 16) | (g << 8) | b
        }
        232..=255 => {
            let v = 8 + u32::from(n - 232) * 10;
            (v << 16) | (v << 8) | v
        }
    }
}

/// alacritty 颜色 → XRGB。命名色走表，前景/背景走默认，Spec 直包，
/// 未专门处理的（Cursor/Dim*/BrightForeground…）归默认前景。
pub fn color_to_xrgb(c: Color) -> u32 {
    match c {
        Color::Named(named) => match named {
            NamedColor::Foreground | NamedColor::BrightForeground => DEFAULT_FG,
            NamedColor::Background => DEFAULT_BG,
            // 0-15 顺序与 ANSI 表一致（vte 定义即如此），直接转索引
            n if (n as usize) < 16 => ANSI_16[n as usize],
            n if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                .contains(&(n as usize)) =>
            {
                // Dim 系：对应普通色减半亮度
                let base = ANSI_16[n as usize - NamedColor::DimBlack as usize];
                let (r, g, b) = (
                    ((base >> 16) & 0xFF) / 2,
                    ((base >> 8) & 0xFF) / 2,
                    (base & 0xFF) / 2,
                );
                (r << 16) | (g << 8) | b
            }
            _ => DEFAULT_FG, // Cursor 等：无画面语义的归前景
        },
        Color::Spec(rgb) => (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b),
        Color::Indexed(n) => indexed_color(n),
    }
}

/// 字号几何（A 档考题钉死）：给出 (光栅字号, 格内基线偏移)。
/// 约束一（BAR-001 基线对齐）：行盒(ascent-descent)装进格内并居中，
///   行盒比格高则按比例缩字号；
/// 约束二（宽度帽）：探针字符步进宽不得超过格宽，超了再缩——否则
///   相邻格字形互相渗透（放大字号后 DejaVuSansMono 自然超宽）
fn fit_probe_px(font: &fontdue::Font, probe: char, cell_w: u32, cell_h: u32) -> (f32, f32) {
    let px0 = cell_h as f32;
    match font.horizontal_line_metrics(px0) {
        Some(lm) if lm.ascent > 0.0 => {
            let line = lm.ascent - lm.descent; // descent 为负，相减即行盒高
            let mut px = if line > px0 { px0 * px0 / line } else { px0 };
            let (mm, _) = font.rasterize(probe, px);
            if mm.advance_width > cell_w as f32 {
                px *= cell_w as f32 / mm.advance_width;
            }
            let lm2 = font.horizontal_line_metrics(px).unwrap_or(lm);
            let pad = (px0 - (lm2.ascent - lm2.descent)).max(0.0) / 2.0;
            (px, pad + lm2.ascent)
        }
        // 无水平度量（极端字体）兜底：原字号 + 经验基线 80% 处
        _ => (px0, px0 * 0.8),
    }
}

/// 主字体（西文等宽）字号几何：宽度帽探针 'M'
pub fn fit_font_px(font: &fontdue::Font, cell_w: u32, cell_h: u32) -> (f32, f32) {
    fit_probe_px(font, 'M', cell_w, cell_h)
}

/// CJK 备用字体字号几何：全角字占两格，宽度帽探针 '中'（调用方传 2 倍格宽）
pub fn fit_cjk_px(font: &fontdue::Font, two_cell_w: u32, cell_h: u32) -> (f32, f32) {
    fit_probe_px(font, '中', two_cell_w, cell_h)
}

/// Term 尺寸适配器（alacritty_terminal::grid::Dimensions 的本地实现）
#[derive(Clone, Copy)]
struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// CJK 备用字体的字号几何（主字体的同款三件套，按两格宽适配）
/// 不止 CJK：主字体缺的都归它（盲文/符号），见 prefer_cjk
struct CjkStyle {
    font: fontdue::Font,
    px: f32,
    baseline_off: f32,
}

/// 终端视图：Term + vte 解析器 + 字体。事件用 VoidListener 空实现丢弃
/// （OSC52 剪贴板/标题改写等本切片不消费）。
pub struct TermView {
    term: Term<VoidListener>,
    processor: Processor,
    font: fontdue::Font,
    /// CJK 备用字体（fallback 链第一节）：主字体缺的字符归它画（prefer_cjk）；
    /// None = 主字体 tofu 顶班
    cjk: Option<CjkStyle>,
    /// tofu 目击名单（去重，16 格）：双字体都缺的字符攒着，android_app
    /// 定期取走上报——「那个方框到底是什么字」不问用户，问机器。
    /// RefCell：render_into 的 display_iter 借用着 term，draw_glyph 只能 &self
    tofu_seen: std::cell::RefCell<Vec<char>>,
    cell_w: u32,
    cell_h: u32,
    /// 实际光栅字号：行盒（ascent-descent）比格高时按比例缩小，保证装进格
    font_px: f32,
    /// 基线在格内的纵向偏移（格顶向下，px）——BAR-001 基线对齐用
    baseline_off: f32,
}

impl TermView {
    /// 建视图：cols/rows 为初始网格尺寸（窗口未出时给个占位，resize 随后到）。
    /// 任一为 0 会被钳到 1——alacritty Grid 不接受 0 维（会下溢 panic）。
    /// cjk_font 为 CJK 备用字体（可 None）
    pub fn new(
        font: fontdue::Font,
        cjk_font: Option<fontdue::Font>,
        cols: u32,
        rows: u32,
        cell_w: u32,
        cell_h: u32,
    ) -> Self {
        let size = TermSize {
            cols: (cols.max(1)) as usize,
            rows: (rows.max(1)) as usize,
        };
        let cell_h = cell_h.max(1);
        let cell_w = cell_w.max(1);
        // 基线几何（BAR-001）+ 宽度帽：见 fit_font_px/fit_cjk_px 文档
        let (font_px, baseline_off) = fit_font_px(&font, cell_w, cell_h);
        let cjk = cjk_font.map(|f| {
            let (px, bo) = fit_cjk_px(&f, cell_w * 2, cell_h);
            CjkStyle {
                font: f,
                px,
                baseline_off: bo,
            }
        });
        Self {
            term: Term::new(Config::default(), &size, VoidListener),
            processor: Processor::new(),
            font,
            cjk,
            tofu_seen: std::cell::RefCell::new(Vec::new()),
            cell_w,
            cell_h,
            font_px,
            baseline_off,
        }
    }

    /// 喂 PTY 原始字节流（含 ANSI/UTF-8），vte 解析器驱动 Term 状态迁移
    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    /// 改网格尺寸（窗口 Resized 时调）。0 维钳 1，理由同 new。
    pub fn resize_cells(&mut self, cols: u32, rows: u32) {
        self.term.resize(TermSize {
            cols: (cols.max(1)) as usize,
            rows: (rows.max(1)) as usize,
        });
    }

    /// 字体探针（诊断用）：光栅化单字符，返回 (宽, 高, 非零覆盖像素数)。
    /// 真机「只见光标不见字」判卷：字体加载成功 ≠ 能出字形（2026-08-13 实拍，
    /// NotoSansCJK.ttc 载上了但疑似光栅全空）——数字传回，存在性说话
    pub fn font_probe(&self, c: char) -> (usize, usize, usize) {
        let (m, bmp) = self.font.rasterize(c, self.cell_h as f32);
        (m.width, m.height, bmp.iter().filter(|&&a| a > 0).count())
    }

    /// 取走 tofu 目击名单（清缓冲）：双字体都缺的字符，android_app 上报用
    pub fn take_tofu_chars(&self) -> Vec<char> {
        self.tofu_seen.take()
    }

    /// 单元格像素尺寸（android_app 用窗口尺寸反推 cols/rows 时取值）
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// 把当前可见网格渲染进 XRGB 帧缓冲（黑底，满幅重绘）。
    /// buf 尺寸必须与 buf_w*buf_h 一致（调用方 softbuffer 保证；不一致只画放得下的部分）。
    pub fn render_into(&mut self, buf: &mut [u32], buf_w: u32, buf_h: u32) {
        buf.fill(DEFAULT_BG);
        if buf_w == 0 || buf_h == 0 {
            return;
        }
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        let content = self.term.renderable_content();
        let cursor = content.cursor;
        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            if line < 0 {
                continue; // 显示偏移下的历史行不进画面
            }
            let (mut fg, mut bg) = (
                color_to_xrgb(indexed.cell.fg),
                color_to_xrgb(indexed.cell.bg),
            );
            if indexed.cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let is_cursor = cursor.shape != CursorShape::Hidden && indexed.point == cursor.point;
            if is_cursor {
                std::mem::swap(&mut fg, &mut bg);
            }
            let (px, py) = cell_origin(
                indexed.point.column.0 as u32,
                line as u32,
                self.cell_w,
                self.cell_h,
            );
            // BAR-005：格原点加边距，网格不贴边（边距带留黑）；
            // BAR-010：顶部走 MARGIN_TOP（圆角屏下探一整行）
            let (px, py) = (px + MARGIN_X, py + MARGIN_TOP);
            if px >= buf_w || py >= buf_h {
                continue; // 窗口比网格小（resize 途中）：裁掉放不下的格
            }
            // 背景不满格重画（全帧已填 DEFAULT_BG），只在非默认背景时补色块
            if bg != DEFAULT_BG {
                frame.fill_rect(px, py, self.cell_w, self.cell_h, bg);
            }
            let c = indexed.cell.c;
            // 空格/控制符（BAR-015：tab 本体）无字形不画；宽字符第二格不画
            if !paintable(c) || indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            self.draw_glyph(&mut frame, c, px, py, fg);
        }
    }

    /// 光栅化单字形并 alpha 混合进帧缓冲。基线对齐（BAR-001）：fontdue
    /// y 轴向上，metrics.ymin 是位图底边相对基线的偏移（下伸字母为负），
    /// 位图顶边（屏坐标）= 格顶 + 基线偏移 - (ymin + 位图高)。
    /// 字体选择：主字体缺该字且备用有 → CJK 三件套（prefer_cjk，两格宽适配）；
    /// 双字体都缺 → 记 tofu 目击名单（主字体画 .notdef 方框）
    fn draw_glyph(&self, frame: &mut Frame<'_>, c: char, px: u32, py: u32, fg: u32) {
        if self.font.lookup_glyph_index(c) == 0 {
            let covered = self
                .cjk
                .as_ref()
                .is_some_and(|k| k.font.lookup_glyph_index(c) != 0);
            let mut seen = self.tofu_seen.borrow_mut();
            if !covered && !seen.contains(&c) && seen.len() < 16 {
                seen.push(c);
            }
        }
        let (font, font_px, baseline) = match &self.cjk {
            Some(cjk) if prefer_cjk(&self.font, &cjk.font, c) => {
                (&cjk.font, cjk.px, cjk.baseline_off)
            }
            _ => (&self.font, self.font_px, self.baseline_off),
        };
        let (metrics, bitmap) = font.rasterize(c, font_px);
        if metrics.width == 0 || metrics.height == 0 {
            return; // 缺字形/空白字形：fontdue 给空位图，不 panic
        }
        let top = py as i64 + baseline as i64 - i64::from(metrics.ymin) - metrics.height as i64;
        for gy in 0..metrics.height as u32 {
            let y = top + i64::from(gy);
            if y < 0 {
                continue; // 上探出屏（基线偏移 + 高字形）：裁
            }
            if y >= i64::from(frame.h) {
                break;
            }
            for gx in 0..metrics.width as u32 {
                // xmin 可为负（斜体左探）：用有符号算再裁
                let x = px as i64 + i64::from(metrics.xmin) + i64::from(gx);
                if x < 0 || x >= i64::from(frame.w) {
                    continue;
                }
                let a = u32::from(bitmap[(gy * metrics.width as u32 + gx) as usize]);
                if a == 0 {
                    continue;
                }
                frame.blend_px(x as u32, y as u32, fg, a);
            }
        }
    }
}

/// 该字符是否值得上屏（BAR-015）：空格与控制符（C0/C1/DEL）无字形——
/// alacritty put_tab 把 '\t' 本体写进格（为选中/复制能还原 tab），
/// 设备主字体（DroidSansMono）没有 tab 字形 → 不拦就画方框（2026-08-14
/// 实拍：ls 输出文件夹名后方框，tofu 目击名单实锤 U+0009）。
/// 契约钉在本纯函数（A 档考题 spec_渲染_tab控制符不落墨不进目击名单）：
/// host 的 DejaVuSansMono 有 tab 空白字形，像素层面咬不住，必须从这里过滤
pub fn paintable(c: char) -> bool {
    c != ' ' && !c.is_control()
}

/// 帧缓冲视图：把 buf + 尺寸打包，免得每个画图函数都拖一溜参数（clippy 红线）
struct Frame<'a> {
    buf: &'a mut [u32],
    w: u32,
    h: u32,
}

impl Frame<'_> {
    /// 画纯色矩形（裁剪到帧缓冲内）
    fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        for row in y..(y + h).min(self.h) {
            for col in x..(x + w).min(self.w) {
                self.buf[(row * self.w + col) as usize] = color;
            }
        }
    }

    /// 单像素按覆盖率 a 混合（调用方保证 x/y 已在界内）
    fn blend_px(&mut self, x: u32, y: u32, fg: u32, a: u32) {
        let dst = &mut self.buf[(y * self.w + x) as usize];
        *dst = blend(fg, *dst, a);
    }
}

/// 按覆盖率 a（0-255）把 fg 混合到 dst 上（逐通道线性插值）
fn blend(fg: u32, dst: u32, a: u32) -> u32 {
    let inv = 255 - a;
    let ch = |f: u32, d: u32| (f * a + d * inv) / 255;
    let r = ch((fg >> 16) & 0xFF, (dst >> 16) & 0xFF);
    let g = ch((fg >> 8) & 0xFF, (dst >> 8) & 0xFF);
    let b = ch(fg & 0xFF, dst & 0xFF);
    (r << 16) | (g << 8) | b
}

/// 供 android_app：从候选路径建视图（主字体 + CJK 备用 + 默认 80x24 占位网格），
/// 返回 (视图, 主字体来源, CJK 字体来源)。主字体全灭返回 None。
pub fn build_from_candidates(candidates: &[&str]) -> Option<(TermView, String, Option<String>)> {
    let (path, font) = load_font(candidates)?;
    let (cjk_path, cjk_font) = match load_cjk_font(CJK_FONT_CANDIDATES) {
        Some((p, f)) => (Some(p), Some(f)),
        None => (None, None),
    };
    Some((
        TermView::new(font, cjk_font, 80, 24, CELL_W, CELL_H),
        path,
        cjk_path,
    ))
}
