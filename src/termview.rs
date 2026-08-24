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
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};

/// 单元格像素尺寸基准（捏合缩放的 1.0 锚点 + 无持久化时的冷启动默认）。
/// 2026-08-13 实拍「字太小」：12x24 → 15x30；2026-08-21 用户两次抱怨
/// 「太小」：15x30 → 18x36（1080 屏净宽 1056px ≈ 58 列）
pub const CELL_W: u32 = 18;
pub const CELL_H: u32 = 36;

/// 画面边距（BAR-005）：网格不贴边，边缘字符不再被屏幕圆角/曲面切半。
/// 纯黑带，不画框——框是装饰，等中央页面定稿再议
pub const MARGIN_X: u32 = 12;
pub const MARGIN_Y: u32 = 12;

/// 顶边距（BAR-010）：圆角屏吃掉首行首字符（2026-08-13 实拍）——
/// 顶部在常规边距之上再下探一整行。这是基准格高（CELL_H）下的常量值；
/// 格高随捏合缩放变后必须走 margin_top(cell_h) 动态版
pub const MARGIN_TOP: u32 = MARGIN_Y + CELL_H;

/// 顶边距动态版（A 档考题钉死）：跟随当前格高——缩放任一档下顶带都是
/// 「常规边距 + 一整行」，圆角屏语义不随缩放漂移
pub const fn margin_top(cell_h: u32) -> u32 {
    MARGIN_Y + cell_h
}

/// 捏合缩放格尺寸钳制区间（A 档考题钉死）：10x20 = 还能认出字的下限，
/// 45x90 = 一屏 24 列 26 行的上限（再大打不了字）
pub const CELL_W_MIN: u32 = 10;
pub const CELL_W_MAX: u32 = 45;
pub const CELL_H_MIN: u32 = 20;
pub const CELL_H_MAX: u32 = 90;

/// 捏合比例 → 格尺寸（A 档考题钉死）：基准 × 比例四舍五入取整，钳到
/// 可读区间。非法比例（NaN/0/负/无穷）不落钳制结果而落基准本身——
/// 坏输入不许把字号打飞
pub fn pinch_cell_size(base_w: u32, base_h: u32, ratio: f64) -> (u32, u32) {
    if !ratio.is_finite() || ratio <= 0.0 {
        return (
            base_w.clamp(CELL_W_MIN, CELL_W_MAX),
            base_h.clamp(CELL_H_MIN, CELL_H_MAX),
        );
    }
    let w = (f64::from(base_w) * ratio).round() as u32;
    let h = (f64::from(base_h) * ratio).round() as u32;
    (
        w.clamp(CELL_W_MIN, CELL_W_MAX),
        h.clamp(CELL_H_MIN, CELL_H_MAX),
    )
}

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

/// ANSI 前 16 色表（XRGB）：0-7 普通，8-15 高亮。主体 VGA 经典配色；
/// **蓝系例外（2026-08-23 实拍）**:VGA #0000AA/#5555FF 在纯黑底上不可读
/// （ssh 远端 ls 目录名、help 标题看不清）——换 kfmv4 品牌蓝系
pub const ANSI_16: [u32; 16] = [
    0x0000_0000, // 黑
    0x00AA_0000, // 红
    0x0000_AA00, // 绿
    0x00AA_5500, // 黄（VGA 棕）
    0x003B_82F6, // 蓝 → kfmv4 品牌正蓝(原 VGA #0000AA 黑底不可读)
    0x00AA_00AA, // 品红
    0x0000_AAAA, // 青
    0x00AA_AAAA, // 白
    0x0055_5555, // 亮黑（灰）
    0x00FF_5555, // 亮红
    0x0055_FF55, // 亮绿
    0x00FF_FF55, // 亮黄
    0x0060_A5FA, // 亮蓝 → 品牌蓝亮一档(原 VGA #5555FF)
    0x00FF_55FF, // 亮品红
    0x0055_FFFF, // 亮青
    0x00FF_FFFF, // 亮白
];

/// 字体加载候选（按序取第一个及格的）：设备 CJK 优先，host 测试用 DejaVu
/// （12:09 真机普查补充：DroidSansFallbackBBK = vivo 的 fallback 字体，
/// DroidSansMono = 设备自带等宽——usable/monospaced 双判定会把关，
/// 不及格的自动跳过，最后落内嵌 DejaVuSansMono）
/// 2026-08-18 启动提速：DroidSansMono 提首（真机实证它就是胜者，
/// 108KB 秒杀）；NotoSansCJK.ttc/DroidSansFallback* 是几十 MB 巨物,
/// 反正过不了探针,留表尾靠 MAX_MAIN_FONT_BYTES 体积闸廉价跳过
pub const FONT_CANDIDATES: &[&str] = &[
    "/system/fonts/DroidSansMono.ttf",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/DroidSansFallbackBBK.ttf",
    "/system/fonts/Roboto-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];

/// 编译期内嵌的等宽兜底字体（BAR-003）：真机字体三连坑——NotoSansCJK.ttc
/// 空光栅（BAR-002）、Roboto 比例字体间距错乱、DroidSansFallbackFull 不存在。
/// 嵌一份及格的等宽字体进包，任何设备都有下限（选型/许可见 assets/fonts/README.md）
pub static VENDORED_MONO_FONT: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// 生产内嵌字体（BAR-021，build.rs 编译期选择：assets/fonts/local/ 覆盖 >
/// 开源占位，规则见 build.rs 头注）。启动零探测——不读 /system/fonts，
/// 不解析 44MB 巨物，TermView 毫秒级建成（启动慢病灶连根拔，BAR-020 终章）
pub static VENDORED_MAIN_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fonts/main.ttf"));
/// 生产内嵌 CJK 备用字体（同 build.rs 选择；全角双宽，覆盖 GB2312 全字库）
pub static VENDORED_CJK_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fonts/cjk.ttf"));

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

/// 主字体体积闸（2026-08-18 启动慢实测：表面建成→TermView 建成 6 秒,
/// 病灶=每次启动全量解析 NotoSansCJK.ttc 32MB + DroidSansFallbackBBK
/// 44MB 再被探针扔掉)。等宽 Latin 主字体不可能是几十 MB 的巨物——
/// 超闸直接不解析,行为不变(它们本来就过不了 usable/mono 探针),
/// CJK 备用表不受此闸(那边的巨物是真字形源)
pub const MAX_MAIN_FONT_BYTES: u64 = 8 * 1024 * 1024;

/// 按候选顺序加载第一个可读、fontdue 认得、能画出字、且等宽的字体，
/// 返回 (来源路径, 字体)。路径候选全灭时落内嵌等宽字体（路径标记
/// "<内嵌>"）；内嵌也废（不可能，有钉）才返回 None。本函数不 panic。
pub fn load_font(candidates: &[&str]) -> Option<(String, fontdue::Font)> {
    for path in candidates {
        // 体积闸:metadata 即判,几十 MB 的巨物连读都不读
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_MAIN_FONT_BYTES {
            continue;
        }
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

/// 坐标换算（A 档考题钉死）：帧缓冲像素 → 屏内格 (col, row)。
/// 渲染的反向：减边距 MARGIN_X 与顶带 margin_top(cell_h)（格高随缩放变，
/// 判定尺必须与 render_into 同一把）；越界（边距带内/网格外）钳到网格边缘
pub fn px_to_cell(x: f64, y: f64, cols: u32, rows: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    let col = ((x - f64::from(MARGIN_X)) / f64::from(cell_w.max(1))).floor();
    let row = ((y - f64::from(margin_top(cell_h))) / f64::from(cell_h.max(1))).floor();
    let max_col = f64::from(cols.max(1)) - 1.0;
    let max_row = f64::from(rows.max(1)) - 1.0;
    (
        col.clamp(0.0, max_col) as u32,
        row.clamp(0.0, max_row) as u32,
    )
}

/// 词选择字符集（A 档考题钉死）：字母数字 + 常见路径字符 `_-./:~`
/// 连续段算一个词——长按选词就是要把路径/URL/选项串整段拎出来
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '~')
}

/// 选择区（网格坐标 (Line, Column)：行号含历史负行——滚进历史后选择
/// 跟着内容走，与 render_into 的 display_iter 行号同坐标系）。
/// anchor = 长按落点词首，cursor = 拖动当前点；归一化在判定/提取时做
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (i32, u32),
    pub cursor: (i32, u32),
}

/// 选择范围判定（A 档考题钉死）：anchor/cursor 归一化（反向拖也算），
/// 含端点的闭区间按 (行, 列) 字典序
pub fn in_selection(anchor: (i32, u32), cursor: (i32, u32), line: i32, col: u32) -> bool {
    let (s, e) = if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    (line, col) >= s && (line, col) <= e
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
    /// 长按选择区（网格坐标，含历史负行）：Some = 选择模式激活，
    /// 渲染高亮 + 单击复制；None = 无选区
    selection: Option<Selection>,
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
            selection: None,
        }
    }

    /// 运行期改格尺寸（双指捏合缩放，2026-08-21）：重算光栅字号/基线/
    /// CJK 备用三件套（逻辑同 new 的 fit_font_px/fit_cjk_px）。网格重排
    /// 不在此做——调用方随后 resize_cells（alacritty resize 自带 reflow）。
    /// 0 维钳 1，同 new；尺寸没变则不动（防抖链最后一环）
    pub fn set_cell_size(&mut self, cell_w: u32, cell_h: u32) {
        let cell_w = cell_w.max(1);
        let cell_h = cell_h.max(1);
        if (cell_w, cell_h) == (self.cell_w, self.cell_h) {
            return;
        }
        self.cell_w = cell_w;
        self.cell_h = cell_h;
        let (px, bo) = fit_font_px(&self.font, cell_w, cell_h);
        self.font_px = px;
        self.baseline_off = bo;
        if let Some(cjk) = &mut self.cjk {
            let (px, bo) = fit_cjk_px(&cjk.font, cell_w * 2, cell_h);
            cjk.px = px;
            cjk.baseline_off = bo;
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

    /// 滚动可视窗口（scrollback）：lines 正 = 看更老的历史（手指向下拖），
    /// 负 = 往最新回。alacritty 内部自钳到历史顶/底，调用方不用管边界
    pub fn scroll_lines(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// 回到底部贴最新输出（用户输入时调用——打字了就是要看现在，不是看历史）
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// 当前显示偏移（行，0 = 贴底）——B 档考题钉 + 实拍上报用
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// 当前视野纯文本导出（调试闸门 text-req 通道，2026-08-24）：
    /// 可见区 = display_offset 起 screen_lines 行（滚动中跟视野走），
    /// 逐格收字符、跳过宽字符 spacer 半格，行尾 trim，行间 \n。
    /// v1 不导 scrollback——闸门只对齐「所见」（网格眼睛胚胎）
    pub fn dump_text(&self) -> String {
        let grid = self.term.grid();
        let off = grid.display_offset() as i32;
        let lines = grid.screen_lines() as i32;
        let cols = grid.columns();
        let mut out = String::with_capacity((lines as usize) * (cols / 2));
        for row in 0..lines {
            let grid_line = Line(row - off);
            let mut s = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[grid_line][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue; // CJK 宽字符的后半格:字已在前半格收过
                }
                s.push(cell.c);
            }
            out.push_str(s.trim_end());
            out.push('\n');
        }
        out
    }

    /// 对端（tmux/kimicode 等 TUI）是否开了鼠标上报（?1000/1002/1003 任一）——
    /// 开了滚屏就必须翻成滚轮事件发过去（BAR-016：alt screen 没有本地历史）
    pub fn mouse_report_active(&self) -> bool {
        self.term.mode().intersects(
            TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
        )
    }

    /// 对端是否开了应用光标模式（?1h，vim/kimicode 会开）——快捷键行的
    /// 方向键/End 序列按它分岔（keymap.rs key_seq 的 app_cursor 参数）
    pub fn app_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// 单元格像素尺寸（android_app 用窗口尺寸反推 cols/rows 时取值）
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    // ---- 长按选择（2026-08-21，状态机/坐标约定见 docs/active/壳层交互.md） ----

    /// 选择模式激活中（有选区）——android_app 据此改路由：拖动 = 扩选，
    /// 单击 = 复制清选，点按唤键盘让路
    pub fn selection_active(&self) -> bool {
        self.selection.is_some()
    }

    /// 像素 → 网格点 (Line 含历史负行, Column)：屏格走 px_to_cell
    /// （边距/顶带同 render_into 一把尺），网格行 = 屏行 - display_offset
    /// （render_into 屏行 = 网格行 + display_offset 的逆运算）
    fn grid_point_at(&self, x: f64, y: f64) -> (i32, u32) {
        let grid = self.term.grid();
        let (col, row) = px_to_cell(
            x,
            y,
            grid.columns() as u32,
            grid.screen_lines() as u32,
            self.cell_w,
            self.cell_h,
        );
        let line = row as i32 - grid.display_offset() as i32;
        (line, col)
    }

    /// 该格是否 CJK 宽字符的 spacer 半格（宽字符占 col-1..col 两格，
    /// col 是 spacer）。行出界（含历史区）按 false 防御
    fn is_spacer(&self, line: i32, col: u32) -> bool {
        let grid = self.term.grid();
        let lo = -(grid.history_size() as i32);
        let hi = grid.screen_lines() as i32 - 1;
        if !(lo..=hi).contains(&line) {
            return false;
        }
        grid[Line(line)][Column(col as usize)]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
    }

    /// 宽字符边界钳制（2026-08-21 kfmv4 对齐）：端点落在 CJK spacer 半格
    /// 时按拖动方向钳——右移钳 col+1（越过该字到下一格），左移钳 col-1
    /// （回到该字格 0）。端点永不劈字。固有结果（实拍判卷点）：右拖终点
    /// 到 spacer 会把后一格也包进选区（后一格非空白时多选一个字）；
    /// 提取本就不收 spacer（selected_text 跳过），钳制前后提取等价
    /// （一致性考题 spec_选择_宽字符钳制提取一致性 钉死）
    fn clamp_wide_endpoint(&self, point: (i32, u32), moving_right: bool) -> (i32, u32) {
        let (line, col) = point;
        if !self.is_spacer(line, col) {
            return point;
        }
        if moving_right {
            let last = self.term.grid().columns() as u32 - 1;
            (line, (col + 1).min(last))
        } else {
            (line, col - 1) // spacer 的格 0 必在 col-1（col ≥ 1）
        }
    }

    /// 长按选词：落点所在词（is_word_char 连续段）整段选中；落点非词
    /// 字符（空白/标点）只选该格。落点在 CJK spacer 半格 → 当作按在该字
    /// 格 0（按下无方向，归字内）；词尾是宽字符格 0 时把它的 spacer 格
    /// 带进选区（端点落整字边界，渲染/提取同尺不劈字）。滚进历史后选的
    /// 就是历史行（坐标含 display_offset，见 grid_point_at）
    pub fn select_word_at(&mut self, x: f64, y: f64) {
        let (line, col) = self.grid_point_at(x, y);
        let col = if self.is_spacer(line, col) {
            col - 1
        } else {
            col
        };
        let cols = self.term.grid().columns() as u32;
        let at = |c: u32| self.term.grid()[Line(line)][Column(c as usize)].c;
        let (mut start, mut end) = (col, col);
        if is_word_char(at(col)) {
            while start > 0 && is_word_char(at(start - 1)) {
                start -= 1;
            }
            while end + 1 < cols && is_word_char(at(end + 1)) {
                end += 1;
            }
        }
        if end + 1 < cols && self.is_spacer(line, end + 1) {
            end += 1; // 词尾宽字符：带上它的 spacer 格
        }
        self.selection = Some(Selection {
            anchor: (line, start),
            cursor: (line, end),
        });
    }

    /// 选择模式拖动扩选：cursor 端跟手指走（落 spacer 半格按拖动方向
    /// 钳，见 clamp_wide_endpoint），跨行/反向/历史区同尺
    /// （归一化在 in_selection/selected_text 做）。反向拖过 anchor 时
    /// 固定端翻转到原词另一端——整词保持在选区内（选词后上拖不收掉半词）
    pub fn extend_selection(&mut self, x: f64, y: f64) {
        let Some(mut sel) = self.selection else {
            return;
        };
        let raw = self.grid_point_at(x, y);
        let point = self.clamp_wide_endpoint(raw, raw >= sel.cursor);
        if (point < sel.anchor && sel.cursor >= sel.anchor)
            || (point > sel.anchor && sel.cursor < sel.anchor)
        {
            sel.anchor = sel.cursor;
        }
        sel.cursor = point;
        self.selection = Some(sel);
    }

    /// 清高亮（复制后/会话重开等）
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// 考题探针：绕过宽字符边界钳制直接摆放选区端点——一致性考题拿它
    /// 把端点人为放到 spacer 半格上，比对「raw 提取 ≡ 钳后提取」
    /// （spec_选择_宽字符钳制提取一致性）。生产路径不走这里
    #[doc(hidden)]
    pub fn set_selection_raw(&mut self, anchor: (i32, u32), cursor: (i32, u32)) {
        self.selection = Some(Selection { anchor, cursor });
    }

    /// 提取选中文字（复制用）：归一化区间逐行收 cell.c——tab 本体在格内
    /// 原样还原（BAR-015：put_tab 写的就是 '\t'）；宽字符占位格跳过；
    /// zerowidth 组合符带上；行尾空白 trim，行间补 \n。无选区 → None
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (s, e) = if sel.anchor <= sel.cursor {
            (sel.anchor, sel.cursor)
        } else {
            (sel.cursor, sel.anchor)
        };
        let grid = self.term.grid();
        let last_col = grid.columns() as u32 - 1;
        // 防御钳制：选区存活期间滚屏/新输出可能让行号出界
        let lo = -(grid.history_size() as i32);
        let hi = grid.screen_lines() as i32 - 1;
        let last_line = e.0.min(hi);
        let mut out = String::new();
        for l in s.0.max(lo)..=last_line {
            let from = if l == s.0 { s.1 } else { 0 };
            let to = if l == e.0 { e.1 } else { last_col };
            let mut line = String::new();
            for c in from..=to.min(last_col) {
                let cell = &grid[Line(l)][Column(c as usize)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                line.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    for &z in zw {
                        line.push(z);
                    }
                }
            }
            out.push_str(line.trim_end());
            if l < last_line {
                out.push('\n');
            }
        }
        Some(out)
    }

    /// 触点命中选区哪一端（边界直拖——2026-08-21 拖柄废除改此：水滴柄
    /// 丑且白占一行高）：触点格与归一化起/止端格的行差、列差都 ≤1
    /// （触控宽容，手指不是鼠标）即算抓住；两端同圈（相邻格小选区）取
    /// 距触点像素近的一端，等距钉 Start（确定性规则，考题钉死）。
    /// 屏外端点天然抓不到（触点总在屏内，距离必超圈）。无选区 → None
    pub fn hit_boundary(&self, x: f64, y: f64) -> Option<SelEnd> {
        let sel = self.selection?;
        let (s, e) = if sel.anchor <= sel.cursor {
            (sel.anchor, sel.cursor)
        } else {
            (sel.cursor, sel.anchor)
        };
        let (line, col) = self.grid_point_at(x, y);
        let near =
            |end: (i32, u32)| (end.0 - line).abs() <= 1 && (end.1 as i32 - col as i32).abs() <= 1;
        // 平局裁决：端点格心距触点的像素距离（行换算回屏行 = +display_offset）
        let dist = |end: (i32, u32)| {
            let cx = f64::from(MARGIN_X + end.1 * self.cell_w) + f64::from(self.cell_w) / 2.0;
            let row = end.0 + self.term.grid().display_offset() as i32;
            let cy =
                f64::from(margin_top(self.cell_h)) + (row as f64 + 0.5) * f64::from(self.cell_h);
            (x - cx).powi(2) + (y - cy).powi(2)
        };
        match (near(s), near(e)) {
            (true, true) => Some(if dist(s) <= dist(e) {
                SelEnd::Start
            } else {
                SelEnd::End
            }),
            (true, false) => Some(SelEnd::Start),
            (false, true) => Some(SelEnd::End),
            (false, false) => None,
        }
    }

    /// 拖动选区边界移动端点：归一化起/止端谁被拖就谁跟手指（网格坐标换算
    /// 沿用 grid_point_at——跨行/历史区同尺；落 spacer 半格按拖动方向钳，
    /// 方向 = 新落点 vs 该端旧位置字典序）。拖过另一端则角色互换
    /// （起点拖过终点 → 它变成新终点），选区不塌缩翻转
    pub fn move_selection_end(&mut self, which: SelEnd, x: f64, y: f64) {
        let Some(sel) = self.selection else { return };
        let (s, e) = if sel.anchor <= sel.cursor {
            (sel.anchor, sel.cursor)
        } else {
            (sel.cursor, sel.anchor)
        };
        let raw = self.grid_point_at(x, y);
        let old = match which {
            SelEnd::Start => s,
            SelEnd::End => e,
        };
        let p = self.clamp_wide_endpoint(raw, raw >= old);
        self.selection = Some(match which {
            SelEnd::Start => {
                if p <= e {
                    Selection {
                        anchor: p,
                        cursor: e,
                    }
                } else {
                    Selection {
                        anchor: e,
                        cursor: p,
                    }
                }
            }
            SelEnd::End => {
                if p >= s {
                    Selection {
                        anchor: s,
                        cursor: p,
                    }
                } else {
                    Selection {
                        anchor: p,
                        cursor: s,
                    }
                }
            }
        });
    }

    /// 放大镜（边界拖动中，android_app 在主渲染+快捷键行之后调用）：
    /// 触点正下方那格为中心，±MAG_HALF_COLS 格 × ±MAG_HALF_ROWS 行的
    /// 帧缓冲源区最近邻 MAG_ZOOM 倍贴进带边框的圆角浮窗，默认浮在触点
    /// 上方（MAG_GAP_PX 间距不挡手）；上方放不下翻转到触点下方，两侧都
    /// 放不下才钳屏内。源区出屏部分留衬底黑
    pub fn render_magnifier(&self, buf: &mut [u32], buf_w: u32, buf_h: u32, x: f64, y: f64) {
        if buf_w == 0 || buf_h == 0 || buf.len() < (buf_w * buf_h) as usize {
            return;
        }
        // 源区中心 = 触点正下方那格的格心（不是触点本身——对齐到格，
        // 用户看清的是「端点正往哪个字符上放」）
        let (col, row) = px_to_cell(
            x,
            y,
            self.term.grid().columns() as u32,
            self.term.grid().screen_lines() as u32,
            self.cell_w,
            self.cell_h,
        );
        let cx = f64::from(MARGIN_X + col * self.cell_w) + f64::from(self.cell_w) / 2.0;
        let cy =
            f64::from(margin_top(self.cell_h) + row * self.cell_h) + f64::from(self.cell_h) / 2.0;
        let src_hw = MAG_HALF_COLS * self.cell_w; // 源区半宽（px）
        let src_hh = MAG_HALF_ROWS * self.cell_h;
        let win_w = src_hw * 2 * MAG_ZOOM;
        let win_h = src_hh * 2 * MAG_ZOOM;
        if win_w == 0 || win_h == 0 || win_w > buf_w || win_h > buf_h {
            return; // 窗比屏大（极端小窗）：保命不画
        }
        // 浮窗位置：水平对触点居中；默认浮触点上方 MAG_GAP_PX 不挡手——
        // 上方放不下（触点贴屏顶）翻转到触点下方（2026-08-21 实拍：贴顶
        // 拖动时旧钳制把浮窗压到屏顶盖住触点，看不见 = 失控）；两侧都
        // 放不下（极端矮屏）才退回屏内钳制保命
        const BORDER: u32 = 2;
        let win_x = (x as i64 - (win_w / 2) as i64).clamp(0, (buf_w - win_w) as i64) as u32;
        let above_y = y as i64 - i64::from(MAG_GAP_PX) - win_h as i64;
        let win_y = if above_y >= 0 {
            above_y as u32
        } else {
            let below_y = y as i64 + i64::from(MAG_GAP_PX);
            if below_y + win_h as i64 <= i64::from(buf_h) {
                below_y as u32
            } else {
                above_y.clamp(0, (buf_h - win_h) as i64) as u32
            }
        };
        // 先把源区拷出来（读写同一块 buf，不拷会自踩）
        let src_x0 = (cx as i64 - src_hw as i64).max(0);
        let src_y0 = (cy as i64 - src_hh as i64).max(0);
        let src_x1 = (cx as i64 + src_hw as i64).min(i64::from(buf_w) - 1);
        let src_y1 = (cy as i64 + src_hh as i64).min(i64::from(buf_h) - 1);
        let (rw, rh) = (
            (src_x1 - src_x0 + 1).max(0) as u32,
            (src_y1 - src_y0 + 1).max(0) as u32,
        );
        let mut region = vec![DEFAULT_BG; (rw * rh) as usize];
        for ry in 0..rh {
            let sy = src_y0 + i64::from(ry);
            for rx in 0..rw {
                let sx = src_x0 + i64::from(rx);
                region[(ry * rw + rx) as usize] = buf[(sy * i64::from(buf_w) + sx) as usize];
            }
        }
        // 边框 + 圆角（外圈），内容贴进内圈
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        frame.fill_round_rect(
            win_x.saturating_sub(BORDER),
            win_y.saturating_sub(BORDER),
            win_w + 2 * BORDER,
            win_h + 2 * BORDER,
            14,
            MAG_BORDER,
        );
        // 最近邻放大：dest 像素 (dx,dy) ← 源 (cx + (dx - win_w/2)/ZOOM, …)
        for dy in 0..win_h {
            let sy = (cy + (f64::from(dy) - win_h as f64 / 2.0) / MAG_ZOOM as f64).round() as i64;
            if sy < src_y0 || sy > src_y1 {
                continue; // 源区外（屏外）：留衬底/边框
            }
            for dx in 0..win_w {
                let sx =
                    (cx + (f64::from(dx) - win_w as f64 / 2.0) / MAG_ZOOM as f64).round() as i64;
                if sx < src_x0 || sx > src_x1 {
                    continue;
                }
                let px = region[((sy - src_y0) as u32 * rw + (sx - src_x0) as u32) as usize];
                frame.buf[((win_y + dy) * buf_w + win_x + dx) as usize] = px;
            }
        }
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
        let selection = self.selection; // Copy 出来，与 content 的 term 借用拆开
        // 屏行 = 网格行 + 显示偏移（BAR-016）：滚进历史后 alacritty 给的行号
        // 是负的（Line(-offset)），跳过或直接用绝对行号都会让内容不随偏移
        // 移动、每滚一行底部黑一行（实拍「从下到上一行行消失」）
        let offset = content.display_offset as i32;
        // 两遍绘制（2026-08-21 实拍「选中态中文只剩左半」病灶）：先全部背景
        // （含选择高亮），后全部字形。一遍绘制时宽字符（CJK）在格 0 画双宽
        // 字形、墨探进格 1，随后 spacer 格的背景填充（选中=SELECT_BG）把
        // 右半字形盖掉——两遍制让一切背景都在字形之下
        struct Cell2D {
            px: u32,
            py: u32,
            fg: u32,
            bg: u32,
            c: char,
            flags: Flags,
        }
        let mut cells: Vec<Cell2D> = Vec::new();
        for indexed in content.display_iter {
            let line = indexed.point.line.0 + offset;
            if !(0..self.term.grid().screen_lines() as i32).contains(&line) {
                continue; // 钳到屏内（防御：迭代区间理论上已对齐）
            }
            let (mut fg, mut bg) = (
                color_to_xrgb(indexed.cell.fg),
                color_to_xrgb(indexed.cell.bg),
            );
            if indexed.cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            // 长按选择高亮：选中格盖选择底色（与网格行同坐标系，滚屏自动跟随）。
            // 宽字符整字扩边：spacer 的格 0 选中 → spacer 也亮；格 0 的 spacer
            // 选中（选词带 spacer 收尾）→ 格 0 也亮——任何钳法下都不劈字
            if let Some(sel) = selection {
                let (line0, col0) = (indexed.point.line.0, indexed.point.column.0 as u32);
                let selected = in_selection(sel.anchor, sel.cursor, line0, col0)
                    || (col0 > 0
                        && indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                        && in_selection(sel.anchor, sel.cursor, line0, col0 - 1))
                    || (indexed.cell.flags.contains(Flags::WIDE_CHAR)
                        && in_selection(sel.anchor, sel.cursor, line0, col0 + 1));
                if selected {
                    bg = SELECT_BG;
                }
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
            // BAR-010：顶部走动态顶带 margin_top（圆角屏下探一整行，
            // 格高随捏合缩放变，顶带跟格高走）
            let (px, py) = (px + MARGIN_X, py + margin_top(self.cell_h));
            if px >= buf_w || py >= buf_h {
                continue; // 窗口比网格小（resize 途中）：裁掉放不下的格
            }
            cells.push(Cell2D {
                px,
                py,
                fg,
                bg,
                c: indexed.cell.c,
                flags: indexed.cell.flags,
            });
        }
        // 第一遍：背景。不满格重画（全帧已填 DEFAULT_BG），非默认背景补色块
        for cell in &cells {
            if cell.bg != DEFAULT_BG {
                frame.fill_rect(cell.px, cell.py, self.cell_w, self.cell_h, cell.bg);
            }
        }
        // 第二遍：字形。空格/控制符（BAR-015：tab 本体）无字形不画；
        // 宽字符第二格（spacer）不画。裁剪宽：宽字符 2 格，其余 1 格——
        // 模糊宽度字符（如 ⇄，宽度判 1 格但 CJK 备用字体是全角字形）的
        // 墨不许溢进下一格（2026-08-21 实拍）
        for cell in &cells {
            if !paintable(cell.c) || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let clip_w = if cell.flags.contains(Flags::WIDE_CHAR) {
                self.cell_w * 2
            } else {
                self.cell_w
            };
            self.draw_glyph(&mut frame, cell.c, cell.px, cell.py, cell.fg, clip_w);
        }
    }

    /// 快捷键行渲染（BAR-017：Java View 被原生 busy 重绘盖掉，改 Rust 自绘——
    /// 覆盖层 UI 的统一模式）。画在帧缓冲底部、键盘 inset 之上的 HEIGHT_PX 带
    /// （键盘弹起时跟着上浮，16777485 实拍：画死在屏底会被键盘盖住）：
    /// 行底 → 圆角药丸键格（修饰键粘滞中换高亮色）→ 标签字形居中
    /// mods = 调用方传入的修饰键粘滞位（input-ime 方案 A：不自读静态，
    /// 状态归 input.modifiers 服务，渲染层只收参数）
    pub fn render_keybar(
        &self,
        buf: &mut [u32],
        buf_w: u32,
        buf_h: u32,
        ime_bottom: u32,
        mods: u8,
    ) {
        use crate::keybar;
        let Some(top) = buf_h
            .checked_sub(ime_bottom)
            .and_then(|b| b.checked_sub(keybar::HEIGHT_PX))
        else {
            return;
        };
        if buf_w == 0 {
            return;
        }
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        frame.fill_rect(0, top, buf_w, keybar::HEIGHT_PX, KEYBAR_BG);
        let cell_w = buf_w / keybar::COLS;
        if cell_w < 8 {
            return; // 窗太窄画不下，保命要紧
        }
        for (row, keys) in keybar::KEYS.iter().enumerate() {
            for (col, kd) in keys.iter().enumerate() {
                if matches!(kd.key, keybar::Key::None) {
                    continue;
                }
                let x = col as u32 * cell_w;
                let y = top + row as u32 * keybar::ROW_H_PX;
                let active = matches!(kd.key, keybar::Key::Modifier(bit) if mods & bit != 0);
                let bg = if active { KEYBAR_MOD_ON } else { KEYBAR_KEY_BG };
                // 圆角药丸键格（内缩出缝，圆角半径 14px）
                frame.fill_round_rect(x + 3, y + 3, cell_w - 6, keybar::ROW_H_PX - 6, 14, bg);
                self.draw_label(&mut frame, kd.label, x, cell_w, y, keybar::ROW_H_PX);
            }
        }
    }

    /// 快捷键行标签：水平居中 + 垂直居中光栅文本。主字体缺字形走 CJK 备用
    /// （↑↓←→ 的命），双缺记 tofu 目击名单后跳过（不画方框吓唬人）
    fn draw_label(&self, frame: &mut Frame<'_>, text: &str, cx: u32, cw: u32, cy: u32, rh: u32) {
        let px = rh as f32 * 0.26; // 字号：行高的 1/4 左右（实拍「太大」后收敛）
        let Some(hm) = self.font.horizontal_line_metrics(px) else {
            return;
        };
        // 逐字挑字体（与 draw_glyph 同规则），顺便算总宽
        let pick = |c: char| -> Option<&fontdue::Font> {
            if self.font.lookup_glyph_index(c) != 0 {
                Some(&self.font)
            } else if let Some(k) = &self.cjk {
                if k.font.lookup_glyph_index(c) != 0 {
                    Some(&k.font)
                } else {
                    None
                }
            } else {
                None
            }
        };
        let mut glyphs = Vec::new();
        let mut width = 0.0f32;
        for c in text.chars() {
            let Some(f) = pick(c) else {
                let mut seen = self.tofu_seen.borrow_mut();
                if !seen.contains(&c) && seen.len() < 16 {
                    seen.push(c); // 标签缺字也上报（↑ 在不在设备字体里，问机器）
                }
                continue;
            };
            let m = f.metrics(c, px);
            glyphs.push((f, c, m.advance_width));
            width += m.advance_width;
        }
        if glyphs.is_empty() {
            return;
        }
        let mut pen_x = cx as f32 + (cw as f32 - width).max(0.0) / 2.0;
        // 垂直居中：行内盒（ascent-descent）放进键格正中
        let baseline = cy as f32 + (rh as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent;
        for (f, c, adv) in glyphs {
            let (m, bmp) = f.rasterize(c, px);
            let top = baseline - m.ymin as f32 - m.height as f32;
            for gy in 0..m.height as u32 {
                let y = top as i64 + i64::from(gy);
                if y < 0 || y >= i64::from(frame.h) {
                    continue;
                }
                for gx in 0..m.width as u32 {
                    let x = (pen_x + m.xmin as f32) as i64 + i64::from(gx);
                    if x < 0 || x >= i64::from(frame.w) {
                        continue;
                    }
                    let a = u32::from(bmp[(gy * m.width as u32 + gx) as usize]);
                    if a > 0 {
                        frame.blend_px(x as u32, y as u32, KEYBAR_LABEL, a);
                    }
                }
            }
            pen_x += adv;
        }
    }

    /// 光栅化单字形并 alpha 混合进帧缓冲。基线对齐（BAR-001）：fontdue
    /// y 轴向上，metrics.ymin 是位图底边相对基线的偏移（下伸字母为负），
    /// 位图顶边（屏坐标）= 格顶 + 基线偏移 - (ymin + 位图高)。
    /// 字体选择：主字体缺该字且备用有 → CJK 三件套（prefer_cjk，两格宽适配）；
    /// 双字体都缺 → 记 tofu 目击名单（主字体画 .notdef 方框）。
    /// clip_w = 右缘裁剪宽（格宽的 1 或 2 倍）：模糊宽度字符（宽度判 1 格
    /// 但落在全角比例的 CJK 字体上，如 ⇄）墨不许溢进下一格的内容区
    fn draw_glyph(&self, frame: &mut Frame<'_>, c: char, px: u32, py: u32, fg: u32, clip_w: u32) {
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
        let clip_right = px as i64 + i64::from(clip_w);
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
                if x < 0 || x >= i64::from(frame.w) || x >= clip_right {
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

/// 快捷键行配色（XRGB，与帧缓冲同格式）
pub const KEYBAR_BG: u32 = 0x0010_1216;
pub const KEYBAR_KEY_BG: u32 = 0x0023_272E;
pub const KEYBAR_MOD_ON: u32 = 0x003E_6FB4;
pub const KEYBAR_LABEL: u32 = 0x00E8_EAED;

/// 长按选择高亮底色（kfmv4 正蓝 #3B82F6，2026-08-21 品牌色板统一——
/// 此前借用的 KEYBAR_MOD_ON 0x3E6FB4 是快捷键行私色，不成套）
pub const SELECT_BG: u32 = 0x003B_82F6;

/// 放大镜（边界拖动中浮窗）：源区 = 触点格 ±5 格宽 × ±3 行高，最近邻 2 倍；
/// 边框 kfmv4 青 #06B6D4（与选中条正蓝同品牌色板），衬底黑
pub const MAG_HALF_COLS: u32 = 5;
pub const MAG_HALF_ROWS: u32 = 3;
pub const MAG_ZOOM: u32 = 2;
pub const MAG_BORDER: u32 = 0x0006_B6D4;
/// 浮窗底缘与触点的间距（不挡手）
pub const MAG_GAP_PX: u32 = 60;

/// 选区边界端点：Start = 归一化后的起端（字典序小），End = 止端
/// （2026-08-21 拖柄废除后改名 SelEnd——柄没了，端点还在）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelEnd {
    Start,
    End,
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

    /// 画圆角矩形（四角半径 r 的圆外像素跳过），快捷键行药丸键用
    fn fill_round_rect(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, color: u32) {
        let r = r.min(w / 2).min(h / 2) as i64;
        for py in 0..h as i64 {
            for px in 0..w as i64 {
                // 角区像素：到角圆心的距离超半径即跳过
                let cx = if px < r {
                    r
                } else if px >= w as i64 - r {
                    w as i64 - r - 1
                } else {
                    px
                };
                let cy = if py < r {
                    r
                } else if py >= h as i64 - r {
                    h as i64 - r - 1
                } else {
                    py
                };
                if (px - cx) * (px - cx) + (py - cy) * (py - cy) > r * r {
                    continue;
                }
                let (ax, ay) = (x as i64 + px, y as i64 + py);
                if ax >= 0 && ay >= 0 && ax < i64::from(self.w) && ay < i64::from(self.h) {
                    self.buf[(ay * i64::from(self.w) + ax) as usize] = color;
                }
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
/// 注：生产已不走这条路（BAR-021 起用 build_vendored 零探测），本函数保留
/// 给考题注入夹具与「探测链」行为的回归钉
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

/// 生产默认构建（BAR-021）：零探测——主/CJK 字体都是编译期内嵌字节，
/// 启动全程不碰 /system/fonts。返回 (视图, "<内嵌主>", Some("<内嵌CJK>"))；
/// 内嵌字节解析失败（不可能，有考题钉）才返回 None。本函数不 panic。
pub fn build_vendored() -> Option<(TermView, String, Option<String>)> {
    let font =
        fontdue::Font::from_bytes(VENDORED_MAIN_FONT, fontdue::FontSettings::default()).ok()?;
    let cjk = fontdue::Font::from_bytes(VENDORED_CJK_FONT, fontdue::FontSettings::default()).ok();
    Some((
        TermView::new(font, cjk, 80, 24, CELL_W, CELL_H),
        "<内嵌主>".to_string(),
        Some("<内嵌CJK>".to_string()),
    ))
}

// ---- trait 层（终端模拟器设计页 §2；插件化边界，方法体一行不动） ----

/// 终端模拟器对象面（服务键 `dyn TermEmuFactory` 产出的实例侧）。
/// `Send` 不含 `Sync`：独占可变持有——类型约束编码状态存活分层（评审裁决 1）。
///
/// 演化纪律（评审裁决 2 边界注记）：方法面 = android_app 现调集合，
/// 新增方法须有调用方先例；自由函数（grid_dims/paintable/颜色表）无状态，
/// 永不进 trait。
pub trait TermEmu: Send {
    fn feed(&mut self, bytes: &[u8]);
    fn resize_cells(&mut self, cols: u32, rows: u32);
    fn cell_size(&self) -> (u32, u32);
    /// 运行期改格尺寸（捏合缩放，android_app 双指手势调用方）
    fn set_cell_size(&mut self, cell_w: u32, cell_h: u32);
    fn render_into(&mut self, buf: &mut [u32], w: u32, h: u32);
    fn render_keybar(&self, buf: &mut [u32], w: u32, h: u32, ime_bottom: u32, mods: u8);
    fn take_tofu_chars(&self) -> Vec<char>;
    fn scroll_lines(&mut self, lines: i32);
    fn scroll_to_bottom(&mut self);
    /// 当前视野纯文本导出（调试闸门 text-req 通道；跟随滚动位置，对齐「所见」）
    fn dump_text(&self) -> String;
    fn mouse_report_active(&self) -> bool;
    fn app_cursor_mode(&self) -> bool;
    fn font_probe(&self, c: char) -> (usize, usize, usize);
    /// 长按选择面（android_app 触摸状态机调用方）
    fn selection_active(&self) -> bool;
    fn select_word_at(&mut self, x: f64, y: f64);
    fn extend_selection(&mut self, x: f64, y: f64);
    fn clear_selection(&mut self);
    fn selected_text(&self) -> Option<String>;
    /// 选区边界/放大镜面（android_app 边界拖动手势调用方）
    fn hit_boundary(&self, x: f64, y: f64) -> Option<SelEnd>;
    fn move_selection_end(&mut self, which: SelEnd, x: f64, y: f64);
    fn render_magnifier(&self, buf: &mut [u32], w: u32, h: u32, x: f64, y: f64);
}

impl TermEmu for TermView {
    fn feed(&mut self, bytes: &[u8]) {
        TermView::feed(self, bytes)
    }
    fn resize_cells(&mut self, cols: u32, rows: u32) {
        TermView::resize_cells(self, cols, rows)
    }
    fn cell_size(&self) -> (u32, u32) {
        TermView::cell_size(self)
    }
    fn set_cell_size(&mut self, cell_w: u32, cell_h: u32) {
        TermView::set_cell_size(self, cell_w, cell_h)
    }
    fn render_into(&mut self, buf: &mut [u32], w: u32, h: u32) {
        TermView::render_into(self, buf, w, h)
    }
    fn render_keybar(&self, buf: &mut [u32], w: u32, h: u32, ime_bottom: u32, mods: u8) {
        TermView::render_keybar(self, buf, w, h, ime_bottom, mods)
    }
    fn take_tofu_chars(&self) -> Vec<char> {
        TermView::take_tofu_chars(self)
    }
    fn scroll_lines(&mut self, lines: i32) {
        TermView::scroll_lines(self, lines)
    }
    fn scroll_to_bottom(&mut self) {
        TermView::scroll_to_bottom(self)
    }
    fn dump_text(&self) -> String {
        TermView::dump_text(self)
    }
    fn mouse_report_active(&self) -> bool {
        TermView::mouse_report_active(self)
    }
    fn app_cursor_mode(&self) -> bool {
        TermView::app_cursor_mode(self)
    }
    fn font_probe(&self, c: char) -> (usize, usize, usize) {
        TermView::font_probe(self, c)
    }
    fn selection_active(&self) -> bool {
        TermView::selection_active(self)
    }
    fn select_word_at(&mut self, x: f64, y: f64) {
        TermView::select_word_at(self, x, y)
    }
    fn extend_selection(&mut self, x: f64, y: f64) {
        TermView::extend_selection(self, x, y)
    }
    fn clear_selection(&mut self) {
        TermView::clear_selection(self)
    }
    fn selected_text(&self) -> Option<String> {
        TermView::selected_text(self)
    }
    fn hit_boundary(&self, x: f64, y: f64) -> Option<SelEnd> {
        TermView::hit_boundary(self, x, y)
    }
    fn move_selection_end(&mut self, which: SelEnd, x: f64, y: f64) {
        TermView::move_selection_end(self, which, x, y)
    }
    fn render_magnifier(&self, buf: &mut [u32], w: u32, h: u32, x: f64, y: f64) {
        TermView::render_magnifier(self, buf, w, h, x, y)
    }
}

/// build 产物：终端实例 + 主/CJK 字体来源名（供调用方诊断上报）
pub type BuiltTerm = (Box<dyn TermEmu>, String, Option<String>);

/// 终端模拟器工厂服务（注册表式、独占绑定 v1）。build 瞬时返回：
/// 内嵌字体解析是毫秒级内存操作（BAR-021 起生产零文件 IO），不违反瞬时返回契约。
pub trait TermEmuFactory: Send + Sync {
    /// 建一台终端；Err = 字体全灭（调用方上报，不算插件失败——裁决 3）。
    /// Ok 附（主字体来源, CJK 字体来源）供调用方诊断上报（现状行为保持）
    fn build(&self) -> Result<BuiltTerm, String>;
}

/// 字体来源：Vendored = 生产（编译期内嵌，零探测，BAR-021）；
/// Probed = 考题注入夹具（按候选路径探测，host 无 /system/fonts）
pub enum FactoryFonts {
    Vendored,
    Probed(&'static [&'static str]),
}

/// alacritty 芯工厂：生产 = 内嵌字体直载；考题 = 候选表探测夹具
pub struct AlacrittyEmuFactory {
    fonts: FactoryFonts,
}

impl AlacrittyEmuFactory {
    /// 生产构造：编译期内嵌字体，零探测
    pub fn vendored() -> Self {
        AlacrittyEmuFactory {
            fonts: FactoryFonts::Vendored,
        }
    }

    /// 注入字体候选表（契约考题用夹具；host 无 /system/fonts）
    pub fn new(candidates: &'static [&'static str]) -> Self {
        AlacrittyEmuFactory {
            fonts: FactoryFonts::Probed(candidates),
        }
    }
}

impl TermEmuFactory for AlacrittyEmuFactory {
    fn build(&self) -> Result<BuiltTerm, String> {
        let built = match &self.fonts {
            FactoryFonts::Vendored => build_vendored(),
            FactoryFonts::Probed(candidates) => build_from_candidates(candidates),
        };
        match built {
            Some((tv, main, cjk)) => Ok((Box::new(tv), main, cjk)),
            None => Err("字体全灭——TermView 建不成".into()),
        }
    }
}
