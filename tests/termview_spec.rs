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

/// 测试字体夹具双环境解析（档位 2 手机自举，2026-08-15）：服务器在
/// /usr/share/fonts，手机 Termux 在 $PREFIX/share/fonts——同名 DejaVu/Nimbus
/// 文件，度量一致才能当 A 档固定夹具。NimbusMonoPS.otf 手机没有，由服务器
/// 拷至 ~/kfm-na-toolchain/fonts/
fn fixture(cands: &[&str]) -> String {
    for c in cands {
        if std::path::Path::new(c).exists() {
            return (*c).to_string();
        }
    }
    panic!("host 测试字体缺失: {cands:?}");
}

fn host_mono() -> String {
    fixture(&[
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    ])
}

fn host_cff() -> String {
    fixture(&[
        "/usr/share/fonts/opentype/urw-base35/NimbusMonoPS-Regular.otf",
        "/data/data/com.termux/files/home/kfm-na-toolchain/fonts/NimbusMonoPS-Regular.otf",
    ])
}

fn host_proportional() -> String {
    fixture(&[
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSans.ttf",
    ])
}

fn host_font() -> fontdue::Font {
    let bytes = std::fs::read(host_mono()).expect("host 测试字体缺失");
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
    // 尖刺常量（2026-08-13 放大一轮：12x24 → 15x30；2026-08-21 再放大：
    // 18x36，用户两次抱怨「太小」）：1080x2400 屏 18x36 格 → 60x66
    assert_eq!(grid_dims(1080, 2400, CELL_W, CELL_H), (60, 66));
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
    // 基准常量 18x36（2026-08-21 放大）：右下角 (71,79) → (1278, 2844)
    assert_eq!(cell_origin(71, 79, CELL_W, CELL_H), (1278, 2844));
}

#[test]
fn spec_字号_步进宽不超格宽() {
    // 宽度帽契约：fit_font_px 给出的字号，'M' 步进宽不得超过格宽
    // （否则相邻格字形互相渗透——放大字号后 DejaVuSansMono 自然超宽）
    let font = load_host_font(&host_mono());
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
    let (path, _font) = termview::load_font(&["/nonexistent/x.ttf", &host_mono()])
        .expect("DejaVuSansMono 必须加载成功");
    assert_eq!(path, host_mono());
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

// CFF 轮廓字体（NimbusMonoPS，host_cff() 夹具）：fontdue 0.9 能载能画西文，
// 但中文字形光栅全空（w=0 h=0 ink=0，2026-08-13 host 实测）——空光栅判定的活教材
// 比例字体（host_proportional() 夹具）：BAR-003 病灶同款（真机 Roboto 即比例字体）
fn load_host_font(path: &str) -> fontdue::Font {
    let bytes = std::fs::read(path).expect("host 测试字体缺失");
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).expect("fontdue 不认该字体")
}

#[test]
fn spec_字体_空光栅判不合格() {
    let font = load_host_font(&host_cff());
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
    let font = load_host_font(&host_mono());
    assert!(termview::font_usable(&font, 'M'));
    // DejaVu 无中文字形但 .notdef 豆腐块有墨（host 实测 ink=150）——
    // 「有墨」与「是对的字」是两回事，判定只管前者
    assert!(termview::font_usable(&font, '中'));
}

#[test]
fn spec_字体_等宽判定() {
    // BAR-003：终端网格按定宽格摆字形，比例字体（i 窄 m 宽）摆进去
    // 间距忽近忽远。契约：'i' 与 'M' 步进宽相等才算终端可用
    assert!(termview::font_monospaced(&load_host_font(&host_mono())));
    assert!(termview::font_monospaced(&load_host_font(&host_cff())));
    assert!(
        !termview::font_monospaced(&load_host_font(&host_proportional())),
        "比例字体必须判非等宽（真机 Roboto 同款病灶）"
    );
}

#[test]
fn spec_字体_加载跳过比例字体() {
    // 比例字体在前、等宽在后：必须跳过比例选等宽（真机场景复刻：
    // Roboto 在前会被挑中，必须让位给后面的等宽）
    let (path, _font) =
        termview::load_font(&[&host_proportional(), &host_mono()]).expect("必须命中等宽候选");
    assert_eq!(path, host_mono());
}

#[test]
fn spec_字体_体积闸跳过巨物() {
    // BAR-020 病灶：NotoSansCJK.ttc(32MB)/DroidSansFallbackBBK(44MB)每次
    // 启动全量解析再被探针扔掉（表面建成→TermView 建成实测 6 秒）。
    // 体积闸：超 MAX_MAIN_FONT_BYTES 连读都不读。巨物在前也必须落到
    // 后面的及格等宽
    let giant = std::env::temp_dir().join("kfm-na-spec-giant-font.ttf");
    std::fs::write(
        &giant,
        vec![0u8; (termview::MAX_MAIN_FONT_BYTES + 1) as usize],
    )
    .expect("巨物夹具写不进");
    let (path, _font) = termview::load_font(&[giant.to_str().unwrap(), &host_mono()])
        .expect("巨物被闸后必须命中等宽候选");
    std::fs::remove_file(&giant).ok();
    assert_eq!(path, host_mono());
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
    let sans = load_host_font(&host_proportional()); // DejaVuSans：有盲文、无 CJK
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

#[test]
fn spec_滚动_scroll_lines驱动display_offset() {
    // 触摸滚动的 B 档钉：scroll_lines 必须真的驱动 alacritty 的 display_offset
    // （正 = 看历史），scroll_to_bottom 必须贴回 0；越界由 alacritty 自钳
    // （滚过历史顶 = 停在历史行数，不许 panic 不许穿透）
    let mut tv = host_termview(8, 10);
    for i in 0..30 {
        tv.feed(format!("L{i:02}\r\n").as_bytes());
    }
    assert_eq!(tv.display_offset(), 0, "新输出必须贴底");
    tv.scroll_lines(3);
    assert_eq!(tv.display_offset(), 3, "+3 行必须看历史");
    tv.scroll_lines(-1);
    assert_eq!(tv.display_offset(), 2, "-1 行必须回新");
    tv.scroll_lines(999);
    assert_eq!(
        tv.display_offset(),
        21,
        "滚过历史顶必须钳住（30 行内容+末尾换行=31 行，历史 31-10=21）"
    );
    tv.scroll_to_bottom();
    assert_eq!(tv.display_offset(), 0, "回底必须贴 0");
}

#[test]
fn spec_滚动_历史行必须画上屏() {
    // BAR-016 病灶①：滚进历史后 alacritty 给的行号是负的（Line(-offset)），
    // render_into 一句 line < 0 就跳过 + 像素行直接用绝对行号——历史行不画、
    // 内容不随偏移移动，净效果是每滚一行底部黑一行（实拍「从下到上一行行
    // 消失」）。契约：屏行 = 网格行 + display_offset，滚 3 行后顶行必须出墨
    // （历史行 L18 上了屏），底行也必须有墨（不许黑带）
    let mut tv = host_termview(8, 10);
    for i in 0..30 {
        tv.feed(format!("L{i:02}\r\n").as_bytes());
    }
    tv.scroll_lines(3);
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 10 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    let row_ink = |row: u32| -> usize {
        let y0 = termview::MARGIN_TOP + row * CELL_H;
        let mut n = 0;
        for y in y0..y0 + CELL_H {
            for x in termview::MARGIN_X..buf_w - termview::MARGIN_X {
                if buf[(y * buf_w + x) as usize] != DEFAULT_BG {
                    n += 1;
                }
            }
        }
        n
    };
    assert!(row_ink(0) > 0, "滚 3 行后顶行必须是历史行（有墨），不许黑");
    assert!(row_ink(9) > 0, "底行必须有内容，不许从下到上黑");
}

#[test]
fn spec_滚动_鼠标上报模式识别() {
    // BAR-016 病灶②配套：tmux/kimicode 开鼠标上报（?1000h 等）时，
    // 滚屏必须翻成滚轮事件发给 PTY（alt screen 没有本地历史）。
    // 契约：默认 false；?1000h 或 ?1006h 置位后 true
    let mut tv = host_termview(8, 2);
    assert!(!tv.mouse_report_active(), "默认必须不上报");
    tv.feed(b"\x1b[?1000h\x1b[?1006h");
    assert!(tv.mouse_report_active(), "?1000h 置位后必须识别为上报模式");
}

#[test]
fn spec_模式_应用光标模式识别() {
    // 快捷键行方向键/End 的序列分岔钉：默认普通模式（CSI），
    // 对端开 ?1h 后必须识别（SS3）——vim/kimicode 方向键靠它活
    let mut tv = host_termview(8, 2);
    assert!(!tv.app_cursor_mode(), "默认必须是普通模式");
    tv.feed(b"\x1b[?1h");
    assert!(tv.app_cursor_mode(), "?1h 置位后必须是应用光标模式");
    tv.feed(b"\x1b[?1l");
    assert!(!tv.app_cursor_mode(), "?1l 复位后必须回普通模式");
}

#[test]
fn spec_快捷键行_渲染冒烟() {
    // BAR-017 二稿的 B 档钉：①行画出来了（键格色真上屏，标签真有墨）；
    // ②键盘 inset 300 时行整体抬 300px（原屏底位置必须是背景）；
    // ③修饰键粘滞中键格换高亮色
    // （2026-08-16 迁移：评审明示批准——断言一字不改，render_keybar 改吃
    // mods 参数，修饰键态不再走进程静态，input-ime 插件化方案 A）
    use kfm_na::keybar;
    let tv = host_termview(8, 2);
    let (w, h) = (700u32, 740u32);
    let mut buf = vec![DEFAULT_BG; (w * h) as usize];
    tv.render_keybar(&mut buf, w, h, 0, 0);
    // ESC 键格（第 1 列上排）左缘中段必须是键格色
    // （中心是标签字形的位置，取不到底色）
    let esc_cx = 8u32;
    let esc_cy = h - keybar::HEIGHT_PX + keybar::ROW_H_PX / 2;
    assert_eq!(
        buf[(esc_cy * w + esc_cx) as usize],
        termview::KEYBAR_KEY_BG,
        "键格色必须上屏"
    );
    // 键格里必须有标签墨（非键底色非行底色的像素存在）
    let mut ink = false;
    for y in (h - keybar::HEIGHT_PX)..(h - keybar::HEIGHT_PX + keybar::ROW_H_PX) {
        for x in 0..100u32 {
            let p = buf[(y * w + x) as usize];
            if p != termview::KEYBAR_KEY_BG && p != termview::KEYBAR_BG && p != DEFAULT_BG {
                ink = true;
            }
        }
    }
    assert!(ink, "ESC 标签必须有墨");
    // 键盘弹起 300px：行整体抬 300，原位置（被键盘盖住）必须是背景
    let mut buf2 = vec![DEFAULT_BG; (w * h) as usize];
    tv.render_keybar(&mut buf2, w, h, 300, 0);
    assert_eq!(
        buf2[(esc_cy * w + esc_cx) as usize],
        DEFAULT_BG,
        "键盘盖住的原行位必须是背景"
    );
    assert_eq!(
        buf2[((esc_cy - 300) * w + esc_cx) as usize],
        termview::KEYBAR_KEY_BG,
        "行必须跟着键盘上浮 300px"
    );
    // 修饰键高亮：点亮 CTRL（局部实例，不碰全局态），下排第 2 列键格必须换色
    let mods = keybar::ModifierState::new();
    mods.toggle(keybar::MOD_CTRL);
    let mut buf3 = vec![DEFAULT_BG; (w * h) as usize];
    tv.render_keybar(&mut buf3, w, h, 0, mods.peek());
    let ctrl_cy = h - keybar::HEIGHT_PX + keybar::ROW_H_PX + keybar::ROW_H_PX / 2;
    assert_eq!(
        buf3[(ctrl_cy * w + 108) as usize],
        termview::KEYBAR_MOD_ON,
        "粘滞中的修饰键必须高亮"
    );
}

// ---------- A 档：生产内嵌字体（BAR-021，2026-08-18） ----------

/// 内嵌主字体（build.rs 编译期选择：local/ 覆盖 > DejaVuSansMono）：
/// 必须可解析、能画、等宽。两种来源（本机商业像素字体 / 开源占位）都要过
#[test]
fn spec_bar021_内嵌主字体_可用且等宽() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_MAIN_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌主字体必须可解析");
    assert!(termview::font_usable(&font, 'M'), "内嵌主字体必须能画 M");
    assert!(
        termview::font_monospaced(&font),
        "内嵌主字体必须等宽（'i' 与 'M' 步进一致）"
    );
}

/// 内嵌 CJK 字体：'中' 必须是真字形（非 tofu）、框线 '─' 在位、
/// 全角步进 = 半角两倍（终端双格几何的命根）
#[test]
fn spec_bar021_内嵌cjk字体_真字形且双宽() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析");
    assert!(
        font.lookup_glyph_index('中') != 0,
        "CJK 字体的 '中' 必须是真字形（lookup 非 0，豆腐块不算）"
    );
    assert!(
        font.lookup_glyph_index('─') != 0,
        "CJK 字体必须有框线 '─'（tmux/TUI 边框命根）"
    );
    let (m_cjk, _) = font.rasterize('中', CELL_H as f32);
    let (m_half, _) = font.rasterize('M', CELL_H as f32);
    let ratio = m_cjk.advance_width / m_half.advance_width;
    assert!(
        (ratio - 2.0).abs() < 0.05,
        "全角步进必须是半角两倍（实得 {ratio}）"
    );
}

/// 窄字符居中钉（BAR-021 烘焙管线实拍病灶：lsb=0 让 freetype 系渲染器把
/// i/l/| 贴到格子左缘）。契约：窄字符墨迹中心必须落在步进中心 ±15% 内。
/// 变异抽检：把判据中心改成 0（贴左）重跑，本考题必须红
#[test]
fn spec_bar021_内嵌主字体_窄字符居中() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_MAIN_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌主字体必须可解析");
    for c in ['i', 'l', '|', '1', 'I'] {
        let (m, bmp) = font.rasterize(c, CELL_H as f32);
        assert!(m.width > 0 && bmp.iter().any(|&a| a > 0), "'{c}' 必须有墨");
        let ink_center = m.xmin as f32 + m.width as f32 / 2.0;
        let cell_center = m.advance_width / 2.0;
        let off = (ink_center - cell_center).abs() / m.advance_width;
        assert!(
            off < 0.15,
            "'{c}' 墨迹中心偏离步进中心 {:.0}%（阈 15%）——局左/局右病灶",
            off * 100.0
        );
    }
}

/// 生产默认零探测钉：vendored 工厂的产物来源名必须标记内嵌——
/// 启动路径碰 /system/fonts 的日子（BAR-020 病灶）不许回来
#[test]
fn spec_bar021_生产默认_零探测() {
    let factory = termview::AlacrittyEmuFactory::vendored();
    let (_tv, main, cjk) = termview::TermEmuFactory::build(&factory).expect("内嵌字体必须建成终端");
    assert!(main.contains("内嵌"), "主字体来源必须内嵌，实得 {main}");
    assert!(
        cjk.as_deref().unwrap_or("").contains("内嵌"),
        "CJK 字体来源必须内嵌，实得 {cjk:?}"
    );
}

/// 终端符号补丁钉（BAR-022：纯 GB2312 子集裁掉了盲文转动点/方块/几何符号，
/// 真机 U+25BD ▽ tofu 目击刷屏）。契约：内嵌 CJK/符号 fallback 字体必须
/// 覆盖补丁表代表字符——盲文（kimi code 转动点）、方块、几何、箭头、框线
#[test]
fn spec_bar022_内嵌cjk字体_终端符号补丁覆盖() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析");
    for c in ['⠋', '█', '▽', '→', '─'] {
        assert!(
            font.lookup_glyph_index(c) != 0,
            "内嵌 CJK/符号字体缺 {c}（U+{:04X}）——补丁表被裁掉了？",
            c as u32
        );
    }
}

// ---------- A 档：捏合缩放（2026-08-21，用户两次抱怨「太小」+ 双指调字号） ----------

#[test]
fn spec_缩放_顶边距随格高走() {
    // BAR-010 语义动态化：顶带 = 常规边距 + 一整行，格高变顶带跟着变。
    // 基准格高下动态版必须等于常量版（旧考题 spec_边距_首格不贴边 的
    // MARGIN_TOP == MARGIN_Y + CELL_H 钉继续有效）
    // 变异抽检：margin_top 改回恒定 MARGIN_Y 本考题必须红
    assert_eq!(termview::margin_top(CELL_H), termview::MARGIN_TOP);
    assert_eq!(termview::margin_top(20), termview::MARGIN_Y + 20);
    assert_eq!(termview::margin_top(90), termview::MARGIN_Y + 90);
}

#[test]
fn spec_缩放_捏合钳制纯函数() {
    use termview::{CELL_H_MAX, CELL_H_MIN, CELL_W_MAX, CELL_W_MIN, pinch_cell_size};
    // 恒等：ratio 1.0 回基准
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, 1.0), (CELL_W, CELL_H));
    // 正常缩放：18x36 × 1.5 = 27x54；× 2.0 = 36x72
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, 1.5), (27, 54));
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, 2.0), (36, 72));
    // 钳制边界：暴捏/暴收都停在可读区间（变异抽检：摘掉 clamp 必须红）
    assert_eq!(
        pinch_cell_size(CELL_W, CELL_H, 10.0),
        (CELL_W_MAX, CELL_H_MAX)
    );
    assert_eq!(
        pinch_cell_size(CELL_W, CELL_H, 0.01),
        (CELL_W_MIN, CELL_H_MIN)
    );
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, 100.0), (45, 90));
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, 0.001), (10, 20));
    // 非法输入（NaN/0/负/无穷）落基准钳制值，不许把字号打飞
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, f64::NAN), (CELL_W, CELL_H));
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, 0.0), (CELL_W, CELL_H));
    assert_eq!(pinch_cell_size(CELL_W, CELL_H, -1.0), (CELL_W, CELL_H));
    assert_eq!(
        pinch_cell_size(CELL_W, CELL_H, f64::INFINITY),
        (CELL_W, CELL_H)
    );
}

/// 数一格内的非背景墨像素（set_cell_size 重算字号的判卷尺：
/// 格放大 → 字号重算 → 同字符墨变多）
fn cell_ink_count(tv: &mut TermView, buf_w: u32, buf_h: u32, col: u32, row: u32) -> usize {
    let (cw, ch) = tv.cell_size();
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    let (x0, y0) = cell_origin(col, row, cw, ch);
    let (x0, y0) = (x0 + termview::MARGIN_X, y0 + termview::margin_top(ch));
    let mut n = 0;
    for y in y0..y0 + ch {
        for x in x0..x0 + cw {
            if buf[(y * buf_w + x) as usize] != DEFAULT_BG {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn spec_缩放_set_cell_size重算字几何() {
    let mut tv = host_termview(10, 3);
    tv.feed(b"M");
    let big_buf = |cw: u32, ch: u32| {
        (
            2 * termview::MARGIN_X + 10 * cw,
            termview::margin_top(ch) + 3 * ch + termview::MARGIN_Y,
        )
    };
    // 基准 18x36 的墨量
    let (bw, bh) = big_buf(CELL_W, CELL_H);
    let ink_base = cell_ink_count(&mut tv, bw, bh, 0, 0);
    // 放大到 27x54：cell_size 跟上、墨量必须显著变多（font_px 真重算——
    // 变异抽检：set_cell_size 只改 cell_w/h 不重算 font_px 必须红）
    tv.set_cell_size(27, 54);
    assert_eq!(tv.cell_size(), (27, 54));
    let (bw, bh) = big_buf(27, 54);
    let ink_big = cell_ink_count(&mut tv, bw, bh, 0, 0);
    assert!(
        ink_big > ink_base * 2,
        "格放大 1.5 倍墨量必须显著增长（基准 {ink_base} → 放大 {ink_big}）"
    );
    // 0 维钳 1 不 panic（resize_cells 同款先例）
    tv.set_cell_size(0, 0);
    assert_eq!(tv.cell_size(), (1, 1));
    let mut tiny = vec![0u32; 64];
    tv.render_into(&mut tiny, 8, 8);
    // 设回不 panic + resize 跟随不 panic（android_app 链路：set_cell_size
    // 后必跟 apply_window_size → resize_cells）
    tv.set_cell_size(CELL_W, CELL_H);
    tv.resize_cells(20, 5);
    tv.resize_cells(0, 0);
    tv.render_into(&mut tiny, 8, 8);
}
