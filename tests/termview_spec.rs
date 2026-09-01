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
    self, ANSI_16, BOOT_COLS, BOOT_ROWS, CELL_H, CELL_W, DEFAULT_BG, DEFAULT_FG, TermView,
    build_vendored, cell_origin, color_to_xrgb, grid_dims, indexed_color,
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
fn spec_颜色_蓝系可读_钉住品牌蓝() {
    // 2026-08-23 实拍:VGA 蓝 #0000AA/#5555FF 在纯黑底上不可读
    // (ssh 远端 ls 目录名看不清)——蓝系换 kfmv4 品牌蓝,钉死防回退
    assert_eq!(ANSI_16[4], 0x003B_82F6);
    assert_eq!(ANSI_16[12], 0x0060_A5FA);
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
fn spec_scrollback_容量钉死显式值() {
    // 审计漂移 #1 用户拍板(2026-08-27):na 显式钉 10000,不许继承上游
    // 默认。灌超帽输出,实测量必须正好压在帽上——上游默认若变了而
    // 有人又退回裸 Config::default(),本题必红
    let mut tv = host_termview(8, 2);
    let extra = 50;
    for i in 0..kfm_na::termview::TermView::SCROLLBACK_LINES + extra {
        tv.feed(format!("x{i}\r\n").as_bytes());
    }
    assert_eq!(
        tv.history_size(),
        kfm_na::termview::TermView::SCROLLBACK_LINES,
        "scrollback 必须正好钉在显式容量上"
    );
    // 顺带区分 nz 式 1000 帽:容量必须远大于千行级(编译期钉)
    const {
        assert!(kfm_na::termview::TermView::SCROLLBACK_LINES >= 5000);
    }
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
        kfm_na::theme::Theme::default().keybar.key_bg,
        "键格色必须上屏"
    );
    // 键格里必须有标签墨（非键底色非行底色的像素存在）
    let mut ink = false;
    for y in (h - keybar::HEIGHT_PX)..(h - keybar::HEIGHT_PX + keybar::ROW_H_PX) {
        for x in 0..100u32 {
            let p = buf[(y * w + x) as usize];
            if p != kfm_na::theme::Theme::default().keybar.key_bg
                && p != kfm_na::theme::Theme::default().keybar.bg
                && p != DEFAULT_BG
            {
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
        kfm_na::theme::Theme::default().keybar.key_bg,
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
        kfm_na::theme::Theme::default().keybar.mod_on,
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
/// 真机 U+25BD ▽ tofu 目击刷屏；BAR-027：agnoster/robbyrussell 要的
/// /✘/⚡/✓/✗/➜/➦ 不在 GB2312，FusionPixel 缺的 7 个从 DejaVuSansMono
/// 借形补位）。契约：内嵌 CJK/符号 fallback 字体必须覆盖补丁表代表字符——
/// 盲文（kimi code 转动点）、方块、几何、箭头、框线、powerline、omz 符号
#[test]
fn spec_bar022_内嵌cjk字体_终端符号补丁覆盖() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析");
    for c in [
        '⠋', '█', '▽', '→', '─', '\u{E0A0}', '\u{E0B0}', '✘', '⚡', '✓', '✗', '➜', '➦',
    ] {
        assert!(
            font.lookup_glyph_index(c) != 0,
            "内嵌 CJK/符号字体缺 {c}（U+{:04X}）——补丁表被裁掉了？",
            c as u32
        );
    }
}

/// powerline 单格钉（BAR-028：FusionPixel 的 E0A0-E0D4 是全角设计，终端按
/// unicode-width=1 渲染，右半被格宽裁剪切掉——agnoster 箭头变「方括号」，
/// 2026-08-23 真机截图目击）。契约：powerline 字形步进 == 半角字符步进，
/// 且墨迹不越格宽（纵向保持满行高不管）
#[test]
fn spec_bar028_powerline字形_单格步进() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析");
    let half = font.metrics('M', 100.0).advance_width;
    for c in ['\u{E0A0}', '\u{E0B0}', '\u{E0B2}'] {
        let m = font.metrics(c, 100.0);
        assert_eq!(
            m.advance_width, half,
            "{c}（U+{:04X}）步进应=半角步进——全角 powerline 会被裁成方括号",
            c as u32
        );
    }
}

#[test]
fn spec_bar032_powerline箭头_实心阶梯三角() {
    // BAR-032：FusionPixel 上游的 E0B0 是「色块+C 形镂空」装饰设计，渲染
    // 出来像方括号/C 字（freetype/fontdue 双光栅器复现，真机实拍目击）。
    // 烘焙已换成合成实心阶梯三角。像素级契约：
    //   中间行满宽有墨（杀镂空）；顶/底行只有左缘有墨（三角收腰，杀色块）；
    //   E0B2 镜像对称。变异抽检：回滚成上游字形，本考题必红。
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析");
    let ink = |c: char, row_ratio: f32, col_ratio: f32| -> bool {
        let (m, bmp) = font.rasterize(c, 100.0);
        // 注：bitmap 顶/底可能各有一行取整产生的空 padding，探针打在 2%/98%
        let y = ((m.height - 1) as f32 * row_ratio) as usize;
        let x = ((m.width - 1) as f32 * col_ratio) as usize;
        bmp[y * m.width + x] > 8
    };
    // E0B0 右箭头：尖朝右
    assert!(
        ink('\u{E0B0}', 0.5, 1.0),
        "E0B0 中间行右缘必须有墨（箭头贴右缘）"
    );
    assert!(
        ink('\u{E0B0}', 0.5, 0.7),
        "E0B0 中间行 70% 处必须有墨（实心，不许镂空）"
    );
    assert!(
        ink('\u{E0B0}', 0.02, 0.0),
        "E0B0 顶行左缘必须有墨（左缘满高贴齐）"
    );
    assert!(
        !ink('\u{E0B0}', 0.02, 0.5),
        "E0B0 顶行中部必须无墨（三角收腰，不许是色块）"
    );
    assert!(
        !ink('\u{E0B0}', 0.98, 1.0),
        "E0B0 底行右缘必须无墨（尖角收拢）"
    );
    // E0B2 左箭头：镜像
    assert!(
        ink('\u{E0B2}', 0.5, 0.0),
        "E0B2 中间行左缘必须有墨（尖朝左）"
    );
    assert!(
        ink('\u{E0B2}', 0.02, 1.0),
        "E0B2 顶行右缘必须有墨（右缘满高贴齐）"
    );
    assert!(
        !ink('\u{E0B2}', 0.02, 0.5),
        "E0B2 顶行中部必须无墨（三角收腰）"
    );
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

// ---------- A 档：单格 CJK 字形格宽裁剪（2026-08-21 实拍 ⇄ 溢出） ----------

#[test]
fn spec_bar026_渲染_单格cjk字形按格宽裁剪() {
    // ⇄ (U+21C4) 模糊宽度：unicode-width 判 1 格，但 FusionPixel 里是
    // 全角字形（px36 时步进 36px）——真机上主字体（商业像素字体）缺 ⇄
    // 落 CJK 备用，墨溢进下一格。契约：单格路径右缘按 1 格宽裁剪。
    // 夹具：内嵌 CJK/符号字体直接当主字体（双环境同一份文件，compile-time
    // include_bytes 恒定），⇄ 走单格路径（无双倍宽标志）。变异抽检：
    // draw_glyph 摘掉 clip_right，本考题必须红
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析");
    assert!(
        font.lookup_glyph_index('\u{21C4}') != 0,
        "夹具前提：⇄ 必须有真字形"
    );
    let mut tv = TermView::new(font, None, 10, 3, CELL_W, CELL_H);
    tv.feed("\u{21C4}\r\n".as_bytes()); // 光标滚到下行，第 0 行无光标反色干扰
    let buf_w = 2 * termview::MARGIN_X + 10 * CELL_W;
    let buf_h = termview::margin_top(CELL_H) + 3 * CELL_H + termview::MARGIN_Y;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    let cell_ink = |col: u32| -> usize {
        let x0 = termview::MARGIN_X + col * CELL_W;
        let y0 = termview::margin_top(CELL_H);
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
    assert!(cell_ink(0) > 0, "⇄ 必须画出来（不许裁没了）");
    assert_eq!(cell_ink(1), 0, "单格字形的墨不许越界到下一格内容区");
}

// ---------- 调试闸门：视野纯文本导出（2026-08-24，三件套之读懂） ----------

/// dump_text 契约：当前视野（display_offset 起 screen_lines 行）逐行收字符，
/// ANSI 转义不露面、CJK 宽字符的 spacer 半格不产垃圾、行尾 trim、行间 \n；
/// 滚动后导出跟视野走（眼睛对齐「所见」）
#[test]
fn spec_dump_text_视野纯文本导出() {
    let mut tv = host_termview(16, 3);
    tv.feed(b"hi\r\n\x1b[31m\xe7\xba\xa2\xe8\x89\xb2\x1b[0m plain");
    let text = tv.dump_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "hi", "第一行原样");
    assert_eq!(
        lines[1], "红色 plain",
        "ANSI 转色不露面;CJK spacer 半格不产垃圾"
    );
    assert!(lines[2].is_empty(), "没内容的行 = 空串(行尾 trim)");

    // 造历史再滚屏:导出的必须是视野而不是缓冲头
    let mut tv = host_termview(16, 3);
    tv.feed(b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
    let text = tv.dump_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["l3", "l4", "l5"], "贴底视野 = 最后三行");
    tv.scroll_lines(1); // 回滚一行进历史
    let text = tv.dump_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["l2", "l3", "l4"], "滚动后导出跟视野走");
    tv.scroll_to_bottom();
    let text = tv.dump_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["l3", "l4", "l5"], "回底后视野复原");
}

/// BAR-035:起手几何钉——build_vendored(真机同款)必须从 BOOT_COLS×BOOT_ROWS
/// 起手:喂超长行,折行点必须落在 BOOT_COLS;喂超行数,视野行数 = BOOT_ROWS。
/// 意义:na-replay 与真机共享这对常量,谁把起手几何改了,这里先红,
/// 「回放=读屏」判卷才不会静默漂走(2026-08-25 终验实拍的漂移路径)
#[test]
fn spec_bar035_内嵌终端_起手几何钉() {
    let (mut tv, _, _) = build_vendored().expect("内嵌字体必须在");
    // 列:BOOT_COLS+10 个 a,折行点必须恰好 BOOT_COLS
    tv.feed("a".repeat(BOOT_COLS as usize + 10).as_bytes());
    tv.feed(b"\r\n");
    let text = tv.dump_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0].chars().count(),
        BOOT_COLS as usize,
        "折行点=BOOT_COLS"
    );
    assert_eq!(lines[1].chars().count(), 10, "余量进第二行");

    // 行:BOOT_ROWS+5 行灌进去,贴底视野必须恰好 BOOT_ROWS 行
    let mut tv = build_vendored().expect("内嵌字体必须在").0;
    for i in 0..(BOOT_ROWS + 5) {
        tv.feed(format!("r{i}\r\n").as_bytes());
    }
    let text = tv.dump_text();
    assert_eq!(
        text.lines().count(),
        BOOT_ROWS as usize,
        "贴底视野行数=BOOT_ROWS"
    );
}

// ---- BAR-040(2026-08-27 用户实拍):开局横幅两行被顶出视野 ----
// 飞行记录仪铁证:横幅 ts=0ms 在 BOOT 80 列印,ts=2ms resize 61 列,
// 重排折行 +2,标题「── kfm-na 就绪 ──」与次行前半被顶进 scrollback,
// 用户要上一次滑才能看全。契约:横幅必须在首个真实几何 resize 之后再印。

#[test]
fn spec_bar040_开局横幅_先印后resize_顶行被顶走() {
    // 病灶钉(变异见证):错误的时序必须真的丢行——若此题转绿,
    // 说明 alacritty 重排行为变了,契约题须跟着重审
    let mut tv = TermView::new(
        host_font(),
        Some(host_font()),
        BOOT_COLS,
        BOOT_ROWS,
        CELL_W,
        CELL_H,
    );
    tv.feed(kfm_na::termview::HELP_BANNER.as_bytes());
    tv.resize_cells(61, 62); // 真机首发几何(flight-rec 实测)
    assert!(
        !tv.dump_text().contains("kfm-na 就绪"),
        "病灶复现:先印后 resize,标题必须被顶出视野"
    );
}

#[test]
fn spec_bar040_开局横幅_先resize后印_顶行完整() {
    // 契约钉(修复时序):先应用真实几何再印横幅,标题必须留在视野顶,
    // 且贴底不滚(display_offset=0)
    let mut tv = TermView::new(
        host_font(),
        Some(host_font()),
        BOOT_COLS,
        BOOT_ROWS,
        CELL_W,
        CELL_H,
    );
    tv.resize_cells(61, 62);
    tv.feed(kfm_na::termview::HELP_BANNER.as_bytes());
    let text = tv.dump_text();
    assert!(
        text.contains("kfm-na 就绪"),
        "先 resize 后印:标题必须在视野内\n{text}"
    );
    assert_eq!(tv.display_offset(), 0, "印完必须贴底,不许自带滚动");
}

// ---------- term-contract C4:宽字符占格(2026-08-27 立项,两线对照) ----------
// 判据(评审定,与 nz measureCell 同语义):同串直喂网格 → 光标推进列数。
// 串表 = term-contract.md §C4 行。教训(评审实拍):经 PTY/shell 注入
// 测宽度会混入 zsh ZLE 转义回显(E0B0 实测被推 4 列)——必须直喂网格
// 断 cursor,不许过 shell。辅助尺 dump_text(spacer 已跳)仍用于原子性。

#[test]
fn spec_c4_光标推进列数_契约串表() {
    let cases: &[(&str, usize)] = &[
        ("A中A", 4),     // 1+2+1
        ("中中", 4),     // 2+2
        ("\u{E0B0}", 1), // powerline 单宽(BAR-028 家族边界)
        ("中文A", 5),    // 2+2+1
    ];
    for (s, want) in cases {
        // 每串独立建视图直喂:col0 起,断推进列数——判据就是 cursor 本身
        let mut tv = host_termview(40, 4);
        tv.feed(s.as_bytes());
        assert_eq!(tv.cursor_col(), *want, "C4 违约:{s:?} 应推 {want} 列");
    }
}

#[test]
fn spec_c4_宽字符劈格防御_行尾半格不拆字() {
    // 一行 8 格,行尾剩 1 格时灌 2 格宽汉字:alacritty 语义 = 换行重排
    // (字整体挪下行),不许把半个字留在上行(spacer 孤儿 = 渲染 tofu 空
    // 半格 + dump_text 错位)。此为 C4 的隐含义务:2 格是原子单位
    let mut tv = host_termview(8, 3);
    tv.feed(b"1234567"); // 行尾剩 1 格
    tv.feed("中".as_bytes()); // 要 2 格 → 必须整体到下一行
    let text = tv.dump_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "1234567", "第一行塞不下整字不许劈");
    assert_eq!(lines[1], "中", "汉字原子换行到第二行");
}

// ========== 渐变插值（2026-08-31 输入栏样式修订，A 档纯逻辑） ==========
// 判卷点：端点原色 / 中点均值 / 单调不回头。fill_round_rect_grad 本体
// 走 C 档实拍（na-shot 对照 kfmv4 参考图），像素轨不在这里判

#[test]
fn spec_lerp_rgb_endpoints_and_midpoint() {
    use kfm_na::termview::lerp_rgb;
    let (c1, c2) = (0x006E_49EB, 0x0018_A8D8); // 输入栏描边两端(左紫/右青)
    assert_eq!(lerp_rgb(c1, c2, 0), c1, "t=0 必须原样出 c1");
    assert_eq!(lerp_rgb(c1, c2, 255), c2, "t=255 必须原样出 c2");
    let mid = lerp_rgb(0x0000_0000, 0x00FF_FFFF, 128);
    let r = (mid >> 16) & 0xFF;
    assert!((127..=128).contains(&r), "黑白中点应≈128,实得 {r}");
}

#[test]
fn spec_lerp_rgb_monotonic_no_wraparound() {
    use kfm_na::termview::lerp_rgb;
    // c1 > c2 的下行通道也不许回绕(u32 下溢会炸出亮斑)
    let mut prev = lerp_rgb(0x00FF_0000, 0x0000_1000, 0);
    for t in 1..=255u32 {
        let cur = lerp_rgb(0x00FF_0000, 0x0000_1000, t);
        assert!(
            (cur >> 16) <= (prev >> 16),
            "红通道必须单调不升:t={t} {prev:#x}→{cur:#x}"
        );
        prev = cur;
    }
}

// ========== 圆角覆盖率（2026-08-31 质感 v2，SDF 抗锯齿的尺） ==========
// 判卷点：内心全覆盖 / 远角零覆盖 / 角区过渡带存在且单调。图元本体
// （描边/发光/高光）走 C 档实拍，尺错了实拍全是错——尺必须先钉

#[test]
fn spec_rr_cover_interior_full_corner_zero() {
    use kfm_na::termview::rr_cover;
    let (w, h, r) = (100, 60, 16);
    assert_eq!(rr_cover(50, 30, w, h, r), 255, "内心必须全覆盖");
    assert_eq!(
        rr_cover(50, 0, w, h, r),
        255,
        "直边中点(中心距边 0.5)全覆盖"
    );
    assert_eq!(rr_cover(0, 0, w, h, r), 0, "远角外必须零覆盖");
    assert_eq!(rr_cover(w - 1, h - 1, w, h, r), 0, "对角同样零覆盖");
}

#[test]
fn spec_rr_cover_corner_transition_band() {
    use kfm_na::termview::rr_cover;
    let (w, h, r) = (100, 60, 16);
    // 角区 16×16 内必须存在 0<cov<255 的过渡像素（没过渡 = 硬边锯齿回潮）
    let mut soft = 0u32;
    for py in 0..r {
        for px in 0..r {
            let c = rr_cover(px, py, w, h, r);
            if c > 0 && c < 255 {
                soft += 1;
            }
        }
    }
    assert!(soft >= 8, "角区过渡带太薄: 仅 {soft} 个半覆盖像素");
    // 沿对角线向角心走,覆盖率单调不回头
    let mut prev = 0u32;
    for i in [2u32, 6, 10, 14] {
        let c = rr_cover(i, i, w, h, r);
        assert!(c >= prev, "角向心覆盖必须单调: {prev}→{c}");
        prev = c;
    }
}

// ========== 换行布局（2026-08-31 移动端 textarea 全量复刻拍板） ==========
// 判卷点:放得下 = 一行;贪心断行(满即断);精确边界;空表不炸。
// kfmv4 .ai-input 是 textarea 自动换行——渲染本体 C 档实拍,断行窗纯逻辑先钉。
// (同日取代尾锚方案 spec_tail_fit_start_*:单行截尾被 textarea 折行淘汰,
// 函数与两题干净移除,git 历史留痕)

#[test]
fn spec_wrap_starts_single_line_when_fits() {
    use kfm_na::termview::wrap_starts;
    assert_eq!(wrap_starts(&[10.0, 10.0, 10.0], 100.0), vec![0]);
    assert_eq!(
        wrap_starts(&[10.0, 10.0, 10.0], 30.0),
        vec![0],
        "刚好放下不断行"
    );
    assert_eq!(wrap_starts(&[], 100.0), vec![0], "空表不炸");
}

#[test]
fn spec_wrap_starts_greedy_breaks() {
    use kfm_na::termview::wrap_starts;
    // 5×10,max 25:行1=10+10,第 3 个满即断 → [0,2,4]
    assert_eq!(wrap_starts(&[10.0; 5], 25.0), vec![0, 2, 4]);
    // 超宽单字(20>15):该行只放它一个也要放(不吞字),随后继续贪心——
    // 行2={10},第 3 个加上就 20>15 满即断 → [0,1,2]
    // (原稿误写 [0,1],与「满即断」自相矛盾,2026-08-31 答案生成前勘误)
    assert_eq!(wrap_starts(&[20.0, 10.0, 10.0], 15.0), vec![0, 1, 2]);
}

// ========== 量行端（textarea 眼手同尺单源：渲染层量宽 → set_lines 写回） ==========
// 判卷点:空文/短文 = 一行;同一长文窗越窄行越多(内嵌真字体真量宽,不是 mocks)

#[test]
fn spec_bar_text_lines_空短文一行() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    assert_eq!(tv.bar_text_lines("", 1080), 1, "空文一行");
    assert_eq!(tv.bar_text_lines("你好", 1080), 1, "短文一行");
    assert_eq!(tv.bar_text_lines("随便什么", 10), 1, "窗退化不炸按一行计");
}

#[test]
fn spec_bar_text_lines_窗越窄行越多() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let long = "这是一段足够长的输入文本专门用来触发折行行为abcde12345更多字";
    let wide = tv.bar_text_lines(long, 1080);
    let narrow = tv.bar_text_lines(long, 400);
    assert!(wide >= 1 && narrow > wide, "窄 {narrow} 必须多于宽 {wide}");
}

// ========== BAR-039：渲染带高从文本实测（stale lines 两张皮回归钉） ==========

#[test]
fn spec_bar039_render_inputbar_带高从文本实测() {
    // BAR-039：snap.lines 是经 poll 转写的读数（后台挂起无写回 = stale），
    // 渲染带高必须从文本实测量出——stale lines=1 + 超三行文本注入，
    // 带顶必须落在三行带高（不许被 stale 压扁成单行带）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
    let snap = kfm_na::input_bar::BarSnap {
        text: long.to_string(),
        focused: false,
        lines: 1, // stale 转写读数（后台 dump 实景）
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
    };
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    // 期望带顶 = 实测行数派生（BAR-039 不变量：渲染带高 == 实测折行带高，
    // 与 stale lines 无关；MAX_LINES 调 5 后此例 6 行实测量、带高封顶 5 行）
    let measured = tv.bar_text_lines(long, w);
    assert!(measured >= 3, "这段文本必须至少折 3 行（30 字×窄窗）");
    let band_top = h - kfm_na::input_bar::height_for_lines(measured);
    let mid = (w / 2) as usize;
    let inside = (band_top + 1) as usize * w as usize + mid;
    let above = (band_top - 1) as usize * w as usize + mid;
    assert_ne!(
        buf[inside], 0,
        "带顶发丝线必须在（stale lines 不许压扁带高——两张皮实景）"
    );
    assert_eq!(buf[above], 0, "带顶之上是终端区，不许有栏带墨");
}

// ========== 光标 + 定位柄（2026-08-31 用户指认浏览器控件行为） ==========
// 判卷点:聚焦+相位亮画光标,相位灭不画,失焦不画;定位柄只跟 handle 走;
// 点按定位换算与渲染同几何(行向钳尾锚块,列向过半归右)

#[test]
fn spec_bar_caret_闪烁相位与定位柄() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let focused = kfm_na::input_bar::BarSnap {
        text: String::new(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: true,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
    };
    let mut on = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut on, w, h, 0, &focused, false, true);
    let mut off = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut off, w, h, 0, &focused, false, false);
    let caret_px = 1089 * w as usize + 120; // 单行带几何:行垂直中心×光标线
    assert_ne!(on[caret_px], off[caret_px], "闪烁相位翻转必须改光标像素");
    // 定位柄只跟 handle 走:关掉 handle 再倒一帧,柄区像素必须不同
    let no_handle = kfm_na::input_bar::BarSnap {
        handle: false,
        ..focused.clone()
    };
    let mut noh = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut noh, w, h, 0, &no_handle, false, true);
    let handle_px = 1138 * w as usize + 120; // 光标行底的柄身(BAR-042 行锚)
    assert_ne!(on[handle_px], noh[handle_px], "定位柄必须悬在光标行底");
    // BAR-042:柄稳显不随光标闪烁(off 相位下柄仍在)
    assert_eq!(off[handle_px], on[handle_px], "柄不随光标闪烁");
    // 失焦不画光标(与相位灭同画素)
    let unfocused = kfm_na::input_bar::BarSnap {
        focused: false,
        ..focused.clone()
    };
    let mut unf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut unf, w, h, 0, &unfocused, false, true);
    assert_eq!(unf[caret_px], off[caret_px], "失焦无光标");
}

#[test]
fn spec_bar_cursor_at_点按定位换算() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    assert_eq!(tv.bar_cursor_at("", 600, 100.0, 30.0), 0, "空文定位 = 0");
    let short = "你好";
    assert_eq!(tv.bar_cursor_at(short, 600, 0.0, 30.0), 0, "行首落 0");
    assert_eq!(
        tv.bar_cursor_at(short, 600, 10_000.0, 30.0),
        2,
        "行尾越界 = 末尾(cursor=字数,插入点在最后)"
    );
    // 长文折行:w=400 一行一字,尾锚显末 5 行——行向越往下全局下标越大
    let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
    let top = tv.bar_cursor_at(long, 400, 100.0, 0.0);
    let mid = tv.bar_cursor_at(long, 400, 100.0, 200.0);
    let bottom = tv.bar_cursor_at(long, 400, 100.0, 10_000.0);
    assert!(
        top < mid && mid < bottom,
        "行向下标单调: {top}<{mid}<{bottom}"
    );
    assert_eq!(bottom, long.chars().count(), "末行中列以远 = 插入点在最后");
}

// ========== IME 组合态渲染（2026-09-01 编辑对齐第 1 批） ==========
// 判卷点:组合段字底品牌青下划线(fill_rect 直写=字面值),稳显不随光标闪烁

#[test]
fn spec_composing_下划线稳显() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "你好".to_string(),
        focused: true,
        lines: 1,
        cursor: 2,
        handle: false,
        composing: "ni".to_string(),
        scroll_px: 0,
        follow: true,
    };
    let mut on = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut on, w, h, 0, &snap, false, true);
    let mut off = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut off, w, h, 0, &snap, false, false);
    // 单行带几何:行中心 y≈1089,下划线在 row_cy+22 → y≈1111..1115
    let has_accent =
        |buf: &[u32]| (0..w as usize).any(|x| buf[1113 * w as usize + x] == 0x0000_D4FF);
    assert!(has_accent(&on), "组合段字底必须有品牌青下划线(字面值)");
    assert!(has_accent(&off), "下划线稳显,不随光标闪烁相位消失");
    // 光标本身随相位翻转(行带内找相位差异;组合尾 x 由字体度量定,不硬编码)
    let mut caret_diff = false;
    'outer: for y in 1060..1120usize {
        for x in 0..w as usize {
            if on[y * w as usize + x] != off[y * w as usize + x] {
                caret_diff = true;
                break 'outer;
            }
        }
    }
    assert!(caret_diff, "光标仍随相位闪烁(行带内有相位差异)");
}

// ========== 视口滚动渲染（2026-09-01 像素级） ==========
// 判卷点:follow=尾锚(条带底贴 field 底);scroll_px=0+follow=false 显头部;
// 光标出视口不画(相位差异像素数 0);follow ≡ scroll_px=尾锚值(缓冲逐
// 像素相等——像素级滚动的精确性钉)

#[test]
fn spec_视口滚动_follow与像素偏移() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1400u32);
    let long = "一二三四五六七八九十".repeat(5);
    let base = |follow: bool, scroll_px: i32| kfm_na::input_bar::BarSnap {
        text: long.clone(),
        focused: true,
        lines: 10,
        cursor: 50,
        handle: false,
        composing: String::new(),
        scroll_px,
        follow,
    };
    let mut tail = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut tail, w, h, 0, &base(true, 0), false, true);
    let mut eqv = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut eqv, w, h, 0, &base(false, 222), false, true);
    assert_eq!(tail, eqv, "follow 尾锚 ≡ scroll_px=最大偏移(逐像素)");
    let mut head = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut head, w, h, 0, &base(false, -9999), false, true);
    assert_ne!(
        &tail[1100 * 600..1100 * 600 + 600],
        &head[1100 * 600..1100 * 600 + 600],
        "field 内有字行:头部窗与尾锚窗内容不同"
    );
    // 光标可见性:同 snap 双相位对拍,唯一变量是光标矩形——
    // 尾锚态光标(文末行)在窗内必画(差异≈208);头部窗该行滚出→零差异
    let caret_diff = |snap: &kfm_na::input_bar::BarSnap| {
        let mut on = vec![0u32; (w * h) as usize];
        tv.render_inputbar(&mut on, w, h, 0, snap, false, true);
        let mut off = vec![0u32; (w * h) as usize];
        tv.render_inputbar(&mut off, w, h, 0, snap, false, false);
        on.iter().zip(off.iter()).filter(|(a, b)| a != b).count()
    };
    assert!(caret_diff(&base(true, 0)) >= 150, "尾锚态光标必可见");
    assert_eq!(
        caret_diff(&base(false, -9999)),
        0,
        "头部窗光标行已滚出视口,不画"
    );
}
