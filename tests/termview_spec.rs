//! termview_spec.rs — 终端视图考题（A 档布局/颜色纯逻辑 + B 档渲染冒烟钉）
//!
//! 判卷维度：
//! - A 档：grid_dims（零尺寸/非整除/1x1 边界）、cell_origin、ANSI/256 色映射、
//!   反色交换——纯函数考题先行，答案 src/termview.rs。变异抽检：故意改坏答案
//!   （如 grid_dims 改用 ceil 除、色表红绿对调）本文件必须红
//! - B 档：真 Term + 真字体（DejaVuSansMono）渲染冒烟——字形真画出来、
//!   ANSI 红色真出红像素、光标格真反色、CJK 缺字形不 panic

use alacritty_terminal::vte::ansi::{Color, NamedColor};
use kfm_na::termview::{
    self, ANSI_16, CELL_H, CELL_W, DEFAULT_BG, DEFAULT_FG, TermView, cell_origin, color_to_xrgb,
    grid_dims, indexed_color,
};

const HOST_FONT: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf";

fn host_font() -> fontdue::Font {
    let bytes = std::fs::read(HOST_FONT).expect("host 测试字体缺失");
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .expect("fontdue 不认 DejaVuSansMono")
}

fn host_termview(cols: u32, rows: u32) -> TermView {
    TermView::new(host_font(), None, cols, rows, CELL_W, CELL_H)
}

// ---------- A 档：布局数学 ----------

#[test]
fn spec_布局_整除与非整除() {
    // 整除：100x48 窗口 10x24 格 → 10x2
    assert_eq!(grid_dims(100, 48, 10, 24), (10, 2));
    // 非整除向下取整：105x50 → 10x2（余下的半格不算）
    assert_eq!(grid_dims(105, 50, 10, 24), (10, 2));
    // 尖刺常量（2026-08-13 放大一轮：12x24 → 15x30）：1080x2400 屏 15x30 格 → 72x80
    assert_eq!(grid_dims(1080, 2400, CELL_W, CELL_H), (72, 80));
}

#[test]
fn spec_布局_零与装不下的边界() {
    // 窗口 0 边
    assert_eq!(grid_dims(0, 100, 10, 24), (0, 4));
    assert_eq!(grid_dims(100, 0, 10, 24), (10, 0));
    // 单元格 0 边（非法输入防除零）
    assert_eq!(grid_dims(100, 100, 0, 24), (0, 0));
    assert_eq!(grid_dims(100, 100, 10, 0), (0, 0));
    // 装不下一个整格
    assert_eq!(grid_dims(9, 23, 10, 24), (0, 0));
    // 恰好 1x1
    assert_eq!(grid_dims(10, 24, 10, 24), (1, 1));
}

#[test]
fn spec_布局_格坐标到像素原点() {
    assert_eq!(cell_origin(0, 0, 10, 24), (0, 0));
    assert_eq!(cell_origin(1, 0, 10, 24), (10, 0));
    assert_eq!(cell_origin(0, 1, 10, 24), (0, 24));
    assert_eq!(cell_origin(3, 2, 10, 24), (30, 48));
    // 尖刺常量 15x30：右下角 (71,79) → (1065, 2370)
    assert_eq!(cell_origin(71, 79, CELL_W, CELL_H), (1065, 2370));
}

#[test]
fn spec_字号_步进宽不超格宽() {
    // 宽度帽契约：fit_font_px 给出的字号，'M' 步进宽不得超过格宽
    // （否则相邻格字形互相渗透——放大字号后 DejaVuSansMono 自然超宽）
    let font = load_host_font(HOST_FONT);
    let (px, baseline) = termview::fit_font_px(&font, CELL_W, CELL_H);
    let (m, _) = font.rasterize('M', px);
    assert!(
        m.advance_width <= CELL_W as f32 + 0.01,
        "步进宽 {} 必须 ≤ 格宽 {CELL_W}",
        m.advance_width
    );
    assert!(px > 0.0 && px <= CELL_H as f32, "字号必须为正且不超格高");
    assert!(
        baseline > 0.0 && baseline <= CELL_H as f32,
        "基线偏移必须在格内"
    );
}

// ---------- A 档：颜色映射 ----------

#[test]
fn spec_颜色_ansi前16色走表() {
    assert_eq!(color_to_xrgb(Color::Named(NamedColor::Black)), ANSI_16[0]);
    assert_eq!(color_to_xrgb(Color::Named(NamedColor::Red)), ANSI_16[1]);
    assert_eq!(color_to_xrgb(Color::Named(NamedColor::White)), ANSI_16[7]);
    assert_eq!(
        color_to_xrgb(Color::Named(NamedColor::BrightRed)),
        ANSI_16[9]
    );
    assert_eq!(
        color_to_xrgb(Color::Named(NamedColor::BrightWhite)),
        ANSI_16[15]
    );
}

#[test]
fn spec_颜色_默认前景背景() {
    assert_eq!(
        color_to_xrgb(Color::Named(NamedColor::Foreground)),
        DEFAULT_FG
    );
    assert_eq!(
        color_to_xrgb(Color::Named(NamedColor::Background)),
        DEFAULT_BG
    );
}

#[test]
fn spec_颜色_spec直包rgb() {
    use alacritty_terminal::vte::ansi::Rgb;
    assert_eq!(
        color_to_xrgb(Color::Spec(Rgb {
            r: 0x12,
            g: 0xAB,
            b: 0xFF
        })),
        0x0012_ABFF
    );
    assert_eq!(color_to_xrgb(Color::Spec(Rgb { r: 0, g: 0, b: 0 })), 0);
}

#[test]
fn spec_颜色_indexed分段边界() {
    // 0-15 同表
    assert_eq!(indexed_color(0), ANSI_16[0]);
    assert_eq!(indexed_color(15), ANSI_16[15]);
    // 16 = 立方原点（黑）；231 = 立方顶点（白 255,255,255）
    assert_eq!(indexed_color(16), 0);
    assert_eq!(indexed_color(231), 0x00FF_FFFF);
    // 立方取位：n=16+36r+6g+b，level 表 [0,95,135,175,215,255]
    assert_eq!(indexed_color(16 + 36), (95 << 16)); // r=1
    assert_eq!(indexed_color(16 + 6), (95 << 8)); // g=1
    assert_eq!(indexed_color(16 + 1), 95); // b=1
    // 灰阶：232 → 8，255 → 238
    assert_eq!(indexed_color(232), 0x0008_0808);
    assert_eq!(indexed_color(255), 0x00EE_EEEE);
}

// ---------- B 档：渲染冒烟钉（真 Term + 真字体） ----------

/// 帧缓冲里存在非背景色像素
fn has_non_bg(buf: &[u32]) -> bool {
    !buf.iter().all(|&p| p == DEFAULT_BG)
}

#[test]
fn spec_渲染_feed文字后帧缓冲有字形像素() {
    let mut tv = host_termview(24, 6);
    tv.feed(b"hello");
    let mut buf = vec![DEFAULT_FG; (24 * CELL_W * 6 * CELL_H) as usize]; // 污染初值防假绿
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H);
    assert!(has_non_bg(&buf), "feed hello 后必须画出非背景像素");
    // 且必须有背景像素（黑底真刷了）——防「全帧涂满」式假实现
    assert!(buf.contains(&DEFAULT_BG), "黑底必须存在");
}

#[test]
fn spec_渲染_ansi红色出红像素() {
    let mut tv = host_termview(24, 6);
    tv.feed(b"\x1b[31mR");
    let mut buf = vec![0u32; (24 * CELL_W * 6 * CELL_H) as usize];
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H);
    // 红像素：R 通道显著高于 G/B
    assert!(
        buf.iter().any(|&p| {
            let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
            r > 100 && r > g * 2 && r > b * 2
        }),
        "ANSI 31 红必须渲染出红像素"
    );
}

#[test]
fn spec_渲染_光标格反色() {
    let mut tv = host_termview(24, 6);
    tv.feed(b"hello"); // 光标落在 (0行, 5列)——空字符格，无字形
    let mut buf = vec![0u32; (24 * CELL_W * 6 * CELL_H) as usize];
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H);
    // 光标格（5列, 0行）反色后背景为白——该格矩形内必须有接近白的像素；
    // 相邻的空格（6列）不是光标，应全黑
    // 渲染的格原点 = cell_origin + 边距（BAR-005）+ 顶部下探（BAR-010）
    let (cx, cy) = cell_origin(5, 0, CELL_W, CELL_H);
    let (cx, cy) = (cx + termview::MARGIN_X, cy + termview::MARGIN_TOP);
    let buf_w = 24 * CELL_W;
    let mut cursor_white = false;
    let mut neighbor_dark = true;
    for y in cy..cy + CELL_H {
        for x in cx..cx + CELL_W {
            let p = buf[(y * buf_w + x) as usize];
            if p == DEFAULT_FG {
                cursor_white = true;
            }
        }
    }
    let (nx, _) = cell_origin(6, 0, CELL_W, CELL_H);
    let nx = nx + termview::MARGIN_X;
    for y in cy..cy + CELL_H {
        for x in nx..nx + CELL_W {
            if buf[(y * buf_w + x) as usize] != DEFAULT_BG {
                neighbor_dark = false;
            }
        }
    }
    assert!(cursor_white, "光标格必须反色（白底）");
    assert!(neighbor_dark, "非光标的空格必须保持黑底");
}

#[test]
fn spec_渲染_cjk缺字形不panic() {
    let mut tv = host_termview(24, 6);
    // DejaVuSansMono 无 CJK 字形——tofu 方框或空位图，绝不许 panic
    tv.feed("中文混排 English 123".as_bytes());
    let mut buf = vec![0u32; (24 * CELL_W * 6 * CELL_H) as usize];
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H);
    assert!(has_non_bg(&buf), "英文部分必须画出来");
}

#[test]
fn spec_渲染_滚屏不panic且新内容在画面() {
    let mut tv = host_termview(10, 3);
    // 灌超屏内容逼滚屏（escape 换行 + 回车）
    for i in 0..10 {
        tv.feed(format!("line-{i}\r\n").as_bytes());
    }
    let mut buf = vec![0u32; (10 * CELL_W * 3 * CELL_H) as usize];
    tv.render_into(&mut buf, 10 * CELL_W, 3 * CELL_H);
    assert!(has_non_bg(&buf));
}

#[test]
fn spec_渲染_resize后正常() {
    let mut tv = host_termview(24, 6);
    tv.feed(b"before");
    tv.resize_cells(10, 2);
    tv.feed(b"\r\nafter");
    let mut buf = vec![0u32; (10 * CELL_W * 2 * CELL_H) as usize];
    tv.render_into(&mut buf, 10 * CELL_W, 2 * CELL_H);
    assert!(has_non_bg(&buf));
    // 0 维钳 1 不 panic
    tv.resize_cells(0, 0);
    tv.render_into(&mut buf, 10 * CELL_W, 2 * CELL_H);
}

// ---------- A 档：字体加载 ----------

/// 帧缓冲里某格的墨水纵向跨度 → (最上, 最下) 非背景像素行（相对格原点）。
/// 无墨水的格返回 (CELL_H, 0)（上下颠倒即为空）。
/// 注意含边距偏移——渲染的格原点 = cell_origin + (MARGIN_X, MARGIN_TOP)
fn cell_ink_span(buf: &[u32], buf_w: u32, col: u32, row: u32) -> (u32, u32) {
    let (ox, oy) = cell_origin(col, row, CELL_W, CELL_H);
    let (ox, oy) = (ox + termview::MARGIN_X, oy + termview::MARGIN_TOP);
    let (mut top, mut bot) = (CELL_H, 0);
    for y in 0..CELL_H {
        for x in 0..CELL_W {
            if buf[((oy + y) * buf_w + ox + x) as usize] != DEFAULT_BG {
                top = top.min(y);
                bot = bot.max(y);
            }
        }
    }
    (top, bot)
}

#[test]
fn spec_bar001_基线对齐_同基线字母底边对齐() {
    let mut tv = host_termview(8, 2);
    tv.feed(b"Axp"); // 光标落在第 4 格，不干扰前 3 格
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + 2 * CELL_H + termview::MARGIN_Y;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    let (top_a, bot_a) = cell_ink_span(&buf, buf_w, 0, 0);
    let (top_x, bot_x) = cell_ink_span(&buf, buf_w, 1, 0);
    let (_, bot_p) = cell_ink_span(&buf, buf_w, 2, 0);
    // BAR-001 病灶：竖直居中让高矮字母各自为政（里倒歪斜）。
    // 契约：同坐基线的字母底边对齐、高字母顶边更高、下伸字母探过基线
    assert_eq!(bot_a, bot_x, "A 与 x 同坐基线：底边必须对齐");
    assert!(top_a < top_x, "A 比 x 高：顶边必须更高");
    assert!(bot_p > bot_x, "p 有下伸：底边必须探过基线");
}

#[test]
fn spec_字体_候选全灭落内嵌等宽() {
    // 契约（BAR-003 后改写）：路径候选全灭不再返回 None——
    // 编译期内嵌的 DejaVuSansMono 兜底，任何设备都有及格等宽终端字体
    let (path, font) =
        termview::load_font(&["/nonexistent/a.ttf", "/nonexistent/b.ttf"]).expect("内嵌字体兜底");
    assert_eq!(path, "<内嵌>");
    assert!(termview::font_usable(&font, 'M'));
    assert!(termview::font_monospaced(&font));
}

#[test]
fn spec_字体_host候选命中() {
    let (path, _font) = termview::load_font(&["/nonexistent/x.ttf", HOST_FONT])
        .expect("DejaVuSansMono 必须加载成功");
    assert_eq!(path, HOST_FONT);
}

/// 内嵌兜底字体（编译期 include_bytes!）：字节必须真在包里、真能用。
/// 钉住防「文件没提交进仓库/路径写错/复制成别的字体」
#[test]
fn spec_字体_内嵌字节可直接用() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_MONO_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌字体字节必须可解析");
    assert!(termview::font_usable(&font, 'M'));
    assert!(termview::font_monospaced(&font));
}

/// CFF 轮廓字体（NimbusMonoPS）：fontdue 0.9 能载能画西文，但中文字形
/// 光栅全空（w=0 h=0 ink=0，2026-08-13 host 实测）——空光栅判定的活教材
const HOST_CFF_FONT: &str = "/usr/share/fonts/opentype/urw-base35/NimbusMonoPS-Regular.otf";
/// 比例字体活教材（BAR-003 病灶同款：真机 Roboto 即比例字体）
const HOST_PROPORTIONAL_FONT: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

fn load_host_font(path: &str) -> fontdue::Font {
    let bytes = std::fs::read(path).expect("host 测试字体缺失");
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).expect("fontdue 不认该字体")
}

#[test]
fn spec_字体_空光栅判不合格() {
    let font = load_host_font(HOST_CFF_FONT);
    assert!(
        !termview::font_usable(&font, '中'),
        "空光栅（CFF 字体缺中文字形）必须判不合格"
    );
    assert!(
        termview::font_usable(&font, 'M'),
        "同字体的正常西文字形必须判合格"
    );
}

#[test]
fn spec_字体_真字形判合格() {
    let font = load_host_font(HOST_FONT);
    assert!(termview::font_usable(&font, 'M'));
    // DejaVu 无中文字形但 .notdef 豆腐块有墨（host 实测 ink=150）——
    // 「有墨」与「是对的字」是两回事，判定只管前者
    assert!(termview::font_usable(&font, '中'));
}

#[test]
fn spec_字体_等宽判定() {
    // BAR-003：终端网格按定宽格摆字形，比例字体（i 窄 m 宽）摆进去
    // 间距忽近忽远。契约：'i' 与 'M' 步进宽相等才算终端可用
    assert!(termview::font_monospaced(&load_host_font(HOST_FONT)));
    assert!(termview::font_monospaced(&load_host_font(HOST_CFF_FONT)));
    assert!(
        !termview::font_monospaced(&load_host_font(HOST_PROPORTIONAL_FONT)),
        "比例字体必须判非等宽（真机 Roboto 同款病灶）"
    );
}

#[test]
fn spec_字体_加载跳过比例字体() {
    // 比例字体在前、等宽在后：必须跳过比例选等宽（真机场景复刻：
    // Roboto 在前会被挑中，必须让位给后面的等宽）
    let (path, _font) =
        termview::load_font(&[HOST_PROPORTIONAL_FONT, HOST_FONT]).expect("必须命中等宽候选");
    assert_eq!(path, HOST_FONT);
}

// ---------- A 档：边距（BAR-005 边缘半字） ----------

#[test]
fn spec_边距_首格不贴边() {
    // BAR-005 病灶：网格从 (0,0) 画起，边缘字符被屏幕圆角/曲面切半。
    // BAR-010：顶带再下探一整行（MARGIN_TOP = MARGIN_Y + CELL_H）——
    // 圆角屏吃首行首字符（2026-08-13 实拍）。
    // 契约：帧缓冲四周一圈边距带内必须是纯背景，字形墨水全部在带内之后；
    // 顶带必须是一整行高（变异抽检：MARGIN_TOP 改回 MARGIN_Y 本考题必须红）
    assert_eq!(termview::MARGIN_TOP, termview::MARGIN_Y + CELL_H);
    let mut tv = host_termview(8, 2);
    tv.feed(b"A");
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 2 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    for y in 0..buf_h {
        for x in 0..buf_w {
            if x < termview::MARGIN_X
                || y < termview::MARGIN_TOP
                || x >= buf_w - termview::MARGIN_X
                || y >= buf_h - termview::MARGIN_Y
            {
                assert_eq!(
                    buf[(y * buf_w + x) as usize],
                    DEFAULT_BG,
                    "边距带 ({x},{y}) 必须是纯背景"
                );
            }
        }
    }
    // 墨水必须真的出现在边距之后的首格区域（防「全帧涂黑」式假绿）
    let mut ink = false;
    for y in termview::MARGIN_TOP..buf_h {
        for x in termview::MARGIN_X..buf_w {
            if buf[(y * buf_w + x) as usize] != DEFAULT_BG {
                ink = true;
            }
        }
    }
    assert!(ink, "边距之后必须有字形墨水");
}

// ---------- A 档：CJK 判定与备用字体 ----------

#[test]
fn spec_cjk_按覆盖挑选() {
    use termview::prefer_cjk;
    let mono = host_font(); // DejaVuSansMono：无 CJK、无盲文（host 实测 idx=0）
    let sans = load_host_font(HOST_PROPORTIONAL_FONT); // DejaVuSans：有盲文、无 CJK
    // 主字体有的（西文/制表符）→ 不换（保等宽 crisp）
    assert!(!prefer_cjk(&mono, &sans, 'A'));
    assert!(!prefer_cjk(&mono, &sans, '─'));
    // 主字体缺、备用有（盲文转动点 ⠋）→ 换备用
    assert!(
        prefer_cjk(&mono, &sans, '⠋'),
        "主字体缺盲文、备用有：必须换备用（TUI 转动点同款场景）"
    );
    // 主字体缺、备用也缺（'中'：DejaVu 双雄都没 CJK）→ 不换，主字体 tofu
    assert!(!prefer_cjk(&mono, &sans, '中'));
    assert!(!prefer_cjk(&mono, &mono, '中'));
}

#[test]
fn spec_字号_cjk宽度帽() {
    // CJK 全角字占两格：'中' 步进宽不得超过 2 格宽
    let font = host_font();
    let (px, _) = termview::fit_cjk_px(&font, 2 * CELL_W, CELL_H);
    let (m, _) = font.rasterize('中', px);
    assert!(
        m.advance_width <= 2.0 * CELL_W as f32 + 0.01,
        "CJK 步进宽 {} 必须 ≤ 两格宽 {}",
        m.advance_width,
        2 * CELL_W
    );
}

#[test]
fn spec_渲染_cjk备用字体上屏() {
    // 主字体无 CJK 字形时，备用字体接管——host 双 DejaVu 画 tofu 也必须有墨，
    // 且绝不 panic（宽字符 + 占位格链路）
    let mut tv = TermView::new(host_font(), Some(host_font()), 8, 2, CELL_W, CELL_H);
    tv.feed("中文A".as_bytes());
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 2 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    assert!(buf.iter().any(|&p| p != DEFAULT_BG), "CJK 必须有墨");
}

#[test]
fn spec_渲染_tofu目击名单() {
    // 双字体都缺的字符进目击名单（方框的真身 census）；
    // 有覆盖的不进；取走后清空（防重复上报刷屏）
    let mut tv = TermView::new(host_font(), Some(host_font()), 8, 2, CELL_W, CELL_H);
    tv.feed("A\u{E000}\u{280B}".as_bytes()); // A 有字形；PUA 私用区、盲文双缺
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 2 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    let tofu = tv.take_tofu_chars();
    assert!(tofu.contains(&'\u{E000}'), "PUA 私用区字符必须目击");
    assert!(tofu.contains(&'\u{280B}'), "双缺的盲文必须目击");
    assert!(!tofu.contains(&'A'), "有字形的字符不许目击");
    assert!(tv.take_tofu_chars().is_empty(), "取走后必须清空");
}

#[test]
fn spec_渲染_tab控制符不落墨不进目击名单() {
    // BAR-015 病灶：alacritty put_tab 把 '\t' 本体写进格（为了选中/复制能还原
    // tab），渲染层照单全收——设备主字体（DroidSansMono）没有 tab 字形 →
    // ls 列对齐的 tab 全画成方框（2026-08-14 实拍：文件夹名后方框，
    // 目击名单实锤 U+0009）。
    // 契约钉在纯函数 paintable 上（A 档）：控制符（C0/C1/DEL）与空格一样
    // 不上屏。注意 host 的 DejaVuSansMono 有 tab 空白字形，像素层面咬不住
    // 这条（光栅全空，修不修都绿）——所以渲染层必须经 paintable 过滤，
    // 本考题直接判 paintable 本身（变异抽检：摘掉 is_control 必须红）
    assert!(!termview::paintable('\t'), "tab 不许上屏");
    assert!(!termview::paintable('\u{0}'), "NUL 不许上屏");
    assert!(!termview::paintable('\u{7f}'), "DEL 不许上屏");
    assert!(!termview::paintable('\u{1b}'), "ESC 不许上屏");
    assert!(!termview::paintable(' '), "空格不许上屏");
    assert!(termview::paintable('a'), "普通字符必须上屏");
    assert!(termview::paintable('中'), "CJK 必须上屏");
    // B 档冒烟：tab 的推进语义不受影响——'b' 落在下一个 tab stop（第 8 列），
    // tab 占据的列无墨，tab 不进 tofu 目击名单
    let mut tv = TermView::new(host_font(), Some(host_font()), 16, 2, CELL_W, CELL_H);
    tv.feed(b"a\tb");
    let buf_w = 2 * termview::MARGIN_X + 16 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 2 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    assert!(!tv.take_tofu_chars().contains(&'\t'), "tab 不许进目击名单");
    let cell_ink = |buf: &[u32], col: u32| -> usize {
        let (x0, y0) = cell_origin(col, 0, CELL_W, CELL_H);
        let (x0, y0) = (x0 + termview::MARGIN_X, y0 + termview::MARGIN_TOP);
        let mut n = 0;
        for y in y0..y0 + CELL_H {
            for x in x0..x0 + CELL_W {
                if buf[(y * buf_w + x) as usize] != DEFAULT_BG {
                    n += 1;
                }
            }
        }
        n
    };
    assert!(cell_ink(&buf, 0) > 0, "'a' 必须有墨");
    for col in 1..8u32 {
        assert_eq!(cell_ink(&buf, col), 0, "tab 占据的列 {col} 必须无墨");
    }
    assert!(cell_ink(&buf, 8) > 0, "'b' 必须落在 tab stop 第 8 列");
}
