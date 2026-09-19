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
    self, ANSI_16, BOOT_COLS, BOOT_ROWS, CELL_H, CELL_W, DEFAULT_BG, DEFAULT_FG, TermEmu, TermView,
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

/// 顶带高（termview::margin_top 是 pub const fn——GPU 收集口考题用）
fn margin_top_of() -> u32 {
    kfm_na::termview::margin_top(kfm_na::termview::CELL_H)
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

/// 字墨判别（2026-09-11 终端卡片壳改造）：默认底色的格不再重刷黑，
/// 透出壳内芯 TERM_CARD_BG——「不是黑」不等于「有墨」，判墨必须同时
/// 排除纯黑壳外带与壳内芯色（变异抽检语义：缺字形/不渲染必须仍能抓红）
fn is_ink(p: u32) -> bool {
    p != DEFAULT_BG && p != termview::TERM_CARD_BG
}

/// 帧缓冲里存在字墨像素
fn has_non_bg(buf: &[u32]) -> bool {
    buf.iter().any(|&p| is_ink(p))
}

#[test]
fn spec_渲染_feed文字后帧缓冲有字形像素() {
    let mut tv = host_termview(24, 6);
    tv.feed(b"hello");
    let mut buf = vec![DEFAULT_FG; (24 * CELL_W * 6 * CELL_H) as usize]; // 污染初值防假绿
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H, 0);
    assert!(has_non_bg(&buf), "feed hello 后必须画出非背景像素");
    // 且必须有背景像素（黑底真刷了）——防「全帧涂满」式假实现
    assert!(buf.contains(&DEFAULT_BG), "黑底必须存在");
}

#[test]
fn spec_渲染_ansi红色出红像素() {
    let mut tv = host_termview(24, 6);
    tv.feed(b"\x1b[31mR");
    let mut buf = vec![0u32; (24 * CELL_W * 6 * CELL_H) as usize];
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H, 0);
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
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H, 0);
    // 光标格（5列, 0行）反色后背景为白——该格矩形内必须有接近白的像素；
    // 相邻的空格（6列）不是光标，整格透出壳内芯 TERM_CARD_BG
    // 渲染的格原点 = cell_origin + 边距（BAR-005）+ 壳环靠泊（2026-09-11 卡片壳）
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
            if buf[(y * buf_w + x) as usize] != termview::TERM_CARD_BG {
                neighbor_dark = false;
            }
        }
    }
    assert!(cursor_white, "光标格必须反色（白底）");
    // 卡片壳契约：非光标的空格不反色——整格透出壳内芯 TERM_CARD_BG
    assert!(neighbor_dark, "非光标的空格必须透出壳内芯（不反色）");
}

#[test]
fn spec_渲染_cjk缺字形不panic() {
    let mut tv = host_termview(24, 6);
    // DejaVuSansMono 无 CJK 字形——tofu 方框或空位图，绝不许 panic
    tv.feed("中文混排 English 123".as_bytes());
    let mut buf = vec![0u32; (24 * CELL_W * 6 * CELL_H) as usize];
    tv.render_into(&mut buf, 24 * CELL_W, 6 * CELL_H, 0);
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
    tv.render_into(&mut buf, 10 * CELL_W, 3 * CELL_H, 0);
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
    tv.render_into(&mut buf, 10 * CELL_W, 2 * CELL_H, 0);
    assert!(has_non_bg(&buf));
    // 0 维钳 1 不 panic
    tv.resize_cells(0, 0);
    tv.render_into(&mut buf, 10 * CELL_W, 2 * CELL_H, 0);
}

// ---------- A 档：字体加载 ----------

/// 帧缓冲里某格的字墨纵向跨度 → (最上, 最下) 字墨像素行（相对格原点）。
/// 无墨水的格返回 (CELL_H, 0)（上下颠倒即为空）。
/// 注意含边距偏移——渲染的格原点 = cell_origin + (MARGIN_X, MARGIN_TOP)；
/// 壳内芯色不算墨（is_ink，卡片壳后空格透壳底）
fn cell_ink_span(buf: &[u32], buf_w: u32, col: u32, row: u32) -> (u32, u32) {
    let (ox, oy) = cell_origin(col, row, CELL_W, CELL_H);
    let (ox, oy) = (ox + termview::MARGIN_X, oy + termview::MARGIN_TOP);
    let (mut top, mut bot) = (CELL_H, 0);
    for y in 0..CELL_H {
        for x in 0..CELL_W {
            if is_ink(buf[((oy + y) * buf_w + ox + x) as usize]) {
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
    tv.render_into(&mut buf, buf_w, buf_h, 0);
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

/// 内嵌 CJK fallback 的月亮相位补丁（2026-09-19：kimi code 转动点
/// 🌑-🌘 U+1F311-1F318 tofu 目击——主字体（商业像素）天然缺，渲染全靠
/// prefer_cjk 路由到这份备用；补丁 = font-bake.py MOON_CPS 借字形
/// 全角位，捐体 DejaVuSans）。变异抽检方向：烘焙漏登记 format 12 /
/// 借成半角位/漏某一相——本钉必须红
#[test]
fn spec_字体_内嵌cjk月亮相位补丁() {
    let font = fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体字节必须可解析");
    for cp in 0x1F311u32..=0x1F318 {
        let c = char::from_u32(cp).unwrap();
        assert!(
            font.lookup_glyph_index(c) != 0,
            "U+{cp:04X} 月亮相位在内嵌 fallback 缺字形"
        );
    }
    // 汉字不能被 format 12 新表顶灭（getBestCmap 优先选 format 12，
    // 只装月亮 = 汉字全灭——烘焙判卷实踩过的坑）
    assert!(font.lookup_glyph_index('中') != 0, "汉字覆盖被月亮补丁破坏");
    // 光栅必须有墨（空轮廓 = 不可见字形，比 tofu 更难察觉）
    let (m, bmp) = font.rasterize('\u{1F316}', 32.0);
    assert!(m.width > 0 && m.height > 0, "月亮光栅尺寸为零");
    assert!(bmp.iter().any(|&v| v > 0), "月亮光栅全空（无墨）");
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
    // 2026-09-11 终端卡片壳改造：边距语义由壳继承——几何单源
    // 纵尺 = 壳外缘(16) + 环粗(3) + 净垫(12) = 31；横尺同日晚用户实拍
    // 拍板再让一整格字宽（「直接贴上了显得太紧」）= 16+3+30 = 49。
    // BAR-010「顶带下探一整行」的防圆角切字意图由壳环靠泊距离接管
    // （顶带不再随格高走，反转旧契约，见 spec_缩放_顶带恒定不随格高）。
    // 契约：①几何单源关系钉死；②壳环与首格之间的净垫带必须纯壳内芯色
    // （字墨不许贴环——变异抽检：MARGIN_X 改小/TERM_CARD_PAD_X 改小必须红）；
    // ③壳外缘列纯黑；④网格区必须有真字墨（防「全帧涂黑」式假绿）
    assert_eq!(
        termview::MARGIN_X,
        termview::AI_PAGE_FRAME_MARGIN + termview::AI_PAGE_FRAME_W + termview::TERM_CARD_PAD_X,
        "横边距必须是壳几何单源：外缘+环粗+横向净垫"
    );
    assert_eq!(
        termview::MARGIN_Y,
        termview::AI_PAGE_FRAME_MARGIN + termview::AI_PAGE_FRAME_W + termview::TERM_CARD_PAD,
        "纵边距必须是壳几何单源：外缘+环粗+净垫"
    );
    assert_eq!(
        termview::TERM_CARD_PAD_X,
        termview::TERM_CARD_PAD + CELL_W,
        "横向净垫 = 纵净垫 + 一整格字宽（2026-09-11 晚用户拍板）"
    );
    // 显式值钉（防关系式跟着常量一起漂——49/31 是拍板值）
    assert_eq!(termview::MARGIN_X, 49, "横边距拍板值 49 = 16+3+(12+18)");
    assert_eq!(termview::MARGIN_Y, 31, "纵边距拍板值 31 = 16+3+12");
    assert_eq!(termview::MARGIN_TOP, termview::MARGIN_Y);
    let mut tv = host_termview(8, 2);
    tv.feed(b"A");
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 2 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    // ②净垫带：环内缘到网格原点之间，只查四边中段的直行带（角带弧区
    // 是环墨几何主场，不在此处判）。环的左缘 3 倍粗（装修配方），
    // 故左垫带从 MARGIN+3W 起，其余三边从 MARGIN+W 起
    let ring_l = termview::AI_PAGE_FRAME_MARGIN + 3 * termview::AI_PAGE_FRAME_W;
    let ring_r = termview::AI_PAGE_FRAME_MARGIN + termview::AI_PAGE_FRAME_W;
    let mid_y = buf_h / 2;
    let mid_x = buf_w / 2;
    for y in mid_y - 10..mid_y + 10 {
        for x in ring_l..termview::MARGIN_X {
            assert_eq!(
                buf[(y * buf_w + x) as usize],
                termview::TERM_CARD_BG,
                "左净垫带 ({x},{y}) 必须纯壳内芯"
            );
        }
        for x in buf_w - termview::MARGIN_X..buf_w - ring_r {
            assert_eq!(
                buf[(y * buf_w + x) as usize],
                termview::TERM_CARD_BG,
                "右净垫带 ({x},{y}) 必须纯壳内芯"
            );
        }
    }
    for y in ring_r..termview::MARGIN_TOP {
        for x in mid_x - 10..mid_x + 10 {
            assert_eq!(
                buf[(y * buf_w + x) as usize],
                termview::TERM_CARD_BG,
                "顶净垫带 ({x},{y}) 必须纯壳内芯"
            );
        }
    }
    // ③壳外缘列纯黑（发光 spread 从外缘 1px 起，0 列必须无墨）
    for y in 0..buf_h {
        assert_eq!(
            buf[(y * buf_w) as usize],
            DEFAULT_BG,
            "壳外左缘列 (0,{y}) 必须纯黑"
        );
        assert_eq!(
            buf[(y * buf_w + buf_w - 1) as usize],
            DEFAULT_BG,
            "壳外右缘列 ({},{y}) 必须纯黑",
            buf_w - 1
        );
    }
    // ④字墨必须真的出现在边距之后的网格区（防「全帧涂壳色」式假绿）
    let mut ink = false;
    for y in termview::MARGIN_TOP..buf_h {
        for x in termview::MARGIN_X..buf_w {
            if is_ink(buf[(y * buf_w + x) as usize]) {
                ink = true;
            }
        }
    }
    assert!(ink, "边距之后必须有字形墨水");
}

// ---------- A 档：终端卡片壳（2026-09-11，终端包壳与三面板同尺同配方） ----------

#[test]
fn spec_终端卡片壳_涂装冒烟() {
    // 壳 = 近黑微蓝灰内芯 + 碳灰渐变环（无色相，与三面板彩色相区分——
    // 终端是基座不是卡）。契约：①内芯 TERM_CARD_BG；②环带直行段有墨且
    // 低饱和（变异抽检：环换彩色相/纯黑环必须红）；③壳外带纯黑；
    // ④小缓冲/巨 inset 不 panic（paint_page_frame_ring i64 根修前
    // 这里实踩 u32 下溢）
    let (w, h) = (200u32, 300u32);
    let mut buf = vec![0u32; (w * h) as usize];
    termview::paint_term_card_chrome(&mut buf, w, h, 0);
    // ①内芯
    assert_eq!(
        buf[(150 * w + 100) as usize],
        termview::TERM_CARD_BG,
        "壳内芯必须是 TERM_CARD_BG"
    );
    // ②环带：左缘中段（左缘 3 倍粗配方，x=20 必在 16..25 环带上）
    let ring_p = buf[(150 * w + 20) as usize];
    assert!(
        ring_p != termview::TERM_CARD_BG && ring_p != DEFAULT_BG,
        "环带必须有墨（x=20,y=150 实得 {ring_p:#010x}）"
    );
    let (r, g, b) = ((ring_p >> 16) & 0xFF, (ring_p >> 8) & 0xFF, ring_p & 0xFF);
    assert!(
        r.abs_diff(g) <= 20 && g.abs_diff(b) <= 20,
        "环必须碳灰低饱和无色相（实得 r={r} g={g} b={b}）"
    );
    assert!(r >= 0x40, "环必须显著亮于内芯（读得出边界）");
    // ③壳外带纯黑（发光 spread 够不到 0 列与 0 行）
    assert_eq!(buf[0], DEFAULT_BG, "壳外角必须纯黑");
    assert_eq!(buf[(150 * w) as usize], DEFAULT_BG, "壳外左缘列必须纯黑");
    // ④小缓冲 + 巨 inset 不 panic
    let mut tiny = vec![0u32; 64];
    termview::paint_term_card_chrome(&mut tiny, 8, 8, 0);
    termview::paint_term_card_chrome(&mut tiny, 8, 8, u32::MAX);
    // ⑤环色相钉：两端色本身都必须碳灰低饱和（无色相）——与三面板
    // 彩色相的区分线钉在常量上（变异抽检：任一端换彩色相必须红）
    for (name, c) in [
        ("C1", termview::TERM_FRAME_C1),
        ("C2", termview::TERM_FRAME_C2),
    ] {
        let (r, g, b) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
        assert!(
            r.abs_diff(g) <= 16 && g.abs_diff(b) <= 16,
            "{name} 必须碳灰低饱和（实得 r={r} g={g} b={b}）"
        );
    }
}

#[test]
fn spec_终端卡片壳_默认底格透出壳内芯() {
    // 眼手同源的渲染层证据：feed 文字后无字格 = TERM_CARD_BG（不是黑），
    // 壳外 = 纯黑——终端坐在卡里，卡外是虚空（变异抽检：默认底格改回
    // 重刷 DEFAULT_BG，或壳内芯换色，本考题必须红）
    let mut tv = host_termview(8, 2);
    tv.feed(b"A");
    let buf_w = 2 * termview::MARGIN_X + 8 * CELL_W;
    let buf_h = termview::MARGIN_TOP + termview::MARGIN_Y + 2 * CELL_H;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    // 取样格避开右上角的设置钮（2026-09-12 齿轮放大到 72px 后，
    // 小缓冲里 (4,1) 格落入字形盒——控件浮文字上是设计语义，换 (2,1)）
    let (cx, cy) = cell_origin(2, 1, CELL_W, CELL_H);
    let (cx, cy) = (
        cx + termview::MARGIN_X + CELL_W / 2,
        cy + termview::MARGIN_TOP + CELL_H / 2,
    );
    assert_eq!(
        buf[(cy * buf_w + cx) as usize],
        termview::TERM_CARD_BG,
        "无字格必须透出壳内芯"
    );
    assert_eq!(buf[0], DEFAULT_BG, "壳外必须纯黑");
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
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    assert!(buf.iter().any(|&p| is_ink(p)), "CJK 必须有墨");
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
    tv.render_into(&mut buf, buf_w, buf_h, 0);
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
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    assert!(!tv.take_tofu_chars().contains(&'\t'), "tab 不许进目击名单");
    let cell_ink = |buf: &[u32], col: u32| -> usize {
        let (x0, y0) = cell_origin(col, 0, CELL_W, CELL_H);
        let (x0, y0) = (x0 + termview::MARGIN_X, y0 + termview::MARGIN_TOP);
        let mut n = 0;
        for y in y0..y0 + CELL_H {
            for x in x0..x0 + CELL_W {
                if is_ink(buf[(y * buf_w + x) as usize]) {
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
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    let row_ink = |row: u32| -> usize {
        let y0 = termview::MARGIN_TOP + row * CELL_H;
        let mut n = 0;
        for y in y0..y0 + CELL_H {
            for x in termview::MARGIN_X..buf_w - termview::MARGIN_X {
                if is_ink(buf[(y * buf_w + x) as usize]) {
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
fn spec_缩放_顶带恒定不随格高() {
    // 2026-09-11 终端卡片壳改造：顶带并入壳几何单源，恒定纵尺 MARGIN_Y(31)——
    // BAR-010「顶带随格高走（MARGIN_Y+CELL_H）」的旧契约就此反转，
    // 防圆角切字的意图由壳环靠泊距离接管（格再怎么大，壳环位置不动）。
    // 变异抽检：margin_top 改回随格高走（MARGIN_Y + cell_h）本考题必须红
    assert_eq!(termview::margin_top(CELL_H), termview::MARGIN_TOP);
    assert_eq!(termview::margin_top(20), termview::MARGIN_Y);
    assert_eq!(termview::margin_top(90), termview::MARGIN_Y);
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
    assert_eq!(
        pinch_cell_size(CELL_W, CELL_H, 100.0),
        (CELL_W_MAX, CELL_H_MAX),
        "百倍暴捏也钳在 6 倍上限（2026-09-11 放宽 Termux 级夸张档）"
    );
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

/// 数一格内的字墨像素（set_cell_size 重算字号的判卷尺：
/// 格放大 → 字号重算 → 同字符墨变多）；壳内芯透出色不算墨
fn cell_ink_count(tv: &mut TermView, buf_w: u32, buf_h: u32, col: u32, row: u32) -> usize {
    let (cw, ch) = tv.cell_size();
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    let (x0, y0) = cell_origin(col, row, cw, ch);
    let (x0, y0) = (x0 + termview::MARGIN_X, y0 + termview::margin_top(ch));
    let mut n = 0;
    for y in y0..y0 + ch {
        for x in x0..x0 + cw {
            if is_ink(buf[(y * buf_w + x) as usize]) {
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
    tv.render_into(&mut tiny, 8, 8, 0);
    // 设回不 panic + resize 跟随不 panic（android_app 链路：set_cell_size
    // 后必跟 apply_window_size → resize_cells）
    tv.set_cell_size(CELL_W, CELL_H);
    tv.resize_cells(20, 5);
    tv.resize_cells(0, 0);
    tv.render_into(&mut tiny, 8, 8, 0);
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
    tv.render_into(&mut buf, buf_w, buf_h, 0);
    let cell_ink = |col: u32| -> usize {
        let x0 = termview::MARGIN_X + col * CELL_W;
        let y0 = termview::margin_top(CELL_H);
        let mut n = 0;
        for y in y0..y0 + CELL_H {
            for x in x0..x0 + CELL_W {
                if is_ink(buf[(y * buf_w + x) as usize]) {
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

#[test]
fn spec_bar_text_lines_硬换行计入行数() {
    // 2026-09-04 Enter 换行：'\n' 硬断行进量行（真字体真量宽，不是 mocks）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    assert_eq!(tv.bar_text_lines("a\nb", 1080), 2, "硬换行 = 两行");
    assert_eq!(tv.bar_text_lines("a\n\nb", 1080), 3, "空逻辑行也算行");
    assert_eq!(tv.bar_text_lines("a\n", 1080), 2, "行尾换行产空行");
    assert_eq!(tv.bar_text_lines("ab", 1080), 1, "无换行仍一行（不退化）");
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
        selecting: false,
        selection_start: 0,
        selection_end: 0,
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
        selecting: false,
        selection_start: 0,
        selection_end: 0,
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
    let snap_of = |t: &str, follow: bool, scroll_px: i32| kfm_na::input_bar::BarSnap {
        text: t.to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px,
        follow,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    assert_eq!(
        tv.bar_cursor_at(&snap_of("", true, 0), 600, 100.0, 30.0),
        0,
        "空文定位 = 0"
    );
    let short = snap_of("你好", true, 0);
    assert_eq!(tv.bar_cursor_at(&short, 600, 0.0, 30.0), 0, "行首落 0");
    assert_eq!(
        tv.bar_cursor_at(&short, 600, 10_000.0, 30.0),
        2,
        "行尾越界 = 末尾(cursor=字数,插入点在最后)"
    );
    // 长文折行:w=400 一行一字,尾锚显末 5 行——行向越往下全局下标越大
    let long_text =
        "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
    let long = snap_of(long_text, true, 0);
    let top = tv.bar_cursor_at(&long, 400, 100.0, 0.0);
    let mid = tv.bar_cursor_at(&long, 400, 100.0, 200.0);
    let bottom = tv.bar_cursor_at(&long, 400, 100.0, 10_000.0);
    assert!(
        top < mid && mid < bottom,
        "行向下标单调: {top}<{mid}<{bottom}"
    );
    assert_eq!(
        bottom,
        long_text.chars().count(),
        "末行中列以远 = 插入点在最后"
    );
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
        selecting: false,
        selection_start: 0,
        selection_end: 0,
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

// BAR-046: 文本选择系统渲染判卷——选区高亮像素+锚点稳显+菜单像素
#[test]
fn spec_selection_高亮与锚点稳显() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 2,
        selection_end: 6,
    };
    let mut on = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut on, w, h, 0, &snap, false, true);
    let mut off = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut off, w, h, 0, &snap, false, false);
    // 单行带几何:行中心 y≈1089;选区底=品牌蓝 0x004488DD
    let has_select_bg =
        |buf: &[u32]| (0..w as usize).any(|x| buf[1089 * w as usize + x] == 0x0044_88DD);
    assert!(has_select_bg(&on), "选区必须有品牌蓝高亮底");
    assert!(has_select_bg(&off), "高亮稳显,不随光标相位消失");
    // 锚点稳显：行底区域找 brand-cyan 像素（锚点色 = select_handle）
    let has_anchor = |buf: &[u32]| {
        (1100..1140usize).any(|y| (0..w as usize).any(|x| buf[y * w as usize + x] == 0x0000_D4FF))
    };
    assert!(has_anchor(&on), "选择锚点必须出现");
    assert!(has_anchor(&off), "锚点稳显不闪烁");
    // 菜单像素：菜单气泡底 menu_bg 必须出现。扫描带由几何给出（2026-09-03
    // ⑤号迭代后菜单贴选区上方、随选区走位，不再写死屏幕带——眼手同尺）
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("选择态几何必须存在");
    let (my0, my1) = (geo.menu_y as usize, (geo.menu_y + geo.menu_h) as usize);
    let has_menu = |buf: &[u32]| {
        (my0..my1).any(|y| (180..420usize).any(|x| buf[y * w as usize + x] == 0x0020_2028))
    };
    assert!(has_menu(&on), "选择菜单气泡必须出现");
}

// BAR-046 2026-09-03 ①②号迭代回归钉：锚点柄两图元（三角/矩形）水平中心
// 对齐 + 触摸几何（热区中心）≡ 视觉中心。原案：三角中心 ax+14、矩形中心
// ax+4 静态错位 10px；热区中心取柄左缘 tip 点，与视觉中心错位 14/18px——
// 指按在看得见的柄上却落热区外，锚点拖不动。判卷：渲染缓冲里锚点色
// （0x0000_D4FF）像素的包围盒中心 ≡ bar_selection_geometry 锚点坐标（±2px）
#[test]
fn spec_bar046_锚点视觉中心等于热区中心() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    // 选区 0..2 控制在同一行内（600 宽每行约 5 个全角字），两个锚点同行
    // 不同 x：左 idx0→ax≈118，右 idx2→ax≈199，x 带不重叠，按 x<160 分簇
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 0,
        selection_end: 2,
    };
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("选择态几何必须存在");
    // caret_on=false：光标也是青色系，避免污染锚点色像素聚类
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    // y>1000 避开栏顶发丝线（accent 同色）
    let cluster = |x0: u32, x1: u32| -> (f64, f64, u32) {
        let (mut min_x, mut max_x, mut min_y, mut max_y, mut n) = (u32::MAX, 0, u32::MAX, 0, 0u32);
        for y in 1000..h {
            for x in x0..x1 {
                if buf[(y * w + x) as usize] == 0x0000_D4FF {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                    n += 1;
                }
            }
        }
        (
            f64::from(min_x + max_x) / 2.0,
            f64::from(min_y + max_y) / 2.0,
            n,
        )
    };
    for (name, (x0, x1), anchor) in [
        ("左", (0u32, 160u32), geo.left_anchor),
        ("右", (160u32, 320u32), geo.right_anchor),
    ] {
        let (cx, cy, n) = cluster(x0, x1);
        assert!(n > 100, "{name}锚点柄必须有足够像素（n={n}）");
        assert!(
            (cx - anchor.0).abs() <= 2.0,
            "{name}锚点水平：柄视觉中心 {cx} ≡ 热区中心 {}（±2）",
            anchor.0
        );
        assert!(
            (cy - anchor.1).abs() <= 2.0,
            "{name}锚点垂直：柄视觉中心 {cy} ≡ 热区中心 {}（±2）",
            anchor.1
        );
    }
}

// BAR-050 2026-09-03 用户实拍放大指认：拖拽柄「脖子」有向内凹的缝隙——
// 三角形底边平直 28px，下方正方形却是四角全圆（半径 8），上边角内收、
// 三角底边两角微凸，一凹一凸接缝豁开。契约：正方形上边角改直角（下边角
// 保持圆角）——三角底边与方形顶边平直 28px 对接（Android 原生柄剪影）。
// BAR-052 契约承接：三图元拼接改 fill_pin_handle 一体光栅后，肩部为
// 切弧过渡带（行宽 26→28 渐进），本钉 ≥26 宽度下限继续守凹口不回潮；
// 平滑硬保证在 spec_bar052。
// 判卷：方形顶部 8 行（原圆角内收带）每行柄色像素必须满宽（≥26/28），
// 未修时该区域每行仅中段 ~12px，本钉必红。
#[test]
fn spec_bar050_锚点柄接缝无凹口() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 0,
        selection_end: 2,
    };
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("选择态几何必须存在");
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    // 右锚点（x>160 那枚）：柄视觉中心 (ax, tip+18) → 方形顶边 y = tip+11 =
    // cy-7，内收带 = 顶边起 8 行。逐行数柄色（0x0000_D4FF）像素宽度。
    let (ax, cy) = geo.right_anchor;
    let top_row = (cy - 7.0) as u32;
    let x0 = (ax - 20.0).max(0.0) as u32;
    let x1 = ((ax + 20.0) as u32).min(w);
    for dy in 0..8u32 {
        let y = top_row + dy;
        let n = (x0..x1)
            .filter(|&x| buf[(y * w + x) as usize] == 0x0000_D4FF)
            .count();
        assert!(
            n >= 26,
            "方形顶边内收带第 {dy} 行柄色像素必须满宽（实测 {n} < 26——接缝凹口未愈）"
        );
    }
}

// BAR-050 同族余案（用户：「既然发现了就一起修掉」）：光标定位柄同构画法
// （36 三角 + 44×44 r12 方块），三角底边窄于方形边长，圆角内收后两腰凹槽
// 不明显但接缝仍豁。判卷：定位柄方块顶边 12 行（原圆角内收带）逐行满宽
// ≥42/44；未修时顶带行仅 ~20px，本钉必红。
// BAR-052 契约承接：一体图钉光栅后肩部是切弧过渡带（行宽 40→44 渐进），
// 阈值随契约改 ≥40；「无凹口」的硬保证移交 spec_bar052 平滑钉（逐行
// |Δx| 上限），本钉守宽度下限防凹口回潮。
#[test]
fn spec_bar050_定位柄接缝无凹口() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 5, // 中段光标，定位柄落光标行底
        handle: true,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    const HANDLE: u32 = 0x003B_82F6; // SELECT_BG
    // 定位柄是缓冲里唯一 SELECT_BG 来源：三角尖 = 最顶行，方块顶 = 尖+16
    let apex = (0..h)
        .find(|&y| (0..w).any(|x| buf[(y * w + x) as usize] == HANDLE))
        .expect("定位柄必现");
    for dy in 16..28u32 {
        let y = apex + dy;
        let n = (0..w)
            .filter(|&x| buf[(y * w + x) as usize] == HANDLE)
            .count();
        assert!(
            n >= 40,
            "定位柄肩部过渡带第 {} 行柄色像素必须守宽度下限（实测 {n} < 40——凹口回潮）",
            dy - 16
        );
    }
}

// BAR-051 2026-09-03 用户放大实拍再指认两条柄形对位：①选择锚点三角比下方
// 方块偏右约 1px——fill_triangle_up 旧语义以 cx 为对称轴画闭区间（底行
// 2*(w/2)+1 奇数宽），与偶数宽 fill_rect 方块对缝必差半像素，多出的那一
// 列恒在右侧；②定位柄 BAR-050 平顶后三角底边(37)窄于方块边长(44)，接缝
// 由凹变凸。修复：光栅改左缘 x0+精确宽度语义（底行恰好 [x0,x0+w)，与
// fill_rect 同锚），定位柄三角加宽 36→44 与方块同边长。
// 判卷：三角最底可见行（方块顶上一行）x 跨度中心 ≡ 方块行 x 跨度中心；
// 定位柄加判三角底行宽 ≥36（旧 33 必红）。
#[test]
fn spec_bar051_锚点三角与方块同轴() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 0,
        selection_end: 2,
    };
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("选择态几何必须存在");
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    // 窗内柄色像素的 x 跨度 [lo,hi]（窗 = 锚点中心 ±20，同 spec_bar050）
    let span = |y: u32, x0: u32, x1: u32| -> (u32, u32) {
        let (mut lo, mut hi) = (u32::MAX, 0);
        for x in x0..x1 {
            if buf[(y * w + x) as usize] == 0x0000_D4FF {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        (lo, hi)
    };
    for (name, anchor) in [("左", geo.left_anchor), ("右", geo.right_anchor)] {
        let (ax, cy) = anchor;
        let top_row = (cy - 7.0) as u32; // 方块顶边（同 spec_bar050 几何推导）
        let x0 = (ax - 20.0).max(0.0) as u32;
        let x1 = ((ax + 20.0) as u32).min(w);
        // 三角最底可见行 = 方块顶上一行；方块参照行取平顶带中位
        let (tlo, thi) = span(top_row - 1, x0, x1);
        let (slo, shi) = span(top_row + 4, x0, x1);
        assert!(
            tlo + thi == slo + shi,
            "{name}锚点三角与方块必须同轴：三角行 [{tlo},{thi}] 与方块行 [{slo},{shi}] 中心差 {}px",
            (tlo + thi).abs_diff(slo + shi)
        );
    }
}

// BAR-051 ②号对位：定位柄三角底边加宽至与方块同边长（44）且同轴。
// 判卷：方块顶上一行（三角最底可见行）宽 ≥36 且跨度中心 ≡ 方块行中心。
#[test]
fn spec_bar051_定位柄三角与方块同宽同轴() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 5, // 中段光标，定位柄落光标行底
        handle: true,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    const HANDLE: u32 = 0x003B_82F6; // SELECT_BG
    // 定位柄是缓冲里唯一 SELECT_BG 来源（同 spec_bar050 前提）
    let apex = (0..h)
        .find(|&y| (0..w).any(|x| buf[(y * w + x) as usize] == HANDLE))
        .expect("定位柄必现");
    let span = |y: u32| -> (u32, u32) {
        let (mut lo, mut hi) = (u32::MAX, 0);
        for x in 0..w {
            if buf[(y * w + x) as usize] == HANDLE {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        (lo, hi)
    };
    // 方块顶 = 尖+16（同 spec_bar050 几何）；三角最底可见行 = 顶上一行
    let (tlo, thi) = span(apex + 15);
    let (slo, shi) = span(apex + 20);
    assert!(
        thi - tlo + 1 >= 36,
        "定位柄三角底边必须加宽对接方块：实测 {}px < 36",
        thi - tlo + 1
    );
    assert!(
        tlo + thi == slo + shi,
        "定位柄三角与方块必须同轴：三角行 [{tlo},{thi}] 与方块行 [{slo},{shi}] 中心差 {}px",
        (tlo + thi).abs_diff(slo + shi)
    );
}

// BAR-052 2026-09-03 用户实拍三指认：两柄三角/方块交接处生硬——成熟输入
// 法柄（Android 原生图钉柄）此处是钝角圆角（肩部 fillet：斜边与立边用
// 精确切圆弧过渡），不是平顶直角的硬拼接。病灶实测：平顶拼接让承载块在
// 接缝行比三角行宽出 2~4px——左缘逐行 |Δx| 出现 2px(锚点)/3px(定位柄)
// 台阶，且 45° 斜边到立边在一个像素上瞬时转向。
// 契约（fill_pin_handle 一体光栅）：斜边经半径 r_sh 切弧摊 4~6 行过渡到
// 立边——①逐行左缘 |Δx| ≤ 1（无台阶）；②全宽立边在顶点行之后才抵达
// （平顶版接缝行即满宽）。判卷用右锚点（左锚点同构由同代码路径覆盖）。
#[test]
fn spec_bar052_锚点柄肩部钝角圆角() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 0,
        selection_end: 2,
    };
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("选择态几何必须存在");
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    let (ax, cy) = geo.right_anchor;
    let x0 = (ax - 20.0).max(0.0) as u32;
    let x1 = ((ax + 20.0) as u32).min(w);
    // 柄竖跨：尖顶 cy-19 ～ 承载块底 cy+20（几何 tip=cy-18，外沿不变）
    let span = |y: u32| -> Option<(u32, u32)> {
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for x in x0..x1 {
            if buf[(y * w + x) as usize] == 0x0000_D4FF {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        (lo <= hi).then_some((lo, hi))
    };
    let y_v = (cy - 6.0) as u32; // 顶点行（斜边抵立边处）：tip+12 = cy-6
    let (mut prev, mut first_full, mut max_w) = (None, None, 0u32);
    // 平滑判卷只扫 斜边+肩弧+立边起点（cy-19..cy-2）；再往下的底角是
    // 标准圆角矩形收边（末行固有 r-√(2r-1) 跳变，BAR-050 起用户认可的
    // 既有剪影，不在本契约内）
    for y in (cy - 19.0) as u32..=(cy - 2.0) as u32 {
        let Some((lo, hi)) = span(y) else { continue };
        if let Some(p) = prev {
            assert!(
                lo.abs_diff(p) <= 1,
                "肩部逐行过渡必须平滑：行 {y} 左缘 {lo} 与上行 {p} 差 {}px——硬台阶（钝角圆角未愈）",
                lo.abs_diff(p)
            );
        }
        if hi - lo + 1 >= 28 && first_full.is_none() {
            first_full = Some(y);
        }
        max_w = max_w.max(hi - lo + 1);
        prev = Some(lo);
    }
    assert_eq!(max_w, 28, "柄身最大宽度必须保持 28px（实测 {max_w}）");
    assert!(
        first_full.expect("柄身必达全宽") > y_v,
        "钝角圆角：全宽立边必须在顶点行 {y_v} 之后抵达（实测第 {} 行即满宽——平顶硬拼接）",
        first_full.unwrap()
    );
}

// BAR-052 ②号：定位柄同契约（斜率 m=21/17 非 45°，切圆按 m 通式）。
#[test]
fn spec_bar052_定位柄肩部钝角圆角() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 5,
        handle: true,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    const HANDLE: u32 = 0x003B_82F6; // SELECT_BG
    let apex = (0..h)
        .find(|&y| (0..w).any(|x| buf[(y * w + x) as usize] == HANDLE))
        .expect("定位柄必现");
    let span = |y: u32| -> Option<(u32, u32)> {
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for x in 0..w {
            if buf[(y * w + x) as usize] == HANDLE {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        (lo <= hi).then_some((lo, hi))
    };
    let y_v = apex + 17; // 顶点行（同 spec_bar050/051 几何：尖+17 抵立边）
    let (mut prev, mut first_full, mut max_w) = (None, None, 0u32);
    // 平滑判卷只扫 斜边+肩弧+立边起点（apex..y_v+5）；底角标准圆角收边
    // 不在本契约内（同锚点钉注）。阈值 ≤2：m=21/17 斜边逐行 1.24px，
    // 取整后固有 1~2px 步进；旧平顶拼接的接缝台阶是 3px（三角行 38px →
    // 承载块行 44px 突变），本钉以此分界
    for y in apex..=y_v + 5 {
        let Some((lo, hi)) = span(y) else { continue };
        if let Some(p) = prev {
            assert!(
                lo.abs_diff(p) <= 2,
                "肩部逐行过渡必须平滑：行 {y} 左缘 {lo} 与上行 {p} 差 {}px——硬台阶（钝角圆角未愈）",
                lo.abs_diff(p)
            );
        }
        if hi - lo + 1 >= 44 && first_full.is_none() {
            first_full = Some(y);
        }
        max_w = max_w.max(hi - lo + 1);
        prev = Some(lo);
    }
    assert_eq!(max_w, 44, "柄身最大宽度必须保持 44px（实测 {max_w}）");
    assert!(
        first_full.expect("柄身必达全宽") > y_v,
        "钝角圆角：全宽立边必须在顶点行 {y_v} 之后抵达（实测第 {} 行即满宽——平顶硬拼接）",
        first_full.unwrap()
    );
}

// BAR-046 2026-09-03 ⑤号迭代回归钉：菜单贴选区上方 12px（原案取选区垂直
// 中心再 -20，多行选区时菜单浮在半空离选区老远）。判卷：像素扫描菜单
// 气泡底缘与选区高亮顶缘的垂直间距 = 12±2
#[test]
fn spec_bar046_菜单贴选区上方() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 2,
        selection_end: 6,
    };
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    let has = |buf: &[u32], y: u32, color: u32| {
        (0..w as usize).any(|x| buf[(y * w) as usize + x] == color)
    };
    let sel_top = (0..h)
        .find(|&y| has(&buf, y, 0x0044_88DD))
        .expect("选区高亮必现");
    let menu_bot = (0..h)
        .rev()
        .find(|&y| has(&buf, y, 0x0020_2028))
        .expect("菜单气泡必现");
    assert!(menu_bot < sel_top, "菜单必须在选区上方");
    let gap = sel_top - menu_bot - 1;
    assert!(
        (10..=14).contains(&gap),
        "菜单贴选区 12px（±2），实测 gap={gap}"
    );
}

// BAR-046 2026-09-03 ④号迭代回归钉：菜单四格绘文字标签（原案 MVP 空色块，
// 实拍菜单条只有分隔线，用户不知道哪格是什么）。判卷：菜单区域内亮像素
// （文字 menu_text 0xE0E0E0 混合气泡底的结果，三通道 >0x80）数量超阈——
// 分隔线 0x616165 与纯气泡底都不达此阈，空色块场景本钉必红
#[test]
fn spec_bar046_菜单按钮绘文字() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let snap = kfm_na::input_bar::BarSnap {
        text: "一二三四五六七八九十".to_string(),
        focused: true,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 2,
        selection_end: 6,
    };
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("选择态几何必须存在");
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    let mut bright = 0u32;
    for y in geo.menu_y..geo.menu_y + geo.menu_h {
        for x in geo.menu_x..geo.menu_x + geo.menu_w {
            let p = buf[(y * w + x) as usize];
            let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
            if r > 0x80 && g > 0x80 && b > 0x80 {
                bright += 1;
            }
        }
    }
    assert!(
        bright > 200,
        "菜单四格必须有文字标签（亮像素 {bright} > 200）"
    );
}

// BAR-049 2026-09-03 用户对照其他输入框实拍指正：「输入的内容其实并不是
// 紧贴着栏的上下沿的，而是有一段距离」——kfmv4 `.ai-input` padding 14px
// CSS ≈ 40 物理（1260 屏 3x DPI）。na 原来文字/高亮贴死 field 上下沿，
// 全选时高亮顶到框线，视觉上「溢出感」的根子之一。契约：文本视口 =
// field 上下各收 TEXT_PAD_Y(40) 内衬，文字/高亮/光标/锚点/菜单锚/滚动
// 钳制全部吃这把尺。判卷：内衬带内不得有选区高亮像素；视口内高亮健在。
#[test]
fn spec_bar049_文本内衬_高亮不贴框沿() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1400u32);
    let text: String = "一二三四五六七八九十".repeat(5);
    let snap = kfm_na::input_bar::BarSnap {
        text,
        focused: true,
        lines: 5,
        cursor: 50,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: true,
        selection_start: 0,
        selection_end: 50,
    };
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    let bar_h = kfm_na::input_bar::height_for_lines(5);
    let field_top = h - bar_h + 32;
    let field_bottom = field_top + (bar_h - 64);
    let pad = kfm_na::input_bar::TEXT_PAD_Y;
    let (mut inner, mut pad_hit) = (0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if buf[(y * w + x) as usize] == 0x0044_88DD {
                if y < field_top + pad || y >= field_bottom - pad {
                    pad_hit += 1;
                } else {
                    inner += 1;
                }
            }
        }
    }
    assert!(inner > 1000, "视口内高亮必须健在（inner={inner}）");
    assert_eq!(
        pad_hit, 0,
        "内衬带（上下各 {pad}px）内不得有高亮像素（pad_hit={pad_hit}）"
    );
}

// BAR-048 2026-09-03 用户实拍：长文全选后上下滚动输入栏，选择菜单跟着
// 「被隐藏的选区首行」往页面上方爬——内容滚得越多菜单爬得越高。病灶：
// 菜单锚定选区首行 line_y(row_s)，全选时首行早滚出栏顶（不可见），菜单
// 追着隐藏位置走。原生（Android/浏览器）语境菜单只锚「看得见的选区」，
// 选区整段滚出视口则菜单消失。判卷：全选+尾锚现场，菜单必须锚在第一个
// 可见行上方（而不是首行上方）；选区滚出视口时几何返回 None（不画不触）。
#[test]
fn spec_bar048_菜单锚可见选区() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1400u32);
    let text: String = "一二三四五六七八九十".repeat(5); // 50 字 ≈ 10 行
    let snap = kfm_na::input_bar::BarSnap {
        text: text.clone(),
        focused: true,
        lines: 5,
        cursor: 50,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true, // 全选后尾锚——选区首行滚出栏顶，正是实拍现场
        selecting: true,
        selection_start: 0,
        selection_end: 50,
    };
    let geo = tv
        .bar_selection_geometry(&snap, w, h, 0)
        .expect("全选必有可见选区，几何必须存在");
    // 与渲染同一把尺算期望：field 几何 + 文本视口（BAR-049 内衬）→ 钉位
    let n_lines = 10u32; // 50 字 / 每行约 5 字
    let bar_h = kfm_na::input_bar::height_for_lines(5);
    let field_top = h - bar_h + 32;
    let field_h = bar_h - 64;
    let ty0 = field_top + kfm_na::input_bar::TEXT_PAD_Y;
    let (_, eff, _) = kfm_na::input_bar::viewport_geometry(
        n_lines,
        kfm_na::input_bar::text_view_h(field_h),
        true,
        0,
    );
    let text_top = ty0 as i32 - eff;
    // 菜单应钉「文本视口上缘上方 12px」固定位（选区起点在视口之上 → 锚 y
    // 钳到视口上缘，不追部分可见行的连续 y——锯齿效应的根治）
    let expect_y = (ty0 as i32 - kfm_na::input_bar::MENU_H as i32 - 12).max(8) as u32;
    assert_eq!(
        geo.menu_y, expect_y,
        "选区起点滚出视口时菜单必须钉文本视口上缘固定位"
    );
    // 反例钉：若锚的是隐藏首行（row 0），menu_y 会是 text_top-84，远小于此
    let buggy_y = (text_top - kfm_na::input_bar::MENU_H as i32 - 12).max(8) as u32;
    assert!(
        geo.menu_y > buggy_y + 100,
        "菜单不得追隐藏的选区首行（buggy={buggy_y} 实得 {}）",
        geo.menu_y
    );

    // 场景二：小选区整段滚出视口 → 菜单消失（几何 None）
    let snap2 = kfm_na::input_bar::BarSnap {
        selecting: true,
        selection_start: 0,
        selection_end: 2, // 第 1 行，尾锚下远在栏顶之上
        ..snap
    };
    assert!(
        tv.bar_selection_geometry(&snap2, w, h, 0).is_none(),
        "选区整段滚出视口，菜单/锚点几何必须消失"
    );
}

// BAR-048 复测钉（2026-09-03 用户复测：「不会移动了，但是会上下抖动」）：
// 锯齿效应——锚第一个可见行时，滚动中该行 y 连续移动 63px，跨行边界锚点
// 跳行又瞬移回 63px，表现为菜单上下抖动。契约：选区起点在视口之上时菜单
// 钉死栏顶固定位，同一段滚动行程内任意两个滚动位 menu_y 恒等。
#[test]
fn spec_bar048_菜单滚动钉死不抖动() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1400u32);
    let text: String = "一二三四五六七八九十".repeat(5);
    let base = kfm_na::input_bar::BarSnap {
        text,
        focused: true,
        lines: 5,
        cursor: 50,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: false, // 手动滚动态（用户抖动现场）
        selecting: true,
        selection_start: 0,
        selection_end: 50,
    };
    // 滚动行程中段两个位置（都在「选区起点滚出栏顶」区段内）
    let y_at = |px: i32| {
        tv.bar_selection_geometry(
            &kfm_na::input_bar::BarSnap {
                scroll_px: px,
                ..base.clone()
            },
            w,
            h,
            0,
        )
        .expect("选区有可见部分")
        .menu_y
    };
    let (y1, y2, y3) = (y_at(140), y_at(170), y_at(200));
    assert_eq!(y1, y2, "滚动 30px 菜单不得移动（锯齿抖动）");
    assert_eq!(y2, y3, "滚动跨行边界菜单也不得跳（锯齿抖动）");
    // 钉的就是文本视口上缘上方 12px 固定位（BAR-049 内衬后同尺）
    let bar_h = kfm_na::input_bar::height_for_lines(5);
    let ty0 = h - bar_h + 32 + kfm_na::input_bar::TEXT_PAD_Y;
    assert_eq!(
        y1,
        ty0 - kfm_na::input_bar::MENU_H - 12,
        "钉位 = 文本视口上缘上方 12px"
    );
}

// BAR-047 2026-09-03 用户实拍：粘贴长文（知乎链接+多段内容）后全选，多行
// 选区高亮盖穿输入栏圆角框——半滚出栏顶的行拿满行高（63px）高亮矩形，
// 画出栏框上缘之外压过边框，视觉上蓝色块「溢出」整个栏。病灶：文字有
// per-pixel 垂直裁剪（Some((field_y0, field_y1))），选区高亮矩形什么都没有。
// 判卷：select_bg 像素必须全部落在 field 矩形内（上下左右四向）。
#[test]
fn spec_bar047_选区高亮不溢出文本区() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1400u32);
    let text: String = "一二三四五六七八九十".repeat(5); // 50 字 ≈ 10 行，超 5 行封顶
    let snap = kfm_na::input_bar::BarSnap {
        text,
        focused: true,
        lines: 5,
        cursor: 50,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true, // 粘贴后跟随尾锚——首行半滚出栏顶，正是实拍现场
        selecting: true,
        selection_start: 0,
        selection_end: 50,
    };
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    // field 几何与 render_inputbar 同一把尺（全用公开常量/函数）
    let bar_h = kfm_na::input_bar::height_for_lines(5);
    let field_top = h - bar_h + 32;
    let field_bottom = field_top + (bar_h - 64);
    let field_left = kfm_na::input_bar::MARGIN_X_PX;
    let field_right = w
        - kfm_na::input_bar::MARGIN_X_PX
        - kfm_na::input_bar::SEND_W_PX
        - kfm_na::input_bar::GAP_PX;
    let (mut n, mut bad) = (0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if buf[(y * w + x) as usize] == 0x0044_88DD {
                n += 1;
                if y < field_top || y >= field_bottom || x < field_left || x >= field_right {
                    bad += 1;
                }
            }
        }
    }
    assert!(n > 1000, "全选高亮必须有足量像素（n={n}）");
    assert_eq!(bad, 0, "高亮不得画出 field 矩形（溢出像素 {bad}）");
}

// BAR-045: prompt_bar.rs 切片倒挂崩溃夜检——防御性切片+display_text 同源
// 实机 panic.log  witnessed: slice index starts at 21/363/794/42 but ends at
// 18/303/754/39。本判卷不追究精确复现路径（可能为行号漂移/inline 导致），
// 只验证：极端 cursor/组合态/滚动组合下 render_inputbar 与 bar_cursor_at
// 均不 panic，且点按换算与渲染同用 display_text。
#[test]
fn spec_inputbar_防御性切片不崩溃() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1400u32);
    let long = "一二三四五六七八九十".repeat(10); // 100 字，多行
    let mut buf = vec![0u32; (w * h) as usize];
    // 组合态插在中间、cursor 越界、scroll_px 极大、follow 切换
    let combos = vec![
        (25usize, "pinyin".to_string(), true, 0i32),
        (50, "pinyin".to_string(), false, 10_000),
        (200, "".to_string(), false, -500),
        (0, "abcdefghijklmnopqrstuvwxyz".to_string(), true, 0),
        (100, "xyz".to_string(), false, 5_000),
    ];
    for (cursor, composing, follow, scroll_px) in combos {
        let snap = kfm_na::input_bar::BarSnap {
            text: long.clone(),
            focused: true,
            lines: 20,
            cursor,
            handle: true,
            composing: composing.clone(),
            scroll_px,
            follow,
            selecting: false,
            selection_start: 0,
            selection_end: 0,
        };
        // 只判「不 panic」；视觉正确性由其他判卷负责
        tv.render_inputbar(&mut buf, w, h, 0, &snap, false, true);
        tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
        // 点按换算在组合态下也不 panic，且与渲染同源 display_text
        let _ = tv.bar_cursor_at(&snap, w, 100.0, 100.0);
        let _ = tv.bar_cursor_at(&snap, w, 10_000.0, 10_000.0);
    }
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
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    let mut tail = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut tail, w, h, 0, &base(true, 0), false, true);
    // 最大偏移 = 条带高 - 文本视口高（BAR-049 内衬后同尺：10×63=630,
    // field 408 收 2×40 内衬 = 328，max_eff = 302）
    let max_eff =
        (10 * kfm_na::input_bar::LINE_STEP_PX - kfm_na::input_bar::text_view_h(472 - 64)) as i32;
    let mut eqv = vec![0u32; (w * h) as usize];
    tv.render_inputbar(&mut eqv, w, h, 0, &base(false, max_eff), false, true);
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

// ========== BAR-043：follow→手动交接播种(视口瞬移根治) ==========
// 判卷点:尾锚态第一笔滚动必须从「当前显示位(尾)」续算,不许从 raw=0(头)起算。
// 2026-09-02 装机实锤:scrollpx -200 视口瞬移文本头(scroll_px raw 停 0,
// clamp 后=0=头)——用户报「第一下失效/比例失真」的真根因,与 BAR-042 死区同族

#[test]
fn spec_bar043_follow转手动首滚_从尾锚续算() {
    let bar = kfm_na::input_bar::InputBarState::new();
    bar.insert_text(&"测滚动文本。".repeat(30));
    bar.set_lines(15);
    let field_h = 300u32;
    let max_eff = (15 * kfm_na::input_bar::LINE_STEP_PX).saturating_sub(field_h) as i32;
    bar.scroll_by_px(-200, field_h); // 尾锚态第一笔:往头 200px
    let s = bar.snap();
    assert!(!s.follow, "滚动后必须脱锚");
    let (_, eff, _) = kfm_na::input_bar::viewport_geometry(15, field_h, s.follow, s.scroll_px);
    assert_eq!(
        eff,
        max_eff - 200,
        "交接必须播种:视口=尾锚位-200,不许瞬移到头(eff=0)"
    );
}

#[test]
fn spec_bar043_尾锚续算_连笔累积_到头钳住() {
    let bar = kfm_na::input_bar::InputBarState::new();
    bar.insert_text(&"测滚动文本。".repeat(30));
    bar.set_lines(15);
    let field_h = 300u32;
    bar.scroll_by_px(-200, field_h);
    bar.scroll_by_px(-200, field_h);
    bar.scroll_by_px(-9999, field_h); // 远超量程 → 钳头
    let s = bar.snap();
    let (_, eff, _) = kfm_na::input_bar::viewport_geometry(15, field_h, s.follow, s.scroll_px);
    assert_eq!(eff, 0, "连笔累积正确,超量程钳头=0");
}

// ========== BAR-044：bar-inject 空读竞态(写半截被值守撞见=静默吞指令) ==========
// 2026-09-02 实锤:writer `cat >` 先截断后写,值守撞进截断窗口读到空串,
// 解析 0 条 applied=0 bad=0 无声无息。空读不许消费,留给下一轮读全量

#[test]
fn spec_bar044_空内容不消费() {
    assert!(
        !kfm_na::gate::bar_should_consume(""),
        "空读(截断窗口撞见)不许消费"
    );
    assert!(!kfm_na::gate::bar_should_consume("  \n "), "纯空白同空读");
    assert!(
        kfm_na::gate::bar_should_consume("scrollpx -200\n"),
        "有指令正常消费"
    );
}

// ---------- 期 1 第 2 层：GPU 收集口（trait TermEmu 委托钉） ----------
#[test]
fn spec_gpu_收集口_空网格空格子_字形供墨可装载() {
    use kfm_na::glyph_atlas::GlyphKey;
    use kfm_na::termview::TermEmu;
    let mut tv = host_termview(8, 2);
    // 空网格（全空格）：格子产出但字符是空格——收集口照样给格（背景决策
    // 归 grid_to_instances）。缓冲必须含四边边距（卡片壳几何 31px）——
    // 格原点 = 边距 + 格坐标，少给边距末行末列就被裁剪漏收
    let cells = TermEmu::gpu_cells(
        &mut tv,
        CELL_W * 8 + 2 * kfm_na::termview::MARGIN_X,
        CELL_H * 2 + margin_top_of() + kfm_na::termview::MARGIN_Y,
    );
    assert_eq!(cells.len(), 16);
    assert!(cells.iter().all(|c| c.c == ' ' && !c.wide && !c.spacer));
    // 图集供墨：'A' 主字体可路由（font 0），位图非空，off_y = 基线 - ymin - h
    let (fid, m, bmp, ox, oy) = TermEmu::rasterize_for_atlas(&tv, 'A').expect("DejaVu 有 A 字形");
    assert_eq!(fid, 0);
    assert_eq!(bmp.len(), m.width * m.height);
    assert!(m.width >= 1 && m.height >= 1);
    let _ = (ox, oy);
    // 空格 → None（空字形不进图集，图集契约）
    assert!(TermEmu::rasterize_for_atlas(&tv, ' ').is_none());
    // 装载后图集能查到（GlyphAtlas 契约串接）
    let mut atlas = kfm_na::glyph_atlas::GlyphAtlas::new(256, 256);
    atlas.insert(
        GlyphKey {
            font: 0,
            c: 'A',
            size: kfm_na::glyph_atlas::GLYPH_SIZE_TERM,
        },
        m.width as u32,
        m.height as u32,
        &bmp,
        ox,
        oy,
    );
    assert!(
        atlas
            .slot(&GlyphKey {
                font: 0,
                c: 'A',
                size: kfm_na::glyph_atlas::GLYPH_SIZE_TERM,
            })
            .is_some()
    );
}

#[test]
fn spec_gpu_收集口_喂字后有真格_宽字符标宽() {
    use kfm_na::termview::TermEmu;
    let mut tv = host_termview(8, 2);
    TermEmu::feed(&mut tv, "A中B\n".as_bytes());
    let (gw, gh) = (
        CELL_W * 8 + 2 * kfm_na::termview::MARGIN_X,
        CELL_H * 2 + margin_top_of() + kfm_na::termview::MARGIN_Y,
    );
    let cells = TermEmu::gpu_cells(&mut tv, gw, gh);
    // trait 委托与固有实现同源（同一份收集逻辑，两入口一字不差）
    let direct = TermView::collect_gpu_cells(&mut tv, gw, gh);
    assert_eq!(direct.len(), cells.len());
    // 第一行：A + 中(宽) + spacer + B = 4 格有字，第二行空格
    let a = cells.iter().find(|c| c.c == 'A').expect("A 格在");
    assert!(!a.wide && !a.spacer);
    let zhong = cells.iter().find(|c| c.c == '中').expect("中格在");
    assert!(zhong.wide, "中应标宽字符（DejaVu 无中→宽度判 2 格）");
    assert!(cells.iter().any(|c| c.spacer), "宽字符第二格应标 spacer");
    assert!(cells.iter().any(|c| c.c == 'B'));
    // 中：主字体无字形 → 供墨走 CJK？本夹具 cjk=None → tofu，None 亦可（契约：
    // 双字体都缺 → None 跳装载，GPU 端 misses 常驻不炸）
    let _ = TermEmu::rasterize_for_atlas(&tv, '中');
}

// ---------- 期 1 第 2 层 C 档：AI 页接入图集管线（GPU 文字） ----------

#[test]
fn spec_gpu_panel_split_真值表() {
    // 分支判定唯一裁决处（panel_split）：三分支语义收成真值对——
    // softbuffer 与 GLES 两路径都从这里取判定，漂移 = 眼手两张皮
    let h = 1000u32;
    // 终端页稳态：键行/网格在，面板不在
    assert_eq!(
        kfm_na::termview::panel_split(-1000, h),
        (true, false),
        "-h = 面板底边压在屏顶，不可见"
    );
    assert_eq!(kfm_na::termview::panel_split(-5000, h), (true, false));
    // AI 页靠泊：面板在，键行/网格不在
    assert_eq!(kfm_na::termview::panel_split(0, h), (false, true));
    // 过渡帧：两者都在（终端在下、面板移位压上）
    assert_eq!(kfm_na::termview::panel_split(-1, h), (true, true));
    assert_eq!(kfm_na::termview::panel_split(-999, h), (true, true));
}

#[test]
fn spec_cfg_split_真值表() {
    // 配置页分层判定（cfg_split，面板栈 §五B 2026-09-10）：与 panel_split
    // 同构的 X 向版本——softbuffer 与 GLES 两路径都从这里取判定
    let w = 1260u32;
    // 配置页靠泊在顶：配置页在，键行/网格不在
    assert_eq!(kfm_na::termview::cfg_split(0, w), (false, true));
    // 屏外右缘稳态：键行/网格在，配置页不在
    assert_eq!(
        kfm_na::termview::cfg_split(1260, w),
        (true, false),
        "+w = 完全屏外"
    );
    assert_eq!(kfm_na::termview::cfg_split(5000, w), (true, false));
    // 过渡帧：两者都在（终端在下、配置页从右缘滑入）
    assert_eq!(kfm_na::termview::cfg_split(1, w), (true, true));
    assert_eq!(kfm_na::termview::cfg_split(1259, w), (true, true));
}

#[test]
fn spec_ai页fit公式_饱和与裁剪() {
    // AI 页布局尺单源（ai_page_fit）：一屏行数 = (h - 顶 - 底 - inset)/64
    // 向下取整；余量不足饱和为 0。render_ai_page / ai_page_glyphs /
    // paint_ai_page_chrome 三方都吃这一把尺——公式漂移 = 视口错位
    let f = kfm_na::termview::ai_page_fit;
    assert_eq!(f(600, 0), 7, "(600-48-48-0)/64 = 7（与既有视口考题同尺）");
    assert_eq!(f(500, 120), 4, "(500-48-48-120)/64 = 4.4 → 4 向下取整");
    assert_eq!(f(96, 0), 0, "恰好零内容高（只有顶底边距）");
    assert_eq!(f(50, 100), 0, "负余量 saturating 饱和为 0，不许下溢");
}

// ---- BAR-067：栏带半透契约（2026-09-05，chrome 层真 alpha 直通后还原
// kfmv4 rgba(18,18,26,.85)——CPU 时代压平的不透明暗板在多行带高下成
// 黑墙）----

#[test]
fn spec_bar067_栏带底_半透写出() {
    // 栏带底色必须携带 CHROME_BAND_ALPHA（kfmv4 .85 半透还原）——
    // 条件 alpha 直通后终端内容 15% 透出；不透明暗板 = 黑墙复发
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必成");
    let (w, h) = (600u32, 1200u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let snap = kfm_na::input_bar::BarSnap {
        text: "hi".to_string(),
        focused: false,
        lines: 1,
        cursor: 0,
        handle: false,
        composing: String::new(),
        scroll_px: 0,
        follow: true,
        selecting: false,
        selection_start: 0,
        selection_end: 0,
    };
    tv.render_inputbar(&mut buf, w, h, 0, &snap, false, false);
    // 探针 = 带内左缘（MARGIN_X_PX 之外的带底区，避开键帽/发丝线/内芯）
    let band_h = kfm_na::input_bar::height_for_lines(1);
    let probe = buf[((h - band_h / 2) * w + 8) as usize];
    assert_eq!(
        (probe >> 24) & 0xFF,
        kfm_na::theme::CHROME_BAND_ALPHA,
        "栏带底必须携带半透 α"
    );
    assert_eq!(
        probe & 0x00FF_FFFF,
        0x0011_1119,
        "RGB = 主题 bg（kfmv4 事后色）"
    );
    // 发丝线探针（带顶第 2 行，blend_px α=102 叠半透带上）：α 必须仍是
    // 0xD9——blend_px 打掉 α 的旧病在此必红（装饰混合保透明度契约）
    let hair = buf[((h - band_h + 1) * w + w / 2) as usize];
    assert_eq!(
        (hair >> 24) & 0xFF,
        kfm_na::theme::CHROME_BAND_ALPHA,
        "发丝线混合必须保留底 α（blend_px 保 α 契约）"
    );
}

#[test]
fn spec_cfg页底装修_accent与平移钉() {
    // accent 入参时代（宪法 §2.2 召唤即随机，2026-09-12）：
    // ①靠泊位内芯 = **渐变暗底**（十二修 §三 渐变暗背景：dark(c1)→dark(c2)
    //   页环同尺 135° 不透明直出，dark(c)=lerp(c,黑,200)——取代 CARD_PAGE_BG
    //   平填；平填回潮即红），边框环有墨 ≠ 底色；
    // ②环色由 accent 驱动——换 accent 重画同位取样必须变色（不变 =
    //   accent 没接进涂装，满屏固定色；变异抽检：涂装写死常量 → 此钉红）；
    // ③X 平移语义不变：透明缘/可见区底色咬合/完全屏外零墨
    let (w, h) = (400u32, 500u32);
    let inset = 120u32;
    let acc_a = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let acc_b = kfm_na::ui::accent::AccentPair {
        c1: 0x0000_FF00,
        c2: 0x00FF_00FF,
    };
    // 页环渐变尺：原点 (16,16)，框 368×348（w/h−margin×2−inset），denom 714
    let dark = |c: u32| kfm_na::termview::lerp_rgb(c, 0, 200);
    let page_bg = |px: i64, py: i64, off_x: i64| {
        kfm_na::termview::ring_gradient_rgb(
            dark(acc_a.c1),
            dark(acc_a.c2),
            px - (16 + off_x),
            py - 16,
            714,
        )
    };
    let mut b0 = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_cfg_page_chrome(&mut b0, w, h, inset, 0, acc_a);
    assert_eq!(
        b0[(h / 2 * w + w / 2) as usize],
        page_bg(200, 250, 0),
        "靠泊位内芯必须 = 渐变暗底精确值（十二修；平填回潮即红）"
    );
    let ring_idx = ((h - inset) / 2 * w + 18) as usize;
    let ring = b0[ring_idx];
    assert_ne!(ring, 0, "边框环必须有墨");
    assert_ne!(
        ring,
        page_bg(18, ((h - inset) / 2) as i64, 0),
        "边框环必须异于内芯暗底"
    );
    let mut b1 = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_cfg_page_chrome(&mut b1, w, h, inset, 0, acc_b);
    assert_ne!(b1[ring_idx], ring, "accent 换了环色必须变（驱动钉）");
    // 平移钉
    let k = 137i32;
    let mut bk = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_cfg_page_chrome(&mut bk, w, h, inset, k, acc_a);
    let mid_row = &bk[((h / 2) * w) as usize..((h / 2) * w + w) as usize];
    assert!(
        mid_row[..k as usize].iter().all(|&p| p == 0),
        "面板左缘之左必须透明（终端透出的前提）"
    );
    assert_eq!(
        mid_row[(k as usize) + 100],
        page_bg(237, 250, i64::from(k)),
        "平移后可见区内芯 = 同尺渐变暗底（随面板刚体平移）"
    );
    // 完全屏外不落墨
    let mut bw = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_cfg_page_chrome(&mut bw, w, h, inset, w as i32, acc_a);
    assert!(bw.iter().all(|&p| p == 0), "完全屏外必须零墨");
}

#[test]
fn spec_ft_split_真值表() {
    // 文件树页分层判定（ft_split，三公民 §五B 2026-09-11）：cfg_split 的
    // 左缘家镜像——off ∈ [-w, 0]，=-w 即完全屏外左缘；softbuffer 与 GLES
    // 两路径都从这里取判定
    let w = 1260u32;
    // 文件树靠泊在顶：文件树在，键行/网格不在
    assert_eq!(kfm_na::termview::ft_split(0, w), (false, true));
    // 屏外左缘稳态：键行/网格在，文件树不在
    assert_eq!(
        kfm_na::termview::ft_split(-1260, w),
        (true, false),
        "-w = 完全屏外"
    );
    assert_eq!(kfm_na::termview::ft_split(-5000, w), (true, false));
    // 过渡帧：两者都在（终端在下、文件树从左缘滑入）
    assert_eq!(kfm_na::termview::ft_split(-1, w), (true, true));
    assert_eq!(kfm_na::termview::ft_split(-1259, w), (true, true));
}

#[test]
fn spec_ft页底装修_accent与平移钉() {
    // accent 入参时代（宪法 §2.2 召唤即随机，2026-09-12）：
    // ①靠泊位内芯 = **渐变暗底**（十二修 §三，与 cfg 页同规——平填回潮
    //   即红），边框环有墨 ≠ 内芯；
    // ②环色由 accent 驱动——换 accent 重画同位取样必须变色（不变 =
    //   accent 没接进涂装，满屏固定色；变异抽检：涂装写死常量 → 此钉红）；
    // ③X 平移语义不变：透明缘/可见区底色咬合/完全屏外零墨
    let (w, h) = (400u32, 500u32);
    let inset = 120u32;
    let acc_a = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let acc_b = kfm_na::ui::accent::AccentPair {
        c1: 0x0000_FF00,
        c2: 0x00FF_00FF,
    };
    let dark = |c: u32| kfm_na::termview::lerp_rgb(c, 0, 200);
    let page_bg = |px: i64, py: i64, off_x: i64| {
        kfm_na::termview::ring_gradient_rgb(
            dark(acc_a.c1),
            dark(acc_a.c2),
            px - (16 + off_x),
            py - 16,
            714,
        )
    };
    let mut b0 = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_ft_page_chrome(&mut b0, w, h, inset, 0, acc_a);
    assert_eq!(
        b0[(h / 2 * w + w / 2) as usize],
        page_bg(200, 250, 0),
        "靠泊位内芯必须 = 渐变暗底精确值（十二修；平填回潮即红）"
    );
    let ring_idx = ((h - inset) / 2 * w + 18) as usize;
    let ring = b0[ring_idx];
    assert_ne!(ring, 0, "边框环必须有墨");
    assert_ne!(
        ring,
        page_bg(18, ((h - inset) / 2) as i64, 0),
        "边框环必须异于内芯暗底"
    );
    let mut b1 = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_ft_page_chrome(&mut b1, w, h, inset, 0, acc_b);
    assert_ne!(b1[ring_idx], ring, "accent 换了环色必须变（驱动钉）");
    // 平移钉
    let k = 137i32;
    let mut bk = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_ft_page_chrome(&mut bk, w, h, inset, -k, acc_a);
    let mid_row = &bk[((h / 2) * w) as usize..((h / 2) * w + w) as usize];
    assert!(
        mid_row[(w as i32 - k) as usize..].iter().all(|&p| p == 0),
        "面板右缘之右必须透明（终端透出的前提）"
    );
    assert_eq!(
        mid_row[100],
        page_bg(100, 250, -i64::from(k)),
        "平移后可见区内芯 = 同尺渐变暗底（随面板刚体平移）"
    );
    // 完全屏外不落墨
    let mut bw = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_ft_page_chrome(&mut bw, w, h, inset, -(w as i32), acc_a);
    assert!(bw.iter().all(|&p| p == 0), "完全屏外必须零墨");
}

#[test]
fn spec_pt_split_真值表() {
    // 解析页分层判定（pt_split，四公民·三缘语义 §五B 2026-09-12）：右缘家
    // 与 cfg_split 同构同尺——off ∈ [0, +w]，=w 即完全屏外右缘；
    // softbuffer 与 GLES 两路径都从这里取判定
    let w = 1260u32;
    // 解析页靠泊在顶：解析页在，键行/网格不在
    assert_eq!(kfm_na::termview::pt_split(0, w), (false, true));
    // 屏外右缘稳态：键行/网格在，解析页不在
    assert_eq!(
        kfm_na::termview::pt_split(1260, w),
        (true, false),
        "+w = 完全屏外"
    );
    assert_eq!(kfm_na::termview::pt_split(5000, w), (true, false));
    // 过渡帧：两者都在（终端在下、解析页从右缘滑入）
    assert_eq!(kfm_na::termview::pt_split(1, w), (true, true));
    assert_eq!(kfm_na::termview::pt_split(1259, w), (true, true));
}

#[test]
fn spec_pt页底装修_accent与平移钉() {
    // accent 入参时代（宪法 §2.2 召唤即随机，2026-09-12）：
    // ①靠泊位内芯 = **渐变暗底**（十二修 §三，与 cfg 页同规——平填回潮
    //   即红），边框环有墨 ≠ 内芯；
    // ②环色由 accent 驱动——换 accent 重画同位取样必须变色（不变 =
    //   accent 没接进涂装，满屏固定色；变异抽检：涂装写死常量 → 此钉红）；
    // ③X 平移语义不变：透明缘/可见区底色咬合/完全屏外零墨
    let (w, h) = (400u32, 500u32);
    let inset = 120u32;
    let acc_a = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let acc_b = kfm_na::ui::accent::AccentPair {
        c1: 0x0000_FF00,
        c2: 0x00FF_00FF,
    };
    let dark = |c: u32| kfm_na::termview::lerp_rgb(c, 0, 200);
    let page_bg = |px: i64, py: i64, off_x: i64| {
        kfm_na::termview::ring_gradient_rgb(
            dark(acc_a.c1),
            dark(acc_a.c2),
            px - (16 + off_x),
            py - 16,
            714,
        )
    };
    let mut b0 = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_parser_page_chrome(&mut b0, w, h, inset, 0, acc_a);
    assert_eq!(
        b0[(h / 2 * w + w / 2) as usize],
        page_bg(200, 250, 0),
        "靠泊位内芯必须 = 渐变暗底精确值（十二修；平填回潮即红）"
    );
    let ring_idx = ((h - inset) / 2 * w + 18) as usize;
    let ring = b0[ring_idx];
    assert_ne!(ring, 0, "边框环必须有墨");
    assert_ne!(
        ring,
        page_bg(18, ((h - inset) / 2) as i64, 0),
        "边框环必须异于内芯暗底"
    );
    let mut b1 = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_parser_page_chrome(&mut b1, w, h, inset, 0, acc_b);
    assert_ne!(b1[ring_idx], ring, "accent 换了环色必须变（驱动钉）");
    // 平移钉
    let k = 137i32;
    let mut bk = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_parser_page_chrome(&mut bk, w, h, inset, k, acc_a);
    let mid_row = &bk[((h / 2) * w) as usize..((h / 2) * w + w) as usize];
    assert!(
        mid_row[..k as usize].iter().all(|&p| p == 0),
        "面板左缘之左必须透明（终端透出的前提）"
    );
    assert_eq!(
        mid_row[(k as usize) + 100],
        page_bg(237, 250, i64::from(k)),
        "平移后可见区内芯 = 同尺渐变暗底（随面板刚体平移）"
    );
    // 完全屏外不落墨
    let mut bw = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_parser_page_chrome(&mut bw, w, h, inset, w as i32, acc_a);
    assert!(bw.iter().all(|&p| p == 0), "完全屏外必须零墨");
}

#[test]
fn spec_cfg标签栏_涂装钉() {
    // 宪法 §四（2026-09-13 十一修，标签随机双色体系；2026-09-14 十二修
    // 选中块均匀渐变 + 渐变暗底）：
    // ①标签行带内有文字墨（标签不是空色块）；
    // ②标签块形态——选中 = **c1(顶)→c2(底) 竖向均匀渐变满填 α255**
    //   （十二修推翻两截硬切：dy 行色 = lerp(c1,c2,dy·255/(h−1)) 精确值——
    //   变异：两截/短渐变回潮即红，dy=12 处必已离 c1）；未选中 = **同一
    //   把 t 尺的均匀渐变薄态 α48 满块**（十三修推翻三段条带硬切——实机
    //   判「暗块依然硬切」），叠在**页内芯渐变暗底**上（十二修：dst 不再
    //   是平色 CARD_PAGE_BG）；
    //   上两角圆角（角外 = 页暗底）、下缘直边（末行靠左有墨——变异：
    //   下缘也圆角必红）；
    // ③每标签独立双色钉——未选中块的条带色 = **该标签自己的**双色
    //   （snap 色列），不是页 accent（变异：块色吃 paint 时 accent 参数即红；
    //   换页 accent 重画块色不变）；
    // ④底线钉——配合标签模式：整根 = 选中标签 c2 **纯色** α255、池区同宽、
    //   不越缘、单行不跨边（变异：画回反转渐变即红——中点与端点同纯色，
    //   渐变尺下两点必不同色）；空态也画（色列空 = 兜底 accent.c2）
    let (w, h) = (400u32, 500u32);
    let inset = 120u32;
    let acc_a = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let acc_b = kfm_na::ui::accent::AccentPair {
        c1: 0x0000_FF00,
        c2: 0x00FF_00FF,
    };
    // 标签 1 自己的双色（刻意与页 accent 不同——色列驱动钉）
    let pair_b = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_E000,
        c2: 0x0040_00FF,
    };
    let bg = kfm_na::ui::accent::CARD_PAGE_BG;
    // 页内芯渐变暗底（十二修 §三）：dark = lerp(c,黑,200)，页环同尺
    // 原点 (16,16) denom 714（框 368×348）
    let dark = |c: u32| kfm_na::termview::lerp_rgb(c, 0, 200);
    let page_bg = |px: i64, py: i64| {
        kfm_na::termview::ring_gradient_rgb(dark(acc_a.c1), dark(acc_a.c2), px - 16, py - 16, 714)
    };
    // blend 公式与 termview 私有 blend 逐字一致（事后色判卷尺）
    let blend = |fg: u32, dst: u32, a: u32| {
        let inv = 255 - a;
        let ch = |f: u32, d: u32| (f * a + d * inv) / 255;
        (ch((fg >> 16) & 0xFF, (dst >> 16) & 0xFF) << 16)
            | (ch((fg >> 8) & 0xFF, (dst >> 8) & 0xFF) << 8)
            | ch(fg & 0xFF, dst & 0xFF)
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut bar = kfm_na::ui::tab_bar::TabBar::new(&["API", "B"], 320);
    bar.set_colors(vec![acc_a, pair_b]); // 选中 0 → 页 accent ≡ acc_a
    let mut snap = bar.snap(0);
    // BAR-096 拆层：底线 span（池区左右内缘）改由壳层每帧喂——钉侧
    // 按池区几何补上（缺省 None = 不画底线）
    {
        let pa = kfm_na::ui::dual_pool::pool_area(w, h, inset);
        snap.line_span = Some((pa.x, pa.x + i64::from(pa.w)));
    }
    // 选中块初态 = 标签 0：x=61, oy=55, w=90, h=72（咬格钉同源读数，
    // 行高 2 格）；标签 B：x=169（61+90+18 间距 1 格），w=54
    let (cx, oy, cw) = (61usize, 55usize, 90usize);
    let ux = 169usize;

    let mut b0 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b0, w, h, inset, 0, acc_a);
    tv.paint_cfg_tab_bar(&mut b0, w, h, &snap, 0, inset, acc_a);

    // ②选中标签块：竖向均匀渐变——逐行精确值（t = dy·255/71）；
    // dy=12 必须已离 c1（变异：两截硬切/短渐变回潮 → 该点 = c1 即红）
    let grad_at =
        |dy: usize| kfm_na::termview::lerp_rgb(acc_a.c1, acc_a.c2, (dy as u32 * 255) / 71);
    assert_eq!(
        b0[(oy + 12) * w as usize + cx + 8],
        grad_at(12),
        "选中块 dy=12 = 均匀渐变精确值（非 c1 直出）"
    );
    assert_ne!(
        b0[(oy + 12) * w as usize + cx + 8],
        acc_a.c1,
        "dy=12 必须已离 c1（变异：两截硬切回潮即红）"
    );
    assert_eq!(
        b0[(oy + 36) * w as usize + cx + 8],
        grad_at(36),
        "选中块中行 = 均匀渐变中点精确值"
    );
    assert_eq!(
        b0[(oy + 60) * w as usize + cx + 8],
        grad_at(60),
        "选中块 dy=60 = 均匀渐变精确值"
    );
    assert_eq!(
        b0[oy * w as usize + cx],
        page_bg(61, 55),
        "上左角外 = 页暗底零墨（上圆角）"
    );
    assert_eq!(
        b0[(oy + 2) * w as usize + cx + 2],
        page_bg(63, 57),
        "角盒近角点必须零墨（圆弧裁外 = 页暗底；变异：上角直角化必红）"
    );
    assert_eq!(
        b0[oy * w as usize + cx + 18],
        acc_a.c1,
        "顶行弧尾以右必须 = c1（t=0；圆角只吃角盒）"
    );
    assert_eq!(
        b0[(oy + 71) * w as usize + cx + 2],
        acc_a.c2,
        "下缘末行靠左必须 = c2（t=255，下缘直边不圆角——变异：下缘圆角化必红）"
    );

    // ②+③未选中标签块：竖向均匀渐变薄态 α48 满块（十三修推翻三段
    // 条带硬切——与选中同一把 t = dy·255/71 尺只降 alpha），叠在页暗
    // 底上；色源是色列[1] 不是页 accent
    let thin_at = |dy: usize, px: i64, py: i64| {
        blend(
            kfm_na::termview::lerp_rgb(pair_b.c1, pair_b.c2, (dy as u32 * 255) / 71),
            page_bg(px, py),
            48,
        )
    };
    assert_eq!(
        b0[(oy + 6) * w as usize + ux + 4],
        thin_at(6, 173, 61),
        "未选中块 dy=6 = 均匀渐变薄态精确值"
    );
    assert_ne!(
        b0[(oy + 6) * w as usize + ux + 4],
        blend(pair_b.c1, page_bg(173, 61), 48),
        "dy=6 必须已离 c1（变异：条带硬切回潮即红）"
    );
    assert_ne!(
        b0[(oy + 6) * w as usize + ux + 4],
        blend(acc_a.c1, page_bg(173, 61), 48),
        "薄态色必须不是页 accent（变异：块色吃 paint 时 accent 参数即红）"
    );
    assert_eq!(
        b0[(oy + 36) * w as usize + ux + 4],
        thin_at(36, 173, 91),
        "未选中块中行 = 均匀渐变薄态中点精确值（变异：6% 白底回潮即红）"
    );
    assert_eq!(
        b0[(oy + 66) * w as usize + ux + 4],
        thin_at(66, 173, 121),
        "未选中块 dy=66 = 均匀渐变薄态精确值"
    );
    assert_eq!(
        b0[(oy + 36) * w as usize + ux - 1],
        page_bg(168, 91),
        "未选中块不外溢（块外即页暗底）"
    );

    // ④底线钉：uy = 标签行下缘紧挨 1px，池区同宽、不越缘；整根 =
    // 选中标签 c2 纯色 α255（端点与中点同色 = 非渐变的铁证）；uy+1 零墨
    let pa = kfm_na::ui::dual_pool::pool_area(w, h, inset);
    let uy = oy + 72;
    assert_eq!(
        b0[uy * w as usize + (pa.x + 4) as usize],
        acc_a.c2,
        "底线 = 选中标签 c2 纯色（左端）"
    );
    assert_eq!(
        b0[uy * w as usize + (pa.x + pa.w as i64 / 2) as usize],
        acc_a.c2,
        "底线中点同纯色（变异：画回渐变尺则中点必异色即红）"
    );
    assert_eq!(
        b0[uy * w as usize + (pa.x + pa.w as i64 - 4) as usize],
        acc_a.c2,
        "底线必须铺满池区宽（右缘内 4px 也有墨）"
    );
    assert_eq!(
        b0[uy * w as usize + (pa.x - 1) as usize],
        page_bg(60, 127),
        "底线不越池区左缘（缘外 = 页暗底）"
    );
    assert_eq!(
        b0[uy * w as usize + (pa.x + pa.w as i64) as usize],
        page_bg(345, 127),
        "底线不越池区右缘（缘外 = 页暗底）"
    );
    assert_eq!(
        b0[(uy + 1) * w as usize + (pa.x + 4) as usize],
        page_bg(65, 128),
        "底线单行不跨边（uy+1 = 页暗底）"
    );
    // 空态也画底线（装修不是内容，与页环同规）：色列空 = 兜底 accent.c2
    let bar_empty = kfm_na::ui::tab_bar::TabBar::new(&[], 320);
    let mut b2 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b2, w, h, inset, 0, acc_b);
    tv.paint_cfg_tab_bar(&mut b2, w, h, &bar_empty.snap(0), 0, inset, acc_b);
    assert_eq!(
        b2[uy * w as usize + (pa.x + 4) as usize],
        acc_b.c2,
        "空态也必须画底线（色列空 = 兜底 accent.c2）"
    );

    // ③色列驱动钉：换页 accent 重画（色列不动），块色/底线必须不变——
    // 数据源是 snap 色列，不是 paint 时 accent 参数
    let mut b1 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b1, w, h, inset, 0, acc_b);
    tv.paint_cfg_tab_bar(&mut b1, w, h, &snap, 0, inset, acc_b);
    assert_eq!(
        b1[(oy + 12) * w as usize + cx + 8],
        grad_at(12),
        "换页 accent 块色不变（色列驱动；变异：吃 paint accent 即红）"
    );
    assert_eq!(
        b1[uy * w as usize + (pa.x + 4) as usize],
        acc_a.c2,
        "换页 accent 底线不变（色列驱动）"
    );

    // ①文字墨：选中标签格心区必须有文字字形墨——深色字（CARD_PAGE_BG）
    // 压在渐变块上（渐变带色永≠CARD_PAGE_BG：G 通道 20→26 不经过 14）
    let mut text_ink = 0usize;
    for y in (oy + 20)..(oy + 44) {
        for x in (cx + 20)..(cx + cw - 20) {
            if b0[y * w as usize + x] == bg {
                text_ink += 1;
            }
        }
    }
    assert!(text_ink > 20, "选中标签格心必须有文字墨（{text_ink} px）");
}

#[test]
fn spec_三级框_涂装钉() {
    // 宪法 §五 池行条款 + §三 渐变暗背景（2026-09-14 十二修，未选中去框 +
    // 值框无边框化 + 渐变暗底）：
    // ①选中行（下池 focus 行）= 全包框：左粗缘 10px/三细边 3px = 135°
    //   渐变 α255 直出（色向 c1→c2 与页环同向同尺；角部只渐形状不渐色）；
    // ②未选中行 = **无框**：左粗缘位/顶边/右边/角部全部 = 渐变暗底不透明
    //   直出（任何框墨即红——α140 薄态/8% 白/左竖线回潮全被抓）；
    // ③上池值框 = 无边框组件：左竖线位/右缘/上缘全 = 渐变暗底（十一修
    //   形态②退役——变异：左竖线回潮即红）；
    // ④渐变暗底配方钉：dark(c) = lerp(c, 黑, 200)（字面量钉，实现侧常量
    //   漂移即红），行内芯同一把 135° 尺（页原点 (0,0)、页 denom）不透明
    //   直出——4% 白平填回潮即红
    use kfm_na::termview::{lerp_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let bg = kfm_na::ui::accent::CARD_PAGE_BG;
    let dark = |c: u32| lerp_rgb(c, 0, 200); // 字面量 200 = 配方钉本体
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1); // 与涂装同一把 135° 尺
    let row_bg = |px: i64, py: i64| ring_gradient_rgb(dark(acc.c1), dark(acc.c2), px, py, denom);
    // 单源钉：实现侧 frame_bg_rgb/FRAME_BG_DIM 必须 ≡ 本钉字面量配方
    // （实现常量漂移/暗底函数配方跑偏均即红）
    assert_eq!(kfm_na::termview::FRAME_BG_DIM, 200, "暗度常量 = 配方 200");
    assert_eq!(
        kfm_na::termview::frame_bg_rgb(acc.c1, acc.c2, 40, 72, denom),
        ring_gradient_rgb(dark(acc.c1), dark(acc.c2), 40, 72, denom),
        "frame_bg_rgb ≡ ring_gradient(dark,dark) 单源等价"
    );
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H); // 内边距 2 格×2 + 1 字段行
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![
        RowView {
            title: "系统管理".into(),
            meta: "1 项".into(),
        },
        RowView {
            title: "网络".into(),
            meta: String::new(),
        },
    ]);
    page.set_upper(vec![UpperRow {
        label: "默认服务器".into(),
        value: "本地终端".into(),
        is_dropdown: false,
    }]);
    let pg = page.snap(1000); // focus=0 → 行 0 选中、行 1 未选中

    let mut b0 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b0, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut b0, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut b0, w, h, &ps, &pg, 0, acc, 0, None, false);

    let r0 = cfg_page::lower_row_rect(0, &ps.lower); // 选中
    let r1 = cfg_page::lower_row_rect(1, &ps.lower); // 未选中
    assert!(
        r1.y + r1.h as i64 <= ps.lower.y + ps.lower.h as i64,
        "夹具前提：行 1 完整在池内"
    );

    // ①选中行全包框：左粗缘中带/顶边中点/右边中点 = 渐变 α255 精确直出
    let sel_band = [
        (r0.x + 4, r0.y + r0.h as i64 / 2, "选中行左粗缘"),
        (r0.x + r0.w as i64 / 2, r0.y + 1, "选中行顶边"),
        (r0.x + r0.w as i64 - 2, r0.y + r0.h as i64 / 2, "选中行右边"),
    ];
    for (px, py, name) in sel_band {
        assert_eq!(
            b0[py as usize * w as usize + px as usize],
            ring_gradient_rgb(acc.c1, acc.c2, px, py, denom),
            "{name}必须 = 渐变 α255 直出"
        );
    }
    // ①选中行角部：弧线中点（局部 (13,13)）颜色 = 纯渐变采样 α255
    // （只渐形状不渐色；变异：混白/降 alpha 即红）
    let (px, py) = (r0.x + 13, r0.y + 13);
    assert_eq!(
        b0[py as usize * w as usize + px as usize],
        ring_gradient_rgb(acc.c1, acc.c2, px, py, denom),
        "选中行角部 = 渐变 α255（渐细只渐形状）"
    );
    // ①选中行内芯 = 渐变暗底不透明直出（4% 白平填回潮即红）
    let (ix, iy) = (r0.x + r0.w as i64 / 2, r0.y + r0.h as i64 / 2);
    assert_eq!(
        b0[iy as usize * w as usize + ix as usize],
        row_bg(ix, iy),
        "选中行内芯 = 渐变暗底（不透明直出）"
    );

    // ②未选中行无框：原左粗缘位/顶边/右边/角部全部 = 渐变暗底
    // （框墨全退；变异：薄态 α140/8% 白/任何框线回潮即红）
    let unsel = [
        (r1.x + 4, r1.y + r1.h as i64 / 2, "未选中行原左粗缘位"),
        (r1.x + r1.w as i64 / 2, r1.y + 1, "未选中行顶边"),
        (
            r1.x + r1.w as i64 - 2,
            r1.y + r1.h as i64 / 2,
            "未选中行右边",
        ),
        (r1.x + 13, r1.y + 13, "未选中行角部"),
        (
            r1.x + r1.w as i64 / 2,
            r1.y + r1.h as i64 / 2,
            "未选中行内芯",
        ),
    ];
    for (px, py, name) in unsel {
        assert_eq!(
            b0[py as usize * w as usize + px as usize],
            row_bg(px, py),
            "{name}必须 = 渐变暗底（无框；任何框墨即红）"
        );
    }

    // ③上池值框无边框：左竖线位/右缘/上缘全 = 渐变暗底
    // （变异：左竖线/四边细框回潮即红）
    // 十四修动态宽度：几何吃实量宽（与涂装同一条 measure_items 尺）
    let ur = cfg_page::upper_row_rect(0, &ps.upper, 0);
    let lw = tv.text_width("默认服务器", 36.0);
    let vw = tv.text_width("本地终端", 30.0);
    let lb = cfg_page::field_label_rect(&ur, lw);
    let vb = cfg_page::field_value_rect(&ur, lb.w, vw, false);
    assert_eq!(
        vb.x + vb.w as i64,
        ur.x + ur.w as i64,
        "值框锚行右缘（十四修）"
    );
    let vb_pts = [
        (vb.x + 4, vb.y + vb.h as i64 / 2, "值框原左竖线位"),
        (vb.x + vb.w as i64 - 2, vb.y + vb.h as i64 / 2, "值框右缘"),
        (vb.x + vb.w as i64 / 2, vb.y + 1, "值框上缘"),
        (vb.x + vb.w as i64 / 2, vb.y + vb.h as i64 / 2, "值框内芯"),
    ];
    for (px, py, name) in vb_pts {
        assert_eq!(
            b0[py as usize * w as usize + px as usize],
            row_bg(px, py),
            "{name}必须 = 渐变暗底（无边框；任何框墨即红）"
        );
    }
    // ⑤标签列背衬（十三修 §五）：圆角 36 无边框背衬块（与值框同高
    // 对齐）= 渐变暗底 + 8% 白提亮（blend 白 α20）——无背衬/不提亮/
    // 提错量即红。取样避字形带（贴背衬顶条）
    let blend = |fg: u32, dst: u32, a: u32| {
        let inv = 255 - a;
        let ch = |f: u32, d: u32| (f * a + d * inv) / 255;
        (ch((fg >> 16) & 0xFF, (dst >> 16) & 0xFF) << 16)
            | (ch((fg >> 8) & 0xFF, (dst >> 8) & 0xFF) << 8)
            | ch(fg & 0xFF, dst & 0xFF)
    };
    let label_pts = [
        (ur.x + 40, vb.y + 10, "标签列背衬左顶"),
        (lb.x + lb.w as i64 - 40, vb.y + 10, "标签列背衬右顶"),
    ];
    for (px, py, name) in label_pts {
        assert_eq!(
            b0[py as usize * w as usize + px as usize],
            blend(0x00FF_FFFF, row_bg(px, py), 20),
            "{name}必须 = 渐变暗底 + 8% 白提亮精确值"
        );
        assert_ne!(
            b0[py as usize * w as usize + px as usize],
            row_bg(px, py),
            "{name}不提亮即红（变异：背衬回退纯暗底）"
        );
    }
    assert_ne!(
        b0[(vb.y + 10) as usize * w as usize + (ur.x + 40) as usize],
        blend(0x00FF_FFFF, row_bg(ur.x + 40, vb.y + 10), 40),
        "提亮量必须是 8%（α20；提错量即红）"
    );
    // ④暗底不是平色：同一行内芯左/右取样必须异色（渐变尺的铁证；
    // 变异：暗底写平色即红）
    let (lx, rx, my) = (r0.x + 40, r0.x + r0.w as i64 - 40, r0.y + r0.h as i64 / 2);
    assert_ne!(
        b0[my as usize * w as usize + lx as usize],
        b0[my as usize * w as usize + rx as usize],
        "暗底必须随 135° 尺渐变（平色变異即红）"
    );
    assert_eq!(
        b0[my as usize * w as usize + lx as usize],
        row_bg(lx, my),
        "暗底左取样 = 尺上精确值"
    );
    // 渐变色向钉：三级框与页环同向 c1→c2（同一采样函数同一分母——
    // 反转变异即全钉红，此处补一刀直证：左粗缘色 ≠ 反转采样）
    let (px, py) = (r0.x + 4, r0.y + r0.h as i64 / 2);
    assert_eq!(
        b0[py as usize * w as usize + px as usize],
        lerp_rgb(
            acc.c1,
            acc.c2,
            ((px + py) * 255 / denom).clamp(0, 255) as u32
        ),
        "左粗缘色向 = c1→c2 正转（与页环同向；反转变异即红）"
    );
    // bg 变量防未用告警（夹具底色仅注释用）
    let _ = bg;
}

#[test]
fn spec_字段行_右对齐与动态宽涂装钉() {
    // 十四修 §五：值文本逐行右对齐（末笔贴框右内缘 1.5 格），标签逐行
    // 左对齐（起笔 = 块左 + 1.5 格）；短值吃最小框宽时对齐方向才有
    // 像素级可观性（贴字框左右对齐不可辨——钉必须吃最小宽形态）。
    // 变异：值画回左对齐 / 标签画回右对齐即红
    use kfm_na::termview::frame_bg_rgb;
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "系统管理".into(),
        meta: String::new(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "key".into(),
        value: "v".into(),  // 短值 → 值框吃最小宽，右对齐可观
        is_dropdown: false, // （ASCII 夹具：host 测试字体无 CJK 字形）
    }]);
    let pg = page.snap(1000);
    let mut b0 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b0, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut b0, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut b0, w, h, &ps, &pg, 0, acc, 0, None, false);

    let ur = cfg_page::upper_row_rect(0, &ps.upper, 0);
    let lb = cfg_page::field_label_rect(&ur, tv.text_width("key", 36.0));
    let vb = cfg_page::field_value_rect(&ur, lb.w, tv.text_width("v", 30.0), false);
    assert_eq!(
        vb.w,
        cfg_page::FIELD_VALUE_MIN_W,
        "夹具前提：短值吃最小框宽"
    );
    let inset_g = cfg_page::FIELD_TEXT_INSET as i64;
    let ink_at = |x: i64, y: i64| {
        let p = b0[y as usize * w as usize + x as usize] & 0x00FF_FFFF;
        let bg = frame_bg_rgb(acc.c1, acc.c2, x, y, denom) & 0x00FF_FFFF;
        // 值灰/标签亮叠在渐变暗底上：任一通道差 >40 判墨
        let d = |a: u32, b: u32| a.abs_diff(b);
        d(p >> 16 & 0xFF, bg >> 16 & 0xFF) > 40
            || d(p >> 8 & 0xFF, bg >> 8 & 0xFF) > 40
            || d(p & 0xFF, bg & 0xFF) > 40
    };
    // 值带扫描（盒垂直中带避上下框缘）：找最左/最右墨位
    let (mut v_min, mut v_max) = (i64::MAX, i64::MIN);
    for y in vb.y + 20..vb.y + vb.h as i64 - 20 {
        for x in vb.x + 2..vb.x + vb.w as i64 - 2 {
            if ink_at(x, y) {
                v_min = v_min.min(x);
                v_max = v_max.max(x);
            }
        }
    }
    assert!(v_max > 0, "夹具前提：值文本必须落墨");
    assert!(
        v_max >= vb.x + vb.w as i64 - inset_g - 4,
        "值末笔必须贴右内缘 1.5 格（右对齐；实采 v_max={v_max} 右内缘={})",
        vb.x + vb.w as i64 - inset_g
    );
    assert!(
        v_min > vb.x + inset_g + 20,
        "短值左部必须留白（右对齐的铁证；左对齐变异即红；实采 v_min={v_min}）"
    );
    // 标签带：起笔贴块左 + 1.5 格
    let (mut l_min, mut l_max) = (i64::MAX, i64::MIN);
    for y in lb.y + 20..lb.y + lb.h as i64 - 20 {
        for x in lb.x + 2..lb.x + lb.w as i64 - 2 {
            if ink_at(x, y) {
                l_min = l_min.min(x);
                l_max = l_max.max(x);
            }
        }
    }
    assert!(l_max > 0, "夹具前提：标签必须落墨");
    assert!(
        l_min <= lb.x + inset_g + 4,
        "标签起笔必须贴块左内缘（左对齐；实采 l_min={l_min} 左内缘={})",
        lb.x + inset_g
    );
}

#[test]
fn spec_下拉面板_涂装钉() {
    // 宪法 §六 下拉栏（2026-09-14 十三修重订，用户拍板；2026-09-18
    // 二十四修面板拆层——BAR-096 模式第五层，页涂装不再画面板，层画布
    // 局部坐标 + 画布渐变尺，合成期 SRC_ALPHA 上屏）：
    // ①触发器 = 三级框全包框（左粗缘/三细边 = 渐变 α255 直出 + 渐变暗
    //   底芯——变异：画回无边框值框即红）——在页涂装，不进层；
    // ②展开面板 = 整面圆角无边框深底：层画布 α252 黑像素直写（GPU
    //   混合 ≡ blend(黑, 下层, 252)——blend_px 回潮 = 透明画布 rgb 恒
    //   0 被 mark_chrome_alpha 判透明，深底整面消失即红）；角外 =
    //   透明像素 0（圆角不吃直角即红）；
    // ③选项行 = 方形无个体背景（未选中行内 = 与面板同一块深底）；
    //   选中行 = 均匀细框（四边 3px 渐变 rgb 直出、α 随深底 252——
    //   blend_px 保 alpha 纪律 BAR-067 的层画布投影；芯渐变暗底不透明
    //   直写——变异：画回逐行圆角行框/无框即红）
    use kfm_na::termview::{frame_bg_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "系统管理".into(),
        meta: "1 项".into(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "默认服务器".into(),
        value: "本地终端".into(),
        is_dropdown: true,
    }]);
    page.set_options(vec!["本地终端".into(), "服务器".into()], 1);

    // before：合着画一遍（下层原样取证）
    let pg0 = page.snap(1000);
    let mut b0 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b0, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut b0, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut b0, w, h, &ps, &pg0, 0, acc, 0, None, false);

    // ①触发器 = 三级框全包框（十四修动态宽度：实量宽喂几何）
    let lw = tv.text_width("默认服务器", 36.0);
    let vw = tv.text_width("本地终端", 30.0);
    let vb = cfg_page::trigger_rect(&ps.upper, 0, true, lw, vw);
    let trig = [
        (vb.x + 4, vb.y + vb.h as i64 / 2, "触发器左粗缘"),
        (vb.x + vb.w as i64 / 2, vb.y + 1, "触发器顶边"),
        (vb.x + vb.w as i64 - 2, vb.y + vb.h as i64 / 2, "触发器右边"),
    ];
    for (px, py, name) in trig {
        assert_eq!(
            b0[py as usize * w as usize + px as usize],
            ring_gradient_rgb(acc.c1, acc.c2, px, py, denom),
            "{name}必须 = 渐变 α255 直出（三级框；无边框回潮即红）"
        );
    }
    assert_eq!(
        b0[(vb.y + vb.h as i64 - 8) as usize * w as usize + (vb.x + 40) as usize],
        frame_bg_rgb(acc.c1, acc.c2, vb.x + 40, vb.y + vb.h as i64 - 8, denom),
        "触发器内芯 = 渐变暗底不透明直出"
    );

    // after：展开——二十四修 §六② 面板拆层（BAR-096 模式第五层）：页涂装
    // 不再画面板，面板走独立槽画布（合成期 SRC_ALPHA 混合上屏）。层画布
    // 局部坐标：原点 = (上池内容左内缘, 面板顶 pr.y)，渐变尺 = 画布尺——
    // 与 android_app 烘焙块同尺复算（眼手同尺）
    page.toggle_dropdown(1000);
    let pg1 = page.snap(1300);
    assert!(pg1.dropdown_open, "夹具前提：展开态");
    // 拆层钉：页缓冲面板区必须 = 合着时原样（页涂装画面板回潮即红）
    let mut b1 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b1, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut b1, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut b1, w, h, &ps, &pg1, 0, acc, 0, None, false);

    let t = cfg_page::trigger_rect(&ps.upper, 0, true, lw, vw);
    let max_h = h.saturating_sub(t.y.max(0) as u32 + t.h + 40);
    // 十七修 BAR-090：几何复算吃与实现同尺的内容最小宽（眼手同尺）
    let cw0 = tv.text_width("本地终端", 36.0) + cfg_page::FIELD_TEXT_INSET * 2;
    let pr = cfg_page::dropdown_panel_rect(2, &ps.upper, max_h, 0, true, lw, vw, cw0);
    let row_h = cfg_page::FIELD_ROW_H as i64;
    let canvas_w = ps.upper.w - (cfg_page::POOL_CONTENT_INSET * 2) as u32;
    let ox = ps.upper.x + cfg_page::POOL_CONTENT_INSET;
    let rx = pr.x - ox; // 面板左缘的画布局部 x
    let dnom = (i64::from(canvas_w) - 1) + (i64::from(pr.h) - 1);
    let mut cv = vec![0u32; (canvas_w * pr.h) as usize];
    tv.paint_dropdown_panel_layer(&mut cv, canvas_w, pr.h, &pg1, &pr, (ox, pr.y), acc);
    let at = |lx: i64, ly: i64| cv[(ly * i64::from(canvas_w) + lx) as usize];

    // 拆层钉：展开后页缓冲面板芯区 = 合着时原样（面板不进页涂装）
    let (px_in, py_in) = (rx + pr.w as i64 - 30, row_h / 2);
    assert_eq!(
        b1[(pr.y + py_in) as usize * w as usize + (ox + px_in) as usize],
        b0[(pr.y + py_in) as usize * w as usize + (ox + px_in) as usize],
        "面板拆层：页涂装画面板回潮即红（并发同拍期会被零拷贝捕获冻进旧代）"
    );
    // ②面板深底：层画布 = α252 黑像素直写（GPU SRC_ALPHA 混合 ≡
    // blend(黑, 下层, 252)——blend_px 回潮即红：透明画布上 rgb 恒 0，
    // mark_chrome_alpha 判纯黑透明 = 深底整面消失）
    assert_eq!(
        at(rx + pr.w as i64 - 30, row_h / 2),
        252 << 24,
        "面板深底 = α252 黑像素直写（blend_px 回潮 = 0 全透明即红）"
    );
    // ②角外 = 透明（合成后下层原样；圆角不吃直角即红）
    assert_eq!(
        at(rx + 1, 1),
        0,
        "面板左上圆角外必须 = 透明（直角化/方角深底即红）"
    );
    // ③选中行（option_sel = 1）= 均匀细框：边 = 渐变 rgb 直出（α 随深底
    // 252——blend_px 保 alpha 纪律 BAR-067 在层画布的投影）；芯 = 渐变
    // 暗底不透明直写（≠ 面板深底）
    let (fx, fy) = (rx + 1, row_h + row_h / 2);
    assert_eq!(
        at(fx, fy),
        (252 << 24) | ring_gradient_rgb(acc.c1, acc.c2, fx, fy, dnom),
        "选中行细框边 = 渐变 rgb 直出（无框/行框回潮即红）"
    );
    let (ix2, iy2) = (rx + pr.w as i64 - 30, row_h + row_h / 2);
    assert_eq!(
        at(ix2, iy2),
        frame_bg_rgb(acc.c1, acc.c2, ix2, iy2, dnom),
        "选中行芯 = 渐变暗底不透明直写（≠ 深底；逐行圆角行框回潮即红）"
    );
    // ③未选中行无个体背景：行 0 芯 = 纯深底（行内另取一点钉「处处深底」）
    assert_eq!(
        at(rx + 60, row_h - 8),
        252 << 24,
        "未选中行 = 纯深底无个体背景（逐行圆角回潮即红）"
    );
}

#[test]
fn spec_cfg双池_涂装钉() {
    // 宪法 §五（2026-09-12 骨架；同日实测二标：池间距 1 格/左右各 2 格
    // 内边距/空占位 4 格/圆角 = 卡片框 36 与光标分家；2026-09-14 十二修：
    // 内芯渐变暗底）：①两枚二级卡框
    // 都有环墨；②内卡渐变反转 c2→c1（§三）逐像素判，正转变异即红；
    // ③内芯 = 反转渐变暗底（平填回潮即红）；④两池 1 格间距——上池底缘墨
    // 与下池顶缘墨之间是页暗底行；⑤底内缘含 inset（下池不顶穿页环底）；
    // ⑥accent 驱动
    use kfm_na::termview::{POOL_FRAME_R, lerp_rgb};
    use kfm_na::ui::dual_pool::{DualPool, POOL_EMPTY_H, POOL_GAP};
    let (w, h) = (400u32, 700u32);
    let inset = 120u32;
    let acc_a = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let acc_b = kfm_na::ui::accent::AccentPair {
        c1: 0x0000_FF00,
        c2: 0x00FF_00FF,
    };
    let bg = kfm_na::ui::accent::CARD_PAGE_BG;
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(400, 700);
    pool.set_viewport(400, 700, inset); // 与页环 chrome 同 inset（顶穿钉前提）
    let snap = pool.layout(1000);
    // 骨架期空占位：upper = (61,163,284,144)，lower = (61,343,284,182)
    assert_eq!(snap.upper.h, POOL_EMPTY_H, "夹具前提：骨架期上池空占位");
    assert_eq!(snap.upper.x, 61, "夹具前提：左右各 2 格内边距");
    assert_eq!(POOL_FRAME_R, 36, "池框圆角 = 卡片框 36（光标 16 分家）");

    let mut b0 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b0, w, h, inset, 0, acc_a);
    tv.paint_cfg_dual_pool(&mut b0, w, h, &snap, 0, acc_a);

    // ①+②上池左缘中带（满覆盖）：逐像素 = lerp(c2→c1, t)——反转钉
    let ux = snap.upper.x as usize;
    let uy = snap.upper.y as usize;
    let uw = snap.upper.w as usize;
    let uh = snap.upper.h as usize;
    let t_up = ((2 + uh / 2) * 255 / ((uw - 1) + (uh - 1))) as u32;
    let up_ring = b0[(uy + uh / 2) * w as usize + ux + 2];
    assert_eq!(
        up_ring,
        lerp_rgb(acc_a.c2, acc_a.c1, t_up.min(255)),
        "上池环墨 = c2→c1 反转渐变（§三 内卡反转；正转变异即红）"
    );
    assert_ne!(
        up_ring,
        lerp_rgb(acc_a.c1, acc_a.c2, t_up.min(255)),
        "与正转色必须不同（反转不是口号）"
    );
    // 下池左缘中带同规
    let ly2 = snap.lower.y as usize;
    let lh2 = snap.lower.h as usize;
    let t_lo = ((2 + lh2 / 2) * 255 / ((uw - 1) + (lh2 - 1))) as u32;
    assert_eq!(
        b0[(ly2 + lh2 / 2) * w as usize + ux + 2],
        lerp_rgb(acc_a.c2, acc_a.c1, t_lo.min(255)),
        "下池环墨 = 同一份反转配方（样式唯一来源）"
    );

    // ③内芯 = 渐变暗底（十二修 §三：池框内芯 = dark(c2)→dark(c1) 反转
    // 暗渐变，池框本地尺 原点(61,163) denom 426——平填回底回潮即红）
    let dark = |c: u32| lerp_rgb(c, 0, 200);
    assert_eq!(
        b0[(uy + uh / 2) * w as usize + ux + 40],
        kfm_na::termview::ring_gradient_rgb(dark(acc_a.c2), dark(acc_a.c1), 40, 72, 426),
        "上池内芯必须 = 反转渐变暗底精确值（十二修）"
    );

    // ④池间距：上池底缘墨（uy+uh−1）与下池顶缘墨（ly2）之间是净底行
    assert_ne!(
        b0[(uy + uh - 1) * w as usize + ux + 100],
        bg,
        "上池底缘必须有墨"
    );
    assert_ne!(b0[ly2 * w as usize + ux + 100], bg, "下池顶缘必须有墨");
    assert_eq!(uy + uh + POOL_GAP as usize, ly2, "两池间距 = 1 格");
    let gap_mid = uy + uh + POOL_GAP as usize / 2;
    assert_eq!(
        b0[gap_mid * w as usize + ux + 100],
        kfm_na::termview::ring_gradient_rgb(
            dark(acc_a.c1),
            dark(acc_a.c2),
            145,
            gap_mid as i64 - 16,
            914
        ),
        "间距中带 = 页暗底（页尺原点(16,16) denom 914；两环辉光够不到中行）"
    );

    // ⑤顶穿钉：下池底缘 = 页环底内缘之上（底内缘 = h − inset − 55）
    let ring_bottom_inner = h as usize - inset as usize - 55;
    assert_eq!(
        ly2 + lh2,
        ring_bottom_inner,
        "下池底必须贴页环底内缘（含 inset）"
    );

    // ⑥accent 驱动：换 accent 同位必变色
    let mut b1 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b1, w, h, inset, 0, acc_b);
    tv.paint_cfg_dual_pool(&mut b1, w, h, &snap, 0, acc_b);
    assert_ne!(
        b1[(uy + uh / 2) * w as usize + ux + 2],
        up_ring,
        "accent 换了池框色必须变（驱动钉）"
    );
}

// ---- 十五修：动效引擎预览 = 乒乓语义化动画（相位表单一源 src/ui/fx_preview.rs）----

/// 动画预览钉共用夹具：开指定 preview 的跳框，喂 now_ms 画一帧
#[allow(clippy::too_many_arguments)]
fn paint_modal_frame(
    pv_match: kfm_na::ui::comp_registry::Preview,
    now_ms: u64,
) -> (Vec<u32>, u32, u32, kfm_na::ui::dual_pool::PoolRect) {
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::comp_registry as cr;
    use kfm_na::ui::dual_pool::DualPool;
    use kfm_na::ui::modal as md;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let mi = cr::COMPONENTS
        .iter()
        .position(|e| e.preview == pv_match)
        .expect("组件表必须有该 preview 的条目");
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "动效引擎".into(),
        meta: "4 件".into(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "弹簧".into(),
        value: "现役".into(),
        is_dropdown: false,
    }]);
    page.set_tab(
        1,
        1000,
        ps.clone(),
        kfm_na::ui::accent::AccentPair { c1: 0, c2: 0 },
    ); // 组件池页
    page.open_modal(mi);
    let pg = page.snap(1000);
    assert_eq!(pg.modal, Some(mi), "夹具前提：跳框开着");

    let mut buf = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut buf, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut buf, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut buf, w, h, &ps, &pg, 0, acc, now_ms, None, false);

    // 展台内区（与 paint_preview_impl 同一份几何：内缩 1 格/半格）
    let entry = &cr::COMPONENTS[mi];
    let card = md::card_rect(w, h, &md::fields_of(entry, md::content_cells(w)));
    let prev = md::preview_rect(&card);
    (buf, w, h, prev)
}

/// 统计矩形内「近白」像素数（白球 α220 叠暗底 ≈ ≥220；渐变 accent
/// 夹具色 FF6000/0080FF 全通道不过 180——球在/不在的判别尺）
fn near_white_count(buf: &[u32], w: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> usize {
    let h = (buf.len() as i64) / i64::from(w);
    let mut n = 0;
    for y in y0.max(0)..y1.min(h) {
        for x in x0.max(0)..x1.min(i64::from(w)) {
            let p = buf[y as usize * w as usize + x as usize] & 0x00FF_FFFF;
            let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
            if r > 180 && g > 180 && b > 180 {
                n += 1;
            }
        }
    }
    n
}

/// 统计矩形内「亮墨」像素数（max 通道 > 100：环带/满填/文字亮墨；
/// 渐变暗底 FRAME_BG_DIM≈22%（max ≤56）不过线——环在/不在判别尺）
fn bright_ink_count(buf: &[u32], w: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> usize {
    let h = (buf.len() as i64) / i64::from(w);
    let mut n = 0;
    for y in y0.max(0)..y1.min(h) {
        for x in x0.max(0)..x1.min(i64::from(w)) {
            let p = buf[y as usize * w as usize + x as usize] & 0x00FF_FFFF;
            let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
            if r.max(g).max(b) > 100 {
                n += 1;
            }
        }
    }
    n
}

/// 展台内两帧异像素数（静态化变异判别尺：动画臂忽略 now_ms 即 0）
fn booth_diff(a: &[u32], b: &[u32], w: u32, prev: &kfm_na::ui::dual_pool::PoolRect) -> usize {
    let mut n = 0;
    for y in prev.y..prev.y + i64::from(prev.h) {
        for x in prev.x..prev.x + i64::from(prev.w) {
            if a[y as usize * w as usize + x as usize] != b[y as usize * w as usize + x as usize] {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn spec_动效预览_弹簧_动画钉() {
    // 十五修相位表（fx_preview 乒乓 1400ms）：点球在曲线原点腿首点触
    // （0..80 淡入/80..160 满/160..300 淡出）；响应点 r8 沿 spring_pos
    // 实曲线往返骑行（Go: 0→100；Return: 100→0）；停靠段钉死
    use kfm_na::ui::comp_registry::Preview;
    let (b_ball, w, _h, prev) = paint_modal_frame(Preview::CurveSpring, 100);
    let (b_mid, _, _, _) = paint_modal_frame(Preview::CurveSpring, 175);
    let (b_end, _, _, _) = paint_modal_frame(Preview::CurveSpring, 400);
    let (b_wrap, _, _, _) = paint_modal_frame(Preview::CurveSpring, 175 + 1400);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let ih = prev.h - CELL_H;
    let origin = (ix, iy + i64::from(ih));

    // ①点球在：t=100（按住窗）原点 ±15 内近白 ≥80（r13 球 ≈530px 盘面）
    let n_ball = near_white_count(
        &b_ball,
        w,
        origin.0 - 15,
        origin.1 - 15,
        origin.0 + 15,
        origin.1 + 15,
    );
    assert!(n_ball >= 80, "t=100 白球必须在曲线原点（实采 {n_ball}）");
    // ②球走：t=400（终点停靠，骑行点在曲线终点）原点窗必须归零
    let n_gone = near_white_count(
        &b_end,
        w,
        origin.0 - 15,
        origin.1 - 15,
        origin.0 + 15,
        origin.1 + 15,
    );
    assert_eq!(n_gone, 0, "t=400 原点不许再有白球（实采 {n_gone}）");
    // ③骑行实跑：去程中 vs 终点停靠两帧必异（静态化变异即红）
    assert!(
        booth_diff(&b_mid, &b_end, w, &prev) > 0,
        "去程中帧与终点停靠帧必须异（响应点骑行）"
    );
    // ④回卷无缝：t 与 t+1400 逐像素全等（相位表不取模即红）
    assert_eq!(
        booth_diff(&b_mid, &b_wrap, w, &prev),
        0,
        "回卷必须无缝（t 与 t+1400 全等）"
    );
}

#[test]
fn spec_动效预览_缓动_动画钉() {
    // 相位表：点球在小面板顶心腿首点触；Go = power2_out 下落（跨度
    // = ih−54）；终点停靠钉死；Return = rise_release 收起
    use kfm_na::ui::comp_registry::Preview;
    let (b_ball, w, _h, prev) = paint_modal_frame(Preview::CurveEase, 100);
    let (b_mid, _, _, _) = paint_modal_frame(Preview::CurveEase, 175);
    let (b_end, _, _, _) = paint_modal_frame(Preview::CurveEase, 400);
    let (b_start, _, _, _) = paint_modal_frame(Preview::CurveEase, 1000);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let panel_cx = ix + i64::from(iw) / 6 + i64::from(iw) / 3;
    // ①t=100：点球按住窗，球骑在面板顶心（面板已随 Go 腿落了一段——
    // 点触即走语义；球位 = iy + power2_out(100/350)·(ih−54) + 27，
    // fx_ease 独立复算；画错位置/没跟面板即红）
    let py100 =
        iy + (kfm_na::ui::fx_ease::power2_out(100.0 / 350.0) * (i64::from(ih) - 54) as f32) as i64;
    let n_ball = near_white_count(
        &b_ball,
        w,
        panel_cx - 15,
        py100 + 12,
        panel_cx + 15,
        py100 + 42,
    );
    assert!(n_ball >= 80, "t=100 白球必须骑在小面板顶（实采 {n_ball}）");
    // ②缓动实跑：下落中 vs 终点停靠必异
    assert!(
        booth_diff(&b_mid, &b_end, w, &prev) > 0,
        "下落中帧与停底帧必须异（缓动实跑；静态化即红）"
    );
    // ③停底钉：t=400 面板落到底（顶缘 = iy+ih−54）——底带亮环墨必须
    // 多过 t=1000（面板在顶时底带只剩静态曲线墨）
    let band = |buf: &[u32]| {
        bright_ink_count(
            buf,
            w,
            ix + i64::from(iw) / 6 + 20,
            iy + i64::from(ih) - 53,
            ix + i64::from(iw) / 6 + i64::from(iw) * 2 / 3 - 20,
            iy + i64::from(ih) - 50,
        )
    };
    assert!(
        band(&b_end) > band(&b_start) + 20,
        "t=400 面板必须落到底（亮环墨 {} vs {}）",
        band(&b_end),
        band(&b_start)
    );
}

#[test]
fn spec_动效预览_手势仲裁_动画钉() {
    // 相位表：拖球全程跟手（0..80 淡入/80..350 满/350..500 淡出）；
    // Go rt<0.66 = 1:1 拖到轨道 70%，rt≥0.66 = 松手 power2_out 补到
    // 终点；Return 纯 1:1 拖回起点
    use kfm_na::ui::comp_registry::Preview;
    let (b_a, w, _h, prev) = paint_modal_frame(Preview::Swipe, 250);
    let (b_b, _, _, _) = paint_modal_frame(Preview::Swipe, 700);
    let (b_c, _, _, _) = paint_modal_frame(Preview::Swipe, 1100);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let my = iy + i64::from(ih) / 2;
    let x0 = ix + 30;
    let x1 = ix + i64::from(iw) - 60;
    let release_x = x0 + (x1 - x0) * 7 / 10;
    // ①t=250：Go rt=250/350≈0.714 ≥ 0.66 → 松手补间段——球位必须 =
    // 相位表精确值（fx_ease::power2_out 独立复算；位置钉：画错位置即红）
    let rt = 250.0f32 / 350.0;
    let bx250 = release_x
        + (kfm_na::ui::fx_ease::power2_out((rt - 0.66) / 0.34) * (x1 - release_x) as f32) as i64;
    let n_ball = near_white_count(&b_a, w, bx250 - 15, my - 15, bx250 + 15, my + 15);
    assert!(
        n_ball >= 60,
        "t=250 白球必须在相位表位 (bx={bx250})（实采 {n_ball}）"
    );
    // ②回程拖动中（t=700）vs 起点停靠（t=1100）：两帧必异（拖回实跑）
    assert!(
        booth_diff(&b_b, &b_c, w, &prev) > 0,
        "回程拖动帧与起点停靠帧必须异"
    );
    // ③t=1100：球已退场——起点窗内不许有白（卡片细框/起点圆是 accent
    // 色不过 180 线）
    let n_gone = near_white_count(&b_c, w, x0 - 15, my - 15, x0 + 15, my + 15);
    assert_eq!(n_gone, 0, "t=1100 起点不许再有白球（实采 {n_gone}）");
}

#[test]
fn spec_动效预览_视口平移_动画钉() {
    // 十五修·满宽推入：新页宽 = 展台内宽 iw，从 ix+iw 推到 ix；旧页从
    // ix 推到 ix−iw 完全挤出（真换页）；拖球贴新页左缘
    use kfm_na::ui::comp_registry::Preview;
    let (b_mid, w, _h, prev) = paint_modal_frame(Preview::ViewportPush, 175);
    let (b_end, _, _, _) = paint_modal_frame(Preview::ViewportPush, 400);
    let (b_start, _, _, _) = paint_modal_frame(Preview::ViewportPush, 1000);
    let (b_wrap, _, _, _) = paint_modal_frame(Preview::ViewportPush, 1000 + 1400);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let ph = i64::from(ih) * 3 / 4;
    let py0 = iy + (i64::from(ih) - ph) / 2;
    // ①推入实跑：拖动中 vs 起点停靠必异
    assert!(
        booth_diff(&b_mid, &b_start, w, &prev) > 0,
        "拖动中帧与起点停靠帧必须异（推入实跑；静态化即红）"
    );
    // ②回卷无缝
    assert_eq!(
        booth_diff(&b_start, &b_wrap, w, &prev),
        0,
        "回卷必须无缝（t 与 t+1400 全等）"
    );
    // ③满宽语义钉：展台 3/4 宽近底缘采样点 q（渐变 t≈0.8，橙蓝分离
    // 度最大处）——t=1000（起点）= 旧灰页底环（R≈B）；t=400（终点）=
    // 新 accent 页底环（橙侧 R>B+40）。旧页没完全挤出/新页没满宽靠泊
    // 即红
    let qx = ix + i64::from(iw) * 3 / 4;
    let qy = py0 + ph - 2;
    let split = |buf: &[u32]| {
        let p = buf[qy as usize * w as usize + qx as usize] & 0x00FF_FFFF;
        ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF)
    };
    let (r0, _, b0) = split(&b_start);
    let (r1, _, b1) = split(&b_end);
    assert!(
        r0.abs_diff(b0) <= 20,
        "t=1000 q 必须是旧灰页底环（实采 R={r0} B={b0}）"
    );
    assert!(
        r1 > b1 + 40,
        "t=400 q 必须是新 accent 页底环（实采 R={r1} B={b1}）"
    );
}

#[test]
fn spec_动效预览_池高伸缩_动画钉() {
    // 语义化（十五修）：上池高 uh = ih·(0.25+0.30·p) 乒乓（ease-in-out
    // 唯一尺）；下池顶 = iy+uh+18 跟随；点球在下池首行腿首点触
    use kfm_na::ui::comp_registry::Preview;
    let (b_mid, w, _h, prev) = paint_modal_frame(Preview::PoolGlide, 175);
    let (b_end, _, _, _) = paint_modal_frame(Preview::PoolGlide, 400);
    let (b_start, _, _, _) = paint_modal_frame(Preview::PoolGlide, 1000);
    let (b_ball, _, _, _) = paint_modal_frame(Preview::PoolGlide, 100);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    // ①伸缩实跑
    assert!(
        booth_diff(&b_mid, &b_start, w, &prev) > 0,
        "上池高必须实跑（静态化即红）"
    );
    // ②语义钉：t=1000（p=0，uh=0.25·ih）上池底环带 = y≈iy+0.25ih；
    // t=400（p=1，uh=0.55ih）同带已在上池内芯（暗底不过 100 线）——
    // 亮环墨必须前者多过后者（uh 区间锚错即红）
    let band_y = iy + i64::from(ih) / 4;
    let band = |buf: &[u32]| {
        bright_ink_count(
            buf,
            w,
            ix + i64::from(iw) / 2 - 40,
            band_y - 2,
            ix + i64::from(iw) / 2 + 40,
            band_y + 2,
        )
    };
    assert!(
        band(&b_start) > band(&b_end) + 10,
        "上池底环必须在 0.25ih 带（{} vs {}）",
        band(&b_start),
        band(&b_end)
    );
    // ③点球：t=100 球在下池首行（ly0+22，ly0 = iy+0.25ih+18）
    let ly0 = iy + i64::from(ih) / 4 + 18;
    let n_ball = near_white_count(
        &b_ball,
        w,
        ix + i64::from(iw) / 2 - 15,
        ly0 + 7,
        ix + i64::from(iw) / 2 + 15,
        ly0 + 37,
    );
    assert!(n_ball >= 60, "t=100 白球必须点在下池首行（实采 {n_ball}）");
}

#[test]
fn spec_动效预览_标签栏层_动画钉() {
    // 语义化（十五修）：两未选 chip（薄态 α48）+ 选中块（满态 α255
    // 均匀渐变）在两者间乒乓滑行；底线纯色；点球在目标 chip 心
    use kfm_na::ui::comp_registry::Preview;
    let (b_end, w, _h, prev) = paint_modal_frame(Preview::TabSlide, 400);
    let (b_start, _, _, _) = paint_modal_frame(Preview::TabSlide, 1000);
    let (b_mid, _, _, _) = paint_modal_frame(Preview::TabSlide, 175);
    let (b_ball, _, _, _) = paint_modal_frame(Preview::TabSlide, 100);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let chip_w = i64::from(iw) * 2 / 5;
    let chip_h = (i64::from(ih) / 2).clamp(40, 118);
    let y0 = iy + (i64::from(ih) - chip_h) / 2 - 6;
    let ax = ix + i64::from(iw) / 12;
    let bx = ix + i64::from(iw) - i64::from(iw) / 12 - chip_w;
    let chip_bright = |buf: &[u32], cx: i64| {
        bright_ink_count(buf, w, cx + 8, y0 + 8, cx + chip_w - 8, y0 + chip_h - 4)
    };
    // ①终点（t=400）选中块在右 chip：右亮墨 ≫ 左；起点（t=1000）反之
    // （薄态 α48 不过 100 线，满态 α255 必过——选中块位置锚错即红）
    assert!(
        chip_bright(&b_end, bx) > chip_bright(&b_end, ax) + 100,
        "终点选中块必须在右 chip（{} vs {}）",
        chip_bright(&b_end, bx),
        chip_bright(&b_end, ax)
    );
    assert!(
        chip_bright(&b_start, ax) > chip_bright(&b_start, bx) + 100,
        "起点选中块必须在左 chip（{} vs {}）",
        chip_bright(&b_start, ax),
        chip_bright(&b_start, bx)
    );
    // ②滑行实跑
    assert!(
        booth_diff(&b_mid, &b_start, w, &prev) > 0,
        "选中块滑行必须实跑"
    );
    // ③点球：t=100 球在右 chip 心（Go 目标）
    let n_ball = near_white_count(
        &b_ball,
        w,
        bx + chip_w / 2 - 15,
        y0 + chip_h / 2 - 15,
        bx + chip_w / 2 + 15,
        y0 + chip_h / 2 + 15,
    );
    assert!(n_ball >= 60, "t=100 白球必须点在右 chip（实采 {n_ball}）");
}

#[test]
fn spec_动效预览_光标滑行_框动字不动钉() {
    // 语义化（十五修）：三行真文字钉死 + 光标框行间乒乓滑行——
    // **框动字不动**（BAR-107 语义钉：文字像素不随光标框动；变异：
    // 文字烤进光标层/随框偏移重画即红）
    use kfm_na::ui::comp_registry::Preview;
    let (b_mid, w, _h, prev) = paint_modal_frame(Preview::CursorSlide, 175);
    let (b_start, _, _, _) = paint_modal_frame(Preview::CursorSlide, 1000);
    let (b_ball, _, _, _) = paint_modal_frame(Preview::CursorSlide, 100);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let rh = CELL_H * 2;
    let rgap = CELL_H / 2;
    let y0 = iy + (i64::from(ih) - i64::from(rh * 3 + rgap * 2)) / 2;
    // ①框动字不动：t=175（p=0.5，光标框恰好盖住中行）vs t=1000（框在
    // 首行）——中行文字带必须逐像素全等（带避开框环：x 让 30、y 让 10）
    let row1_y = y0 + i64::from(rh + rgap);
    let mut text_diff = 0usize;
    for y in row1_y + 10..row1_y + i64::from(rh) - 10 {
        for x in ix + 30..ix + 200 {
            if b_mid[y as usize * w as usize + x as usize]
                != b_start[y as usize * w as usize + x as usize]
            {
                text_diff += 1;
            }
        }
    }
    assert_eq!(text_diff, 0, "中行文字带必须逐像素钉死（框动字不动）");
    // ②框动：两帧展台必异（光标框滑行实跑）
    assert!(
        booth_diff(&b_mid, &b_start, w, &prev) > 0,
        "光标框滑行必须实跑"
    );
    // ③点球：t=100 球在行三心（Go 目标 = 行三）
    let row3_cy = y0 + i64::from(rh + rgap) * 2 + i64::from(rh) / 2;
    let n_ball = near_white_count(
        &b_ball,
        w,
        ix + i64::from(iw) / 2 - 15,
        row3_cy - 15,
        ix + i64::from(iw) / 2 + 15,
        row3_cy + 15,
    );
    assert!(n_ball >= 60, "t=100 白球必须点在行三（实采 {n_ball}）");
}

#[test]
fn spec_动效预览_下拉开合_抽屉随面钉() {
    // 语义化（十五修）：抽屉面板高 = p·满高；选项行钉死面板顶、Y clip
    // 到面板底（抽屉随面——下方先入场、上方先没入）；▼三角旋转
    // p×180°；点球在触发器
    use kfm_na::ui::comp_registry::Preview;
    let (b_mid, w, _h, prev) = paint_modal_frame(Preview::DropdownAnim, 175);
    let (b_open, _, _, _) = paint_modal_frame(Preview::DropdownAnim, 400);
    let (b_shut, _, _, _) = paint_modal_frame(Preview::DropdownAnim, 1000);
    let (b_ball, _, _, _) = paint_modal_frame(Preview::DropdownAnim, 100);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let tw = i64::from(iw) * 2 / 3;
    let th = 54i64;
    let tx = ix + (i64::from(iw) - tw) / 2;
    let full_ph = i64::from(ih) - th - 12;
    let py = iy + th + 12;
    // ①开合实跑：全开 vs 全收 面板区亮墨差
    let region =
        |buf: &[u32]| bright_ink_count(buf, w, tx + 10, py + 20, tx + tw - 10, py + full_ph - 10);
    assert!(
        region(&b_open) > region(&b_shut) + 200,
        "全开面板区必须有墨（{} vs {}）",
        region(&b_open),
        region(&b_shut)
    );
    // ②抽屉随面：t=175（p=0.5）面板底缘+外发光晕带（spread 14）以下
    // 必须与全收帧逐像素全等（选项行不许漏出抽屉底；clip 摘掉/行不钉
    // 面板顶即红）
    let clip_bottom = py + full_ph / 2;
    let mut leak = 0usize;
    for y in clip_bottom + 16..py + full_ph - 2 {
        for x in tx..tx + tw {
            if b_mid[y as usize * w as usize + x as usize]
                != b_shut[y as usize * w as usize + x as usize]
            {
                leak += 1;
            }
        }
    }
    assert_eq!(leak, 0, "抽屉底以下不许漏墨（实采 {leak}）");
    // ③三角旋转：全开 vs 全收 三角区必异（180° 翻转；不转即红）
    let (tcx, tcy) = (tx + tw - 30, iy + th / 2);
    let mut tri = 0usize;
    for y in tcy - 12..tcy + 12 {
        for x in tcx - 12..tcx + 12 {
            if b_open[y as usize * w as usize + x as usize]
                != b_shut[y as usize * w as usize + x as usize]
            {
                tri += 1;
            }
        }
    }
    assert!(tri > 10, "▼三角必须旋转（实采 {tri}）");
    // ④点球：t=100 球在触发器心偏左（三角位右侧不受球污染）
    let n_ball = near_white_count(
        &b_ball,
        w,
        tx + tw / 2 - 35,
        iy + th / 2 - 15,
        tx + tw / 2 - 5,
        iy + th / 2 + 15,
    );
    assert!(n_ball >= 60, "t=100 白球必须点在触发器（实采 {n_ball}）");
}

#[test]
fn spec_动效预览_视口平移切页_动画钉() {
    // 语义化（十五修）：两迷你页（各含双小池）面与内容一体横移换页；
    // 页宽 7/10 iw、stride = iw（B 起点 = ix+iw 展台右缘外）；
    // 点球在展台顶（= 点标签触发切页）
    use kfm_na::ui::comp_registry::Preview;
    let (b_mid, w, _h, prev) = paint_modal_frame(Preview::PagePan, 175);
    let (b_end, _, _, _) = paint_modal_frame(Preview::PagePan, 400);
    let (b_start, _, _, _) = paint_modal_frame(Preview::PagePan, 1000);
    let (b_wrap, _, _, _) = paint_modal_frame(Preview::PagePan, 2400);
    let (b_ball, _, _, _) = paint_modal_frame(Preview::PagePan, 100);

    let (ix, iy) = (prev.x + CELL_W as i64, prev.y + (CELL_H / 2) as i64);
    let (iw, ih) = (prev.w - CELL_W * 2, prev.h - CELL_H);
    let page_h = i64::from(ih) * 4 / 5;
    let py0 = iy + (i64::from(ih) - page_h) / 2;
    // ①换页实跑
    assert!(
        booth_diff(&b_mid, &b_start, w, &prev) > 0,
        "换页中帧与起点停靠帧必须异"
    );
    // ②回卷无缝
    assert_eq!(
        booth_diff(&b_start, &b_wrap, w, &prev),
        0,
        "回卷必须无缝（t 与 t+1400 全等）"
    );
    // ③靠泊身份钉：页环顶直段采样（ix+30, py0+1）渐变 t≈0 处 ≈ 纯
    // c1——A 页 c1=accent.c1（FF6000 橙，R>B+100）；B 页 c1=accent.c2
    // （0080FF 蓝，B>R+100）。t=1000 靠泊 A、t=400 靠泊 B（页序/双色
    // 锚错即红）
    let split = |buf: &[u32]| {
        let p = buf[(py0 + 1) as usize * w as usize + (ix + 30) as usize] & 0x00FF_FFFF;
        ((p >> 16) & 0xFF, (p >> 8) & 0xFF)
    };
    let (ra, ba) = split(&b_start);
    let (rb, bb) = split(&b_end);
    assert!(
        ra > ba + 100,
        "t=1000 必须靠泊 A 页（橙顶；实采 R={ra} B={ba}）"
    );
    assert!(
        bb > rb + 100,
        "t=400 必须靠泊 B 页（蓝顶；实采 R={rb} B={bb}）"
    );
    // ④点球：t=100 球在展台顶（ix+3iw/4, iy+8）
    let n_ball = near_white_count(
        &b_ball,
        w,
        ix + i64::from(iw) * 3 / 4 - 15,
        iy - 7,
        ix + i64::from(iw) * 3 / 4 + 15,
        iy + 23,
    );
    assert!(n_ball >= 60, "t=100 白球必须点在展台顶（实采 {n_ball}）");
}

#[test]
fn spec_bar088_恰好满宽_末字落墨钉() {
    // BAR-088：draw_items_left_inset 右缘判停 `>=` 误杀恰好满宽的末字——
    // 像素字体（CJK）步进整数和 == 内宽时，字段行集体少一字（redroid
    // 实机实证：弹簧→弹/手势仲裁→手势仲/现役→现）。host DejaVu 步进
    // 非整数永远打不中此边界（这正是考题全绿行为全错的漏网口），故
    // 手搓整数步进 items（排笔只吃 item.2 步进，与字形无关）复现：
    // 两字符各 10px 步进、内宽 20 = 恰好满宽，末字必须落墨。
    // 变异：判停回 >= 即红（右半带断墨）。
    let tv = host_termview(8, 2);
    let font = host_font();
    let items: [(&fontdue::Font, char, f32); 2] = [(&font, 'k', 10.0), (&font, 'e', 10.0)];
    let (w, h) = (64u32, 64u32);
    let ink_in = |buf: &[u32], x0: u32, x1: u32| {
        (x0..x1)
            .flat_map(|x| (0..h).map(move |y| (x, y)))
            .any(|(x, y)| buf[(y * w + x) as usize] & 0x00FF_FFFF != 0)
    };
    // 恰好满宽：两字都必须落墨（末字在右半带）
    let mut b0 = vec![0u32; (w * h) as usize];
    tv.spec_draw_items_left(&mut b0, w, h, &items, 0, 20, 0, 64, 14.0, 0x00FF_FFFF);
    assert!(ink_in(&b0, 0, 10), "首字带必须有墨（夹具前提）");
    assert!(
        ink_in(&b0, 10, 20),
        "恰好满宽末字必须落墨（BAR-088：>= 判停误杀即红）"
    );
    // 真装不下对照：内宽 15 < 两字步进和 20 → 末字不许起笔
    let mut b1 = vec![0u32; (w * h) as usize];
    tv.spec_draw_items_left(&mut b1, w, h, &items, 0, 15, 0, 64, 14.0, 0x00FF_FFFF);
    assert!(ink_in(&b1, 0, 10), "对照组首字带必须有墨");
    assert!(
        !ink_in(&b1, 15, 20),
        "真装不下仍须截断（> 判停不许放成无裁剪）"
    );
}

// ---- 十五修：池区/下拉动画的涂装侧钉（宪法 §五/§六；弹簧/缓动数学钉
// 在 cfg_page_spec / dual_pool_spec，本组钉涂装吃动画几何）----

#[test]
fn spec_cfg下池_光标滑行涂装钉() {
    // 十五修 §五：选中全包框吃光标弹簧瞬时值（行号小数 → 像素）——
    // 手搓 cursor_row=0.5（行 0/1 正中相位），框必须落在两行之间；
    // 两行本体恒按未选中画（内容/页色即时切换不等光标）。
    // 变异：选中框画回 focus 行（瞬移回潮）/行本体带选中态回潮即红。
    use kfm_na::termview::{lerp_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{
        self, CfgPage, LOWER_ROW_H, POOL_CONTENT_INSET, ROW_GAP, RowView, UpperRow,
    };
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let dark = |c: u32| lerp_rgb(c, 0, 200);
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let row_bg = |px: i64, py: i64| ring_gradient_rgb(dark(acc.c1), dark(acc.c2), px, py, denom);
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![
        RowView {
            title: "系统管理".into(),
            meta: "1 项".into(),
        },
        RowView {
            title: "网络".into(),
            meta: String::new(),
        },
    ]);
    page.set_upper(vec![UpperRow {
        label: "默认服务器".into(),
        value: "本地终端".into(),
        is_dropdown: false,
    }]);
    page.select(
        1,
        1000,
        ps.clone(),
        kfm_na::ui::accent::AccentPair { c1: 0, c2: 0 },
    );
    let mut pg = page.snap(2000); // 弹簧 1s 后贴死（手搓相位下一行覆盖）
    pg.cursor_row = 0.5; // 手搓滑行中途相位（弹簧时值钉在 cfg_page_spec）

    let mut buf = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut buf, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut buf, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut buf, w, h, &ps, &pg, 0, acc, 0, None, false);

    let stride = LOWER_ROW_H as i64 + ROW_GAP;
    let cy = ps.lower.y + POOL_CONTENT_INSET + (0.5f32 * stride as f32).round() as i64;
    let crx = ps.lower.x + POOL_CONTENT_INSET;
    // ①滑行框左粗缘中带 = 渐变 α255 直出（框落在两行之间）
    let (px, py) = (crx + 4, cy + LOWER_ROW_H as i64 / 2);
    assert_eq!(
        buf[py as usize * w as usize + px as usize],
        ring_gradient_rgb(acc.c1, acc.c2, px, py, denom),
        "滑行框左粗缘必须 = 渐变 α255（框不在弹簧瞬时值位即红）"
    );
    // ②行 0 本体无选中框（内容即时切换不等光标——行本体恒未选中）
    let r0 = cfg_page::lower_row_rect(0, &ps.lower);
    assert_eq!(
        buf[(r0.y + 30) as usize * w as usize + (r0.x + 4) as usize],
        row_bg(r0.x + 4, r0.y + 30),
        "行 0 本体不许带选中框（瞬移回潮即红）"
    );
    // ③行 1（focus 行）本体同样无框——选中框在 0.5 相位不在 focus 行
    let r1 = cfg_page::lower_row_rect(1, &ps.lower);
    assert_eq!(
        buf[(r1.y + LOWER_ROW_H as i64 / 2) as usize * w as usize + (r1.x + 4) as usize],
        row_bg(r1.x + 4, r1.y + LOWER_ROW_H as i64 / 2),
        "focus 行本体不许带选中框（框画回 focus 行 = 瞬移回潮即红）"
    );
}

#[test]
fn spec_cfg下拉_抽屉随面钉() {
    // 十七修 §六③（「面与内容一体」垂直实例，2026-09-14 用户拍板）：
    // 选项行/选中细框钉在面板**全高刚体**上随当前高滑出滑回——涂装
    // y 偏移 = 当前高 − 全高。progress=0.5（2 项面板半高）：露出末行
    // （下方选项先入场），行 0 没入面板顶缘。帘幕式（内容钉顶只裁底）
    // 退役。变异：drawer_dy 删（回帘幕钉顶）即红——半高时行 0 钉在
    // 面板顶（无细框深底）、末行细框出底被裁。
    use kfm_na::termview::{frame_bg_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "SYS".into(),
        meta: String::new(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "LOCAL".into(),
        is_dropdown: true,
    }]);
    page.set_options(vec!["LOCAL".into(), "SRV0".into()], 1);

    // 手搓半高相位（缓动时值钉在 cfg_page_spec；涂装只吃 progress 维）。
    // 二十四修拆层：面板走独立槽画布（原点 = (上池内容左内缘, pr.y)，
    // 画布尺渐变），页涂装不再画面板
    page.toggle_dropdown(1000);
    let mut pg1 = page.snap(1125);
    pg1.dropdown_progress = 0.5;

    let lw = tv.text_width("server", 36.0);
    let vw = tv.text_width("LOCAL", 30.0);
    let cw0 = tv
        .text_width("LOCAL", 36.0)
        .max(tv.text_width("SRV0", 36.0))
        + cfg_page::FIELD_TEXT_INSET * 2;
    let t = cfg_page::trigger_rect(&ps.upper, 0, true, lw, vw);
    let max_h = h.saturating_sub(t.y.max(0) as u32 + t.h + 40);
    let pr = cfg_page::dropdown_panel_rect(2, &ps.upper, max_h, 0, true, lw, vw, cw0);
    let row_h = cfg_page::FIELD_ROW_H as i64;
    let canvas_w = ps.upper.w - (cfg_page::POOL_CONTENT_INSET * 2) as u32;
    let ox = ps.upper.x + cfg_page::POOL_CONTENT_INSET;
    let rx = pr.x - ox;
    let dnom = (i64::from(canvas_w) - 1) + (i64::from(pr.h) - 1);
    let mut cv = vec![0u32; (canvas_w * pr.h) as usize];
    tv.paint_dropdown_panel_layer(&mut cv, canvas_w, pr.h, &pg1, &pr, (ox, pr.y), acc);
    let at = |lx: i64, ly: i64| cv[(ly * i64::from(canvas_w) + lx) as usize];
    assert_eq!(
        (pr.h as f32 * 0.5).round() as i64,
        row_h,
        "夹具前提：2 项面板半高 = 单行高"
    );

    // ①抽屉：半高面板内露**末行**（sel=1，drawer_dy = −row_h → 细框
    // 落在画布局部 y∈[0,row_h)）——细框左缘中带 = 渐变 rgb 直出（α 随
    // 深底 252，BAR-067 层投影）（帘幕钉顶回潮 = 此处是行 0 无细框深
    // 底，即红）
    let (fx, fy) = (rx + 1, row_h / 2);
    assert_eq!(
        at(fx, fy),
        (252 << 24) | ring_gradient_rgb(acc.c1, acc.c2, fx, fy, dnom),
        "抽屉随面：末行细框必须落在半高面板内（drawer_dy 被删即红）"
    );
    // ②末行芯 = 渐变暗底不透明直写（≠ 面板深底）
    let (ix, iy) = (rx + pr.w as i64 - 30, row_h / 2);
    assert_eq!(
        at(ix, iy),
        frame_bg_rgb(acc.c1, acc.c2, ix, iy, dnom),
        "末行芯 = 渐变暗底不透明直写"
    );
    // ③面板半高外 = 透明像素 0（合成后下层原样；展开瞬开回潮即红）
    assert_eq!(
        at(rx + pr.w as i64 - 30, row_h + row_h / 2),
        0,
        "半高面板外必须 = 透明（瞬开回潮即红）"
    );
    // ④先框后字（BAR-089 同规）：细框芯条带上必须有末行文字落墨
    let mut ink = 0usize;
    for py in 10..row_h - 10 {
        for px in rx + 20..rx + 120 {
            if at(px, py) != frame_bg_rgb(acc.c1, acc.c2, px, py, dnom) {
                ink += 1;
            }
        }
    }
    assert!(
        ink > 30,
        "选中细框芯上必须有末行文字落墨（实得 {ink} px——「字先框后」回潮 = 0）"
    );
}

#[test]
fn spec_cfg下池_选中行文字落墨钉() {
    // 2026-09-14 用户实机抓「选中态三级框里没有文字」：选中框内芯 =
    // 不透明渐变暗底，涂装序「行字→选中框」把选中行文字盖没。宪法
    // 语义：选中框是光标不是蒙版，行内容恒可见（先框后字）。
    // 变异：文字画回选中框之前（涂装序回潮）即红。
    use kfm_na::termview::{lerp_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let dark = |c: u32| lerp_rgb(c, 0, 200);
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let row_bg = |px: i64, py: i64| ring_gradient_rgb(dark(acc.c1), dark(acc.c2), px, py, denom);
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    // host 字体无 CJK 字形——落墨钉必须 ASCII 夹具（state.md 夹具教训）
    page.set_rows(vec![
        RowView {
            title: "SYS".into(),
            meta: "1 item".into(),
        },
        RowView {
            title: "NET".into(),
            meta: String::new(),
        },
    ]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "local".into(),
        is_dropdown: false,
    }]);
    page.select(
        1,
        1000,
        ps.clone(),
        kfm_na::ui::accent::AccentPair { c1: 0, c2: 0 },
    );
    let pg = page.snap(2000); // 弹簧 600ms 兜底贴死 → 收敛在行 1
    assert_eq!(pg.cursor_row, 1.0, "夹具前提：光标收敛在选中行");

    let mut buf = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut buf, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut buf, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut buf, w, h, &ps, &pg, 0, acc, 0, None, false);

    let r1 = cfg_page::lower_row_rect(1, &ps.lower); // 选中行
    let mut ink = 0usize;
    for py in r1.y + 10..r1.y + 80 {
        for px in r1.x + 20..r1.x + 140 {
            if buf[py as usize * w as usize + px as usize] != row_bg(px, py) {
                ink += 1;
            }
        }
    }
    assert!(
        ink > 50,
        "选中行标题带必须有文字落墨（实得 {ink} px——「字先框后」回潮 = 0）"
    );
}

#[test]
fn spec_cfg下拉_选中细框滑行涂装钉() {
    // 两段时序（2026-09-14 用户拍板）涂装侧：选中细框吃 option_sel_f
    // 瞬时值——手搓 0.5（行 0/1 正中相位），细框必须落在两行之间；
    // 且先框后字（细框芯不透明渐变暗底，选项文字画在芯上不被盖没）。
    // 变异：细框画回 option_sel 整行（瞬移回潮）/字先框后回潮即红。
    use kfm_na::termview::ring_gradient_rgb;
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "SYS".into(),
        meta: "1 item".into(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "LOCAL".into(),
        is_dropdown: true,
    }]);
    page.set_options(vec!["LOCAL".into(), "SRV0".into(), "SRV1".into()], 0);
    page.toggle_dropdown(1000);
    let mut pg = page.snap(1250); // 展开毕
    pg.dropdown_progress = 1.0;
    pg.option_sel_f = 0.5; // 手搓Ⅰ段滑行正中相位（时值钉在 cfg_page_spec）

    // 二十四修拆层：面板走独立槽画布（原点 = (上池内容左内缘, pr.y)，
    // 画布尺渐变），页涂装不再画面板
    let lw = tv.text_width("server", 36.0);
    let vw = tv.text_width("LOCAL", 30.0);
    let cw0 = tv.text_width("SRV1", 36.0) + cfg_page::FIELD_TEXT_INSET * 2;
    let t = cfg_page::trigger_rect(&ps.upper, 0, true, lw, vw);
    let max_h = h.saturating_sub(t.y.max(0) as u32 + t.h + 40);
    let pr = cfg_page::dropdown_panel_rect(3, &ps.upper, max_h, 0, true, lw, vw, cw0);
    let row_h = cfg_page::FIELD_ROW_H as i64;
    let canvas_w = ps.upper.w - (cfg_page::POOL_CONTENT_INSET * 2) as u32;
    let ox = ps.upper.x + cfg_page::POOL_CONTENT_INSET;
    let rx = pr.x - ox;
    let dnom = (i64::from(canvas_w) - 1) + (i64::from(pr.h) - 1);
    let mut cv = vec![0u32; (canvas_w * pr.h) as usize];
    tv.paint_dropdown_panel_layer(&mut cv, canvas_w, pr.h, &pg, &pr, (ox, pr.y), acc);
    let at = |lx: i64, ly: i64| cv[(ly * i64::from(canvas_w) + lx) as usize];

    // ①细框左缘中带（0.5 相位 = 两行之间）= 渐变 rgb 直出（α 随深底
    // 252，BAR-067 层投影）
    let sel_y = (0.5f32 * row_h as f32).round() as i64;
    let (fx, fy) = (rx + 1, sel_y + row_h / 2);
    assert_eq!(
        at(fx, fy),
        (252 << 24) | ring_gradient_rgb(acc.c1, acc.c2, fx, fy, dnom),
        "选中细框必须落在滑行瞬时值位（画回 option_sel 整行 = 瞬移回潮即红）"
    );
    // ②行 0 整相位处（sel_f=0 的旧位）不许有细框左缘——对照
    let (ox2, oy) = (rx + 1, row_h / 2);
    assert_ne!(
        at(ox2, oy),
        ring_gradient_rgb(acc.c1, acc.c2, ox2, oy, dnom),
        "细框旧位不许残留（瞬移回潮对照）"
    );
    // ③选项文字盖过细框内芯：扫描细框芯条带（上下芯内各让 10px，
    // 跨行 0 下半与行 1 上半的字形区），非芯底色像素 = 文字墨
    use kfm_na::termview::frame_bg_rgb;
    let mut ink = 0usize;
    for py in sel_y + 10..sel_y + row_h - 10 {
        for px in rx + 20..rx + 120 {
            if at(px, py) != frame_bg_rgb(acc.c1, acc.c2, px, py, dnom) {
                ink += 1;
            }
        }
    }
    assert!(
        ink > 30,
        "选中细框芯上必须有选项文字落墨（实得 {ink} px——「字先框后」回潮 = 0）"
    );
}

#[test]
fn spec_下拉三角旋转_涂装钉() {
    // 十七修 §六④：▼ 矢量三角绕中心随开合进度旋转 θ = progress×180°
    // （展开 ▼→▲，收起回 ▼；两段时序Ⅰ段冻结在 180°）。基三角 17×9：
    // ly∈[-4.5,4.5]，|lx| ≤ 8.5×(4.5−ly)/9，逐像素中心逆旋转采样。
    // 采样点解析推导（像素中心 +0.5）：
    //   B(-7,-4)：▼ 有（|−6.5|≤7.56）▲ 无（界 0.94）——翻转铁证
    //   C(-7, 3)：▼ 无（界 0.94）▲ 有（|6.5|≤7.56）——翻转铁证
    //   E(-3,-3)：▼ 有（界 6.61）半转无（θ90° 界 1.89）——旋转铁证
    //   G( 2, 4)：▼ 无（界 0）   半转有（|4.5|≤6.61）——旋转铁证
    // 变异：旋转角恒 0（半转 ≡ ▼）/退回逐行静态三角即红。
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "SYS".into(),
        meta: String::new(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "LOCAL".into(),
        is_dropdown: true,
    }]);
    page.set_options(vec!["LOCAL".into()], 0);

    // 三角中心（与涂装同尺复算：值框右缘 −37，行中带）
    let lw = tv.text_width("server", 36.0);
    let vw = tv.text_width("LOCAL", 30.0);
    let vb = cfg_page::trigger_rect(&ps.upper, 0, true, lw, vw);
    let (cx, cy) = (vb.x + vb.w as i64 - 37, vb.y + vb.h as i64 / 2);

    let ink = |buf: &[u32], dx: i64, dy: i64| {
        buf[(cy + dy) as usize * w as usize + (cx + dx) as usize] == 0x00D9_D9D9
    };
    let paint = |page: &CfgPage, progress: f32| {
        let mut pg = page.snap(2000);
        pg.dropdown_progress = progress; // 手搓相位（时值钉在 cfg_page_spec）
        let mut b = vec![0u32; (w * h) as usize];
        termview::paint_cfg_page_chrome(&mut b, w, h, inset, 0, acc);
        tv.paint_cfg_dual_pool(&mut b, w, h, &ps, 0, acc);
        tv.paint_cfg_pool_content(&mut b, w, h, &ps, &pg, 0, acc, 0, None, false);
        b
    };
    let b_down = paint(&page, 0.0); // ▼
    let b_half = paint(&page, 0.5); // 旋转 90°
    let b_up = paint(&page, 1.0); // ▲

    // 翻转铁证（▼ vs ▲）
    assert!(ink(&b_down, -7, -4), "▼ 顶边左必须有墨");
    assert!(!ink(&b_up, -7, -4), "▲ 同位必须无墨（旋转 180° 翻转）");
    assert!(!ink(&b_down, -7, 3), "▼ 下侧左必须无墨");
    assert!(ink(&b_up, -7, 3), "▲ 同位必须有墨（底边宽）");
    // 旋转铁证（半转 90° 与 ▼ 形同异）
    assert!(ink(&b_down, -3, -3), "▼ 斜带必须有墨");
    assert!(
        !ink(&b_half, -3, -3),
        "半转 90° 同位必须无墨（旋转角恒 0 即红）"
    );
    assert!(!ink(&b_down, 2, 4), "▼ 尖下偏右必须无墨");
    assert!(ink(&b_half, 2, 4), "半转 90° 同位必须有墨（旋转移墨）");
    // 稳态前提：▼/▲ 中心列恒有墨（三角没丢）
    assert!(ink(&b_down, 0, 0) && ink(&b_up, 0, 0), "三角中心恒有墨");
}

#[test]
fn spec_视口平移双代同画_涂装钉() {
    // 十七修 §六「面与内容一体」页面级（标签切换）：双池框+内容**双代
    // 同画**——旧代冻结快照带偏移出（dir+1 = 内容左移）、新代活态带
    // 偏移进，视口 = 页内容裁剪带（页环带外零墨）。十八修 §七：平移
    // 距 = 视口宽 + 留隙 G（PAN_GAP_PAGE = 2 格——隐藏布局轴上
    // 「旧代 | 隙 G | 新代」）；曲线 ease-in-out cubic。
    // ①同帧双代：t≈0.26 帧（raw 0.4）——旧代未选中行内芯（**旧
    //   accent** 渐变暗底，渐变锚 temp 原坐标）左移 t×(池宽+G) 落带
    //   内；同帧新代选中行内芯（新 accent）从右进，左部已落带内——
    //   一帧两代各带各色 = 铁证；
    // ②方向律：t≈0.97 帧（raw 0.8）新代选中框左粗缘 = 原左缘 +
    //   (1−t)×(池宽+G)（右进），渐变锚 temp 原坐标——dir 反号变异
    //   （新代左进）即红；
    // ③视口裁剪：带右缘外像素 = chrome 原样（clip 缺失即红）；
    // ④留隙律：t=0.5 帧——旧代池框右粗缘与新一代池框左粗缘同帧
    //   相隔 G，间隙带像素 = 隐藏布局轴上的页背景（temp 源坐标锚
    //   取证；T 改回池宽「贴挤」即红）。
    // 调用方纪律同步钉：Page 域平移中**不**调 paint_cfg_dual_pool（框
    // 由 pool_content 双代自理）。
    use kfm_na::termview::{frame_bg_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc_old = kfm_na::ui::accent::AccentPair {
        c1: 0x0020_C040,
        c2: 0x0040_20C0,
    };
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let pw = ps.upper.w as i64;
    let mut page = CfgPage::new();
    page.set_rows(vec![
        RowView {
            title: "OLD0".into(),
            meta: String::new(),
        },
        RowView {
            title: "OLD1".into(),
            meta: String::new(),
        },
    ]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "LOCAL".into(),
        is_dropdown: true,
    }]);
    page.set_tab(1, 1000, ps.clone(), acc_old); // dir +1（前进）
    page.set_rows(vec![RowView {
        title: "NEW0".into(),
        meta: String::new(),
    }]); // 壳 rebuild 模拟：新代行表

    // ---- ①同帧双代（t≈0.26：旧代未全出、新代左部已进）----
    let travel = pw + cfg_page::PAN_GAP_PAGE; // 十八修留隙律：平移距 = 视口宽+G
    let pg = page.snap(1100); // raw 0.4 → ease-in-out t = 4×0.4³ ≈ 0.256
    let pan = pg.pan.clone().expect("夹具前提：中帧平移账在");
    assert_eq!(pan.dir, 1, "夹具前提：前进方向");
    assert!(
        pan.t > 0.15 && pan.t < 0.45,
        "夹具前提：t≈0.26（实得 {})",
        pan.t
    );
    let d_old = -(pan.t * travel as f32).round() as i64;
    let d_new = ((1.0 - pan.t) * travel as f32).round() as i64;
    let mut b1 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b1, w, h, inset, 0, acc);
    // Page 域：不调 paint_cfg_dual_pool（调用方纪律——框随内容双代）
    tv.paint_cfg_pool_content(&mut b1, w, h, &ps, &pg, 0, acc, 1100, None, false);

    let r1 = cfg_page::lower_row_rect(1, &ps.lower); // 旧代行 1（未选中）
    let (ix, py1) = (r1.x + r1.w as i64 / 2, r1.y + r1.h as i64 / 2);
    let x_old = ix + d_old;
    assert!(x_old > 60, "夹具前提：旧代采样点仍在视口带内（{x_old}）");
    assert_eq!(
        b1[py1 as usize * w as usize + x_old as usize],
        frame_bg_rgb(acc_old.c1, acc_old.c2, ix, py1, denom),
        "旧代行内芯必须带**旧 accent** 左移落位（渐变锚 temp 原坐标；单代/换色即红）"
    );
    // 同帧新代：行 0（选中）内芯左部已进带（temp 原坐标 224 处无字）
    let r0 = cfg_page::lower_row_rect(0, &ps.lower);
    let py0 = r0.y + r0.h as i64 / 2;
    let (sx_new, x_new) = (r0.x + 160, r0.x + 160 + d_new);
    assert!(
        x_new < i64::from(w) - 60 && x_new > 60,
        "夹具前提：新代采样点在视口带内（{x_new}）"
    );
    assert_eq!(
        b1[py0 as usize * w as usize + x_new as usize],
        frame_bg_rgb(acc.c1, acc.c2, sx_new, py0, denom),
        "新代行内芯必须带**新 accent** 右进落位（同帧双代铁证）"
    );

    // ---- ②方向律（t≈0.97：新代选中框左粗缘 = 原左缘 + (1−t)×(池宽+G)）----
    let pg_b = page.snap(1200); // raw 0.8 → ease-in-out t ≈ 0.968
    let pan_b = pg_b.pan.clone().expect("夹具前提：后段平移账在");
    assert!(pan_b.t > 0.9, "夹具前提：t≈0.97（实得 {})", pan_b.t);
    let d_new_b = ((1.0 - pan_b.t) * travel as f32).round() as i64;
    let mut b2 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b2, w, h, inset, 0, acc);
    tv.paint_cfg_pool_content(&mut b2, w, h, &ps, &pg_b, 0, acc, 1200, None, false);
    let edge_temp = ps.lower.x + cfg_page::POOL_CONTENT_INSET + 4; // 左粗缘 temp 原位
    let edge_x = edge_temp + d_new_b;
    assert_eq!(
        b2[py0 as usize * w as usize + edge_x as usize],
        ring_gradient_rgb(acc.c1, acc.c2, edge_temp, py0, denom),
        "新代选中框左粗缘必须 = 原左缘 + (1−t)×(池宽+G)（方向律；dir 反号即红）"
    );

    // ---- ③视口裁剪：带右缘外 = chrome 原样 ----
    let mut bc = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut bc, w, h, inset, 0, acc);
    let ox = (i64::from(w) - 2) as usize;
    assert_eq!(
        b2[py0 as usize * w as usize + ox],
        bc[py0 as usize * w as usize + ox],
        "视口带右缘外不许有双代墨（clip 缺失即红）"
    );

    // ---- ④留隙律（t=0.5：双代池框缘相隔 G，间隙 = 隐藏布局轴页背景）----
    let pg_c = page.snap(1125); // raw 0.5 → ease-in-out t = 0.5
    let pan_c = pg_c.pan.clone().expect("夹具前提：半程平移账在");
    assert!(
        (pan_c.t - 0.5).abs() < 0.01,
        "夹具前提：t=0.5（实得 {})",
        pan_c.t
    );
    let d5 = (0.5 * travel as f32).round() as i64; // |d_old| = d_new = 半程
    let mut b4 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b4, w, h, inset, 0, acc);
    tv.paint_cfg_pool_content(&mut b4, w, h, &ps, &pg_c, 0, acc, 1125, None, false);
    let fy = ps.upper.y + ps.upper.h as i64 / 2; // 上池框腰（无内容墨处）
    // 静物 oracle：旧/新 accent 各画一遍双池框（渐变锚公式件内细节
    // 不手推——参照缓冲同路径涂装，逐像素即铁证）
    let mut bref_old = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut bref_old, w, h, inset, 0, acc_old);
    tv.paint_cfg_dual_pool(&mut bref_old, w, h, &ps, 0, acc_old);
    let mut bref_new = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut bref_new, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut bref_new, w, h, &ps, 0, acc);
    // 旧代池框右粗缘（temp 原位 pool右−2）左出半程，带**旧** accent
    let xr_temp = ps.upper.x + pw - 2;
    assert_eq!(
        b4[fy as usize * w as usize + (xr_temp - d5) as usize],
        bref_old[fy as usize * w as usize + xr_temp as usize],
        "旧代池框右粗缘必须带旧 accent 左出（留隙轴上旧代右端）"
    );
    // 新代池框左粗缘（temp 原位 pool左+1）右进半程，带**新** accent
    let xl_temp = ps.upper.x + 1;
    assert_eq!(
        b4[fy as usize * w as usize + (xl_temp + d5) as usize],
        bref_new[fy as usize * w as usize + xl_temp as usize],
        "新代池框左粗缘必须带新 accent 右进（留隙轴上新代左端）"
    );
    // 间隙带中点：两代源坐标都在各自布局的池外空白 → temp 里是 chrome
    // 页背景原样——取证 = chrome-only 参照的源锚像素（T 改回池宽即红）
    let x_g = ps.upper.x + pw / 2 + 3;
    let sx_g = x_g - d5;
    assert_eq!(
        b4[fy as usize * w as usize + x_g as usize],
        bc[fy as usize * w as usize + sx_g as usize],
        "双代间隙带必须 = 隐藏布局轴上的页背景（留隙 G 被删即贴挤，红）"
    );
}

#[test]
fn spec_上池平移_框静止留隙_涂装钉() {
    // 十八修 §七 上池级实例（2026-09-15 用户实机录屏三条）：
    // ①**框静止**——平移裁剪带 = 上池**内容矩形**（POOL_CONTENT_INSET
    //   内缩），池框左粗竖条（9px）一像素不进带（旧带左缘 x+4 吞了粗
    //   条右半 = 框随内容滑，用户判「视觉断裂」）；带外采样 = 静物
    //   参照逐像素相等（带左缘改回 +4 即红）；
    // ②**留隙 G = PAN_GAP_UPPER**（2×POOL_CONTENT_INSET）+ 双代同画：
    //   t=0.5 帧旧代值框左粗缘带旧 accent 左出、新代标签块背衬带新
    //   accent 右进同帧；间隙带像素 = 池内芯静物原样（两代在源轴上都
    //   够不到 = 隐藏布局「旧代 | 隙 | 新代」；T 改回内容宽即红）。
    use kfm_na::termview::{frame_bg_rgb, ring_gradient_rgb};
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc_old = kfm_na::ui::accent::AccentPair {
        c1: 0x0020_C040,
        c2: 0x0040_20C0,
    };
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![
        RowView {
            title: "a".into(),
            meta: String::new(),
        },
        RowView {
            title: "b".into(),
            meta: String::new(),
        },
        RowView {
            title: "c".into(),
            meta: String::new(),
        },
    ]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "LOCAL".into(),
        is_dropdown: true,
    }]);
    page.select(2, 1000, ps.clone(), acc_old); // dir+1，旧代冻结（行表/页色/池几何）
    page.set_upper(vec![UpperRow {
        // 壳 rebuild 模拟：新代上池行表
        label: "NEWL".into(),
        value: "NEWV".into(),
        is_dropdown: true,
    }]);

    let pg = page.snap(1125); // raw 0.5 → ease-in-out t = 0.5
    let pan = pg.pan.clone().expect("夹具前提：上池平移账在");
    assert_eq!(pan.scope, cfg_page::PanScope::Upper);
    assert_eq!(pan.dir, 1, "夹具前提：前进方向");
    assert!(
        (pan.t - 0.5).abs() < 0.01,
        "夹具前提：t=0.5（实得 {})",
        pan.t
    );

    let mut b1 = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut b1, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut b1, w, h, &ps, 0, acc); // Upper 域：框静物照常
    tv.paint_cfg_pool_content(&mut b1, w, h, &ps, &pg, 0, acc, 1125, None, false);
    // 静物参照（无内容）：chrome + 双池框
    let mut bref = vec![0u32; (w * h) as usize];
    termview::paint_cfg_page_chrome(&mut bref, w, h, inset, 0, acc);
    tv.paint_cfg_dual_pool(&mut bref, w, h, &ps, 0, acc);

    // ---- ①框静止：池框左粗竖条/右细缘/下细缘 = 参照逐像素相等 ----
    for (sx, sy) in [
        (ps.upper.x + 5, ps.upper.y + 30),
        (ps.upper.x + 5, ps.upper.y + ps.upper.h as i64 - 30),
        (ps.upper.x + ps.upper.w as i64 - 2, ps.upper.y + 40),
        (
            ps.upper.x + ps.upper.w as i64 / 2,
            ps.upper.y + ps.upper.h as i64 - 2,
        ),
    ] {
        assert_eq!(
            b1[sy as usize * w as usize + sx as usize],
            bref[sy as usize * w as usize + sx as usize],
            "上池框/粗竖条不许进平移带（带左缘吞条即红）：({sx},{sy})"
        );
    }

    // ---- ②双代同画 + 留隙 G ----
    let x0c = ps.upper.x + cfg_page::POOL_CONTENT_INSET;
    let x1c = ps.upper.x + ps.upper.w as i64 - cfg_page::POOL_CONTENT_INSET;
    let pw_c = x1c - x0c;
    let travel = pw_c + cfg_page::PAN_GAP_UPPER;
    let d5 = (0.5 * travel as f32).round() as i64;
    // 旧代：值框（下拉行 = 三级框全包框）左粗缘左出半程，带旧 accent
    let lw_o = tv.text_width("server", 36.0);
    let vw_o = tv.text_width("LOCAL", 30.0);
    let vb_o = cfg_page::trigger_rect(&ps.upper, 0, true, lw_o, vw_o);
    let (ax, ay) = (vb_o.x + 1, vb_o.y + vb_o.h as i64 / 2);
    assert!(
        ax - d5 > x0c,
        "夹具前提：旧代值框采样点仍在带内（{}）",
        ax - d5
    );
    assert_eq!(
        b1[ay as usize * w as usize + (ax - d5) as usize],
        ring_gradient_rgb(acc_old.c1, acc_old.c2, ax, ay, denom),
        "旧代值框左粗缘必须带旧 accent 左出（渐变锚 temp 原坐标）"
    );
    // 新代：标签块背衬右进半程，带新 accent（背衬 = 渐变暗底 + 白 α20
    // 提亮——blend_px 配方镜像，配方改方 = 有意红）
    let blend_white20 = |bg: u32| -> u32 {
        let ch = |f: u32, d: u32| (f * 20 + d * 235) / 255;
        (ch(255, (bg >> 16) & 0xFF) << 16) | (ch(255, (bg >> 8) & 0xFF) << 8) | ch(255, bg & 0xFF)
    };
    let row0 = cfg_page::upper_row_rect(0, &ps.upper, 0);
    let lb_n = cfg_page::field_label_rect(&row0, tv.text_width("NEWL", 36.0));
    let (bx, by) = (lb_n.x + 2, lb_n.y + lb_n.h as i64 / 2);
    assert!(
        bx + d5 < x1c,
        "夹具前提：新代标签采样点在带内（{}）",
        bx + d5
    );
    assert_eq!(
        b1[by as usize * w as usize + (bx + d5) as usize],
        blend_white20(frame_bg_rgb(acc.c1, acc.c2, bx, by, denom)),
        "新代标签块背衬必须带新 accent 右进（同帧双代）"
    );
    // 间隙带中点：两代源坐标都出带 → 池内芯静物原样（留隙被删即红）
    let x_g = x0c + pw_c / 2 + 3;
    let y_g = ps.upper.y + ps.upper.h as i64 / 2;
    assert_eq!(
        b1[y_g as usize * w as usize + x_g as usize],
        bref[y_g as usize * w as usize + x_g as usize],
        "双代间隙带必须 = 上池内芯静物（留隙 G 被删即贴挤，红）"
    );
}

// ---- 十九修 D8：平移升合成期——裁剪带纯函数（涂装域与合成域同尺） ----

#[test]
fn pan_band_upper_inset_and_page_union_epochs() {
    use kfm_na::termview::{page_pan_band, upper_pan_band};
    use kfm_na::ui::cfg_page::POOL_CONTENT_INSET;
    // 上池带 = 内容矩形（十八修 §七 BAR-091 语义：池框/左粗竖条一像素
    // 不进带——合成域 scissor 吃同一把尺，粗条被吞 = 框随内容滑回归）
    let up = kfm_na::ui::dual_pool::PoolRect {
        x: 100,
        y: 200,
        w: 800,
        h: 600,
    };
    let b = upper_pan_band(&up, 0);
    assert_eq!(
        b.0,
        up.x + POOL_CONTENT_INSET,
        "带左缘 = 内缩（9px 粗条在外）"
    );
    assert_eq!(
        b.2,
        up.x + up.w as i64 - POOL_CONTENT_INSET,
        "带右缘 = 内缩"
    );
    assert_eq!(b.1, up.y + 12, "带上缘 12px 留隙");
    assert_eq!(b.3, up.y + up.h as i64 - 12, "带下缘 12px 留隙");
    // off 平移随尺（与涂装域同参）
    let b2 = upper_pan_band(&up, 50);
    assert_eq!((b2.0, b2.2), (b.0 + 50, b.2 + 50), "off 只平移 x");
    // 页面带：x = 内容带两缘；y = 两代池几何并集（旧代池高弹簧中
    // 几何可异——带必须罩住两代，否则 band fill 擦不净旧帧）
    let pb = page_pan_band(1260, 0, 300, 280, 2000, 1900);
    assert_eq!(pb.1, 280, "y0 = 两代上缘取小");
    assert_eq!(pb.3, 2000, "y1 = 两代下缘取大");
    let (ox, _oy) = kfm_na::ui::tab_bar::content_origin();
    let pb2 = page_pan_band(1260, 30, 300, 300, 2000, 2000);
    assert_eq!(pb2.0, ox as i64 + 30, "x0 = 内容带左缘 + off");
    assert_eq!(
        pb2.2,
        1260 - (kfm_na::termview::AI_PAGE_FRAME_MARGIN
            + kfm_na::termview::AI_PAGE_FRAME_W
            + kfm_na::termview::CELL_W) as i64
            + 30,
        "x1 = 内容带右缘 + off"
    );
}

/// 二十修 §六②：UpperBody 带 = 上池内容矩形挖掉行 0（触发器钉住，
/// 带顶 = 行 0 底缘）；x 缘/下缘与 Upper 带同尺；scroll 真值入算
#[test]
fn pan_band_upper_body_excludes_row0() {
    use kfm_na::termview::{upper_body_pan_band, upper_pan_band};
    use kfm_na::ui::cfg_page::{FIELD_ROW_H, POOL_CONTENT_INSET, upper_row_rect};
    let up = kfm_na::ui::dual_pool::PoolRect {
        x: 100,
        y: 200,
        w: 800,
        h: 600,
    };
    let ub = upper_pan_band(&up, 0);
    let b = upper_body_pan_band(&up, 0, 0);
    assert_eq!(b.0, ub.0, "带左缘与 Upper 带同尺");
    assert_eq!(b.2, ub.2, "带右缘与 Upper 带同尺");
    assert_eq!(b.3, ub.3, "带下缘与 Upper 带同尺");
    let row0_bottom = upper_row_rect(0, &up, 0).y + FIELD_ROW_H as i64;
    assert_eq!(b.1, row0_bottom, "带顶 = 行 0 底缘（触发器一像素不进带）");
    assert!(b.1 > up.y + 12, "行 0 底缘必在 Upper 带上缘之下（真挖掉）");
    // 滚动真值：行 0 随滚动上移，带顶跟走
    let b_scr = upper_body_pan_band(&up, 0, 30);
    assert_eq!(b_scr.1, row0_bottom - 30, "scroll 入算：带顶随滚动上移");
    // off 平移随尺
    let b2 = upper_body_pan_band(&up, 50, 0);
    assert_eq!((b2.0, b2.2), (b.0 + 50, b.2 + 50), "off 只平移 x");
    // POOL_CONTENT_INSET  sanity：行矩形内缩与带内缩同源
    assert_eq!(b.0, up.x + POOL_CONTENT_INSET);
}

// ---- BAR-092（十九修 D8 补钉）：Upper hold 静物=池内芯——上池行不
// 进静态画布。合成域带内静物必须与 vc425 涂装域同语义：行只活在双
// 代滑层里，静物透出内芯渐变；hold 丢维 = 定格行透在隙底（鬼影） ----
#[test]
fn spec_bar092_upper_hold_rows_out_of_canvas() {
    use kfm_na::ui::cfg_page::{self, CfgPage, RowView, UpperRow};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let inset = 120u32;
    let acc = kfm_na::ui::accent::AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, inset);
    pool.set_upper_content_h(72 + cfg_page::FIELD_ROW_H);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![RowView {
        title: "a".into(),
        meta: String::new(),
    }]);
    page.set_upper(vec![UpperRow {
        label: "server".into(),
        value: "LOCAL".into(),
        is_dropdown: true,
    }]);
    // 采样点：首行标签块文字区（字段行 y 带内、标签列 x 带内）
    let fr = cfg_page::upper_row_rect(0, &ps.upper, 0);
    let (sx, sy) = (fr.x + 24, fr.y + cfg_page::FIELD_ROW_H as i64 / 2);

    let paint = |hold: bool, upper: &[UpperRow]| {
        let mut p2 = CfgPage::new();
        p2.set_rows(vec![RowView {
            title: "a".into(),
            meta: String::new(),
        }]);
        p2.set_upper(upper.to_vec());
        let pg2 = p2.snap(1000);
        let mut b = vec![0u32; (w * h) as usize];
        termview::paint_cfg_page_chrome(&mut b, w, h, inset, 0, acc);
        tv.paint_cfg_dual_pool(&mut b, w, h, &ps, 0, acc);
        tv.paint_cfg_pool_content(
            &mut b,
            w,
            h,
            &ps,
            &pg2,
            0,
            acc,
            1000,
            hold.then_some(cfg_page::PanScope::Upper),
            false,
        );
        b[sy as usize * w as usize + sx as usize]
    };
    let with_rows = paint(
        false,
        &[UpperRow {
            label: "server".into(),
            value: "LOCAL".into(),
            is_dropdown: true,
        }],
    );
    let held = paint(
        true,
        &[UpperRow {
            label: "server".into(),
            value: "LOCAL".into(),
            is_dropdown: true,
        }],
    );
    let norows = paint(true, &[]);
    assert_ne!(
        with_rows, held,
        "hold=true 必须把上池行挡在静物外（行墨进静物 = 鬼影回归）"
    );
    assert_eq!(
        held, norows,
        "hold 静物 = 无行内芯（池内芯渐变，非页底/非定格行）"
    );
}

/// BAR-096 钉①（帧饥饿根治件一）：标签栏**层**与整页版逐像素等价——
/// 层坐标 = 页坐标 − y_shift 的等价性。拆层的正确性前提：不许改任何
/// 像素（游标滑行只脏 0.65MB 小层，画面必须与整页版完全一致）。
/// 变异：层函数漏减/多减 y_shift、clip 用页尺 → 红。
#[test]
fn spec_bar096_标签栏层_与整页逐像素等价() {
    use kfm_na::termview::TermEmu;
    use kfm_na::ui::accent::AccentPair;
    use kfm_na::ui::tab_bar::{TAB_LAYER_H, TabBar, content_origin};
    let (w, h) = (1260u32, 2400u32);
    let acc = AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut bar = TabBar::new(&["SYS", "NET"], w);
    bar.select(1, 1000); // 游标滑到标签 1
    let mut snap = bar.snap(1250);
    snap.line_span = Some((100, 1100)); // 池区带（壳层每帧喂）
    // 整页版（y_shift = 0）
    let mut full = vec![0u32; (w * h) as usize];
    tv.paint_tab_bar_layer(&mut full, w, h, 0, &snap, acc);
    // 层版（y_shift = 内容原点）
    let oy = i64::from(content_origin().1);
    let mut layer = vec![0u32; (w * TAB_LAYER_H) as usize];
    tv.paint_tab_bar_layer(&mut layer, w, TAB_LAYER_H, oy, &snap, acc);
    let mut diffs = 0usize;
    for y in 0..TAB_LAYER_H as usize {
        for x in 0..w as usize {
            let a = layer[y * w as usize + x];
            let b = full[(y + oy as usize) * w as usize + x];
            if a != b {
                diffs += 1;
            }
        }
    }
    assert_eq!(
        diffs, 0,
        "标签栏层与整页版必须逐像素等价（差 {diffs} 像素）"
    );
    let nz = layer.iter().filter(|p| **p & 0x00FF_FFFF != 0).count();
    assert!(nz > 1000, "标签栏层必须真落墨（非零像素 {nz}）");
}

/// BAR-096 钉②（帧饥饿根治件二）：下池光标层**渐变保真**——层像素必须
/// 采出「框在页上原位」的色（页坐标 + 页分母），不许漂成层画布尺
/// （层 1000×162 的 denom ≪ 页 1260×2400 的 denom，混用 = 颜色整体
/// 变调）。独立复算锚：ring_gradient_rgb/frame_bg_rgb 按页坐标重算。
/// 变异：paint_lower_cursor_layer 传 (0,0,层denom) → 红。
#[test]
fn spec_bar096_光标层_渐变保真钉() {
    use kfm_na::termview::{TermEmu, frame_bg_rgb, ring_gradient_rgb};
    use kfm_na::ui::accent::AccentPair;
    let (w, h) = (1260u32, 2400u32);
    let acc = AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let (cw, ch) = (1000u32, 162u32);
    let (px0, py0) = (102i64, 777i64);
    let denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let mut layer = vec![0u32; (cw * ch) as usize];
    tv.paint_lower_cursor_layer(&mut layer, cw, ch, px0, py0, denom, acc);
    // 左粗竖条内（x=5）——环带 = 渐变实色 α255
    let y_mid = (ch / 2) as i64;
    let got = layer[(ch / 2) as usize * cw as usize + 5];
    let want = ring_gradient_rgb(acc.c1, acc.c2, px0 + 5, py0 + y_mid, denom);
    assert_eq!(
        got & 0x00FF_FFFF,
        want & 0x00FF_FFFF,
        "左粗条颜色必须按页坐标页尺采（保真条）"
    );
    // 内芯（中心点）——渐变暗底不透明直出
    let got_c = layer[(ch / 2) as usize * cw as usize + (cw / 2) as usize];
    let want_c = frame_bg_rgb(acc.c1, acc.c2, px0 + (cw / 2) as i64, py0 + y_mid, denom);
    assert_eq!(
        got_c & 0x00FF_FFFF,
        want_c & 0x00FF_FFFF,
        "内芯颜色必须按页坐标页尺采（保真条）"
    );
}

/// BAR-107 钉（2026-09-17 用户拍板「框动行不动」，取代 BAR-096 落墨钉）：
/// 光标层**只画框、不画字**——旧版把选中行文字烤进光标层（BAR-089 绕道：
/// 框芯不透明会盖字），平移期光标层与行层各走一条曲线 → 同一行文字同帧
/// 两处错位（用户真机录屏「整个框都移动了+重影」，vc449 差分 a00 帧
/// 「组件」双现定罪）。修 = 芯改半透明（LOWER_CURSOR_CORE_ALPHA），文字
/// 只归行层透出。钉三言：
/// ①内芯像素 alpha 字节 = 0x55 且 RGB 非零（mark_chrome_alpha 对高字节
///   非 0 像素直通 → GLES 真混合，下层行文字透出）；
/// ②环带像素 alpha 字节 = 0 且 RGB 非零（mark 判不透明 = 实框不变）；
/// ③剪影外像素全 0（圆角外透明，不污下层）。
/// （0x55 与 termview LOWER_CURSOR_CORE_ALPHA 咬合——pub(crate) 测试不可
/// 达，此处硬编码契约值，调常量须同步本钉。）
/// 变异：core_alpha 传 0（芯回不透明）→ 言①红；环带误写 alpha → 言②红。
#[test]
fn spec_bar107_光标层_框动行不动() {
    use kfm_na::termview::TermEmu;
    use kfm_na::ui::accent::AccentPair;
    let acc = AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let (cw, ch) = (1000u32, 162u32);
    let (px0, py0) = (102i64, 777i64);
    let denom = 4058i64;
    let mut layer = vec![0u32; (cw * ch) as usize];
    tv.paint_lower_cursor_layer(&mut layer, cw, ch, px0, py0, denom, acc);
    // ①内芯（中心点）——半透明渐变暗底
    let core = layer[(ch / 2) as usize * cw as usize + (cw / 2) as usize];
    assert_eq!(
        core >> 24,
        0x55,
        "内芯必须自带半透明 alpha（0x55）让行层文字透出（BAR-107）"
    );
    assert!(core & 0x00FF_FFFF != 0, "内芯必须是渐变暗底非纯黑");
    // ②环带（左粗条 x=5）——裸 RGB（mark 后不透明实框）
    let ring = layer[(ch / 2) as usize * cw as usize + 5];
    assert_eq!(ring >> 24, 0, "环带必须保持裸 RGB（mark 判不透明实框）");
    assert!(ring & 0x00FF_FFFF != 0, "环带必须真落墨");
    // ③剪影外（圆角外角点）——全 0 透明
    assert_eq!(
        layer[cw as usize + 1],
        0,
        "圆角剪影外必须全透明（不污下层行层）"
    );
}

/// BAR-102：字形位图缓存——槽涂装/CPU 画字热路径不许每次从轮廓重光栅
/// （全页重烘 50-130ms 压在动画关键帧上 = 120Hz 屏肉眼黑洞）。
/// 钉两言：二次取同字同号必命中同一份 Arc（零重光栅）；缓存位图与
/// 直调 rasterize 逐字节一致（缓存不许改变任何像素）。
/// 变异抽检：rasterize_cached 改成每次新光栅 → Arc::ptr_eq 言红；
/// key 摘字号/字体维 → 串号/串字体时一致言或像素钉红
#[test]
fn spec_bar102_字形缓存_命中且逐字节一致() {
    let tv = host_termview(80, 24);
    let (hit_a, same_a) = tv.spec_glyph_cache_probe('K', 36.0);
    assert!(hit_a, "二次取同字同号必须命中缓存（Arc 同一份）");
    assert!(same_a, "缓存位图必须与直调 rasterize 逐字节一致");
    // 第二字符独立成键，且同样命中
    let (hit_b, same_b) = tv.spec_glyph_cache_probe('g', 30.0);
    assert!(hit_b && same_b, "每 (字,字号,字体) 独立成键且命中");
}

/// BAR-103：渐变 LUT 化逐像素等价钉——环带/池内芯/行框内芯的 LUT 采样
/// 值必须等于 ring_gradient_rgb/frame_bg_rgb 逐点直算（LUT 建表错/索引
/// 偏 1/钳位语义漂移即红）。背景：计时考题定罪全页重烘 63ms =
/// 池框 29+页环 18+内容 15（每像素 1 次整数除法+内芯每像素 2 次压暗
/// lerp+逐像素边界检查），LUT 后 26ms。
/// 变异抽检：LUT 索引 s+1 → 三言齐红；行切片起点偏 1 → 红
#[test]
fn spec_bar103_渐变lut_采样钉() {
    use kfm_na::termview::{POOL_FRAME_R, TermEmu, frame_bg_rgb, ring_gradient_rgb};
    use kfm_na::ui::accent::AccentPair;
    use kfm_na::ui::cfg_page::{CfgPage, RowView, lower_row_rect};
    use kfm_na::ui::dual_pool::DualPool;
    let (w, h) = (1260u32, 2400u32);
    let acc = AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, 0);
    pool.set_upper_content_h(600);
    let ps = pool.layout(1000);
    let mut page = CfgPage::new();
    page.set_rows(vec![
        RowView {
            title: "SYS".into(),
            meta: "1 item".into(),
        },
        RowView {
            title: "NET".into(),
            meta: String::new(),
        },
    ]);
    let cs = page.snap(1000);

    let mut buf = vec![0u32; (w * h) as usize];
    tv.paint_cfg_dual_pool(&mut buf, w, h, &ps, 0, acc);
    // 言一：上池内芯中带整行段逐点扫（±1 索引错在 t 跨档点必留色差——
    // 单点钉曾漏 ±1 变异：整除量化+t 步进亚字节，两点采样咬不住）
    let up = &ps.upper;
    let denom = (i64::from(up.w) - 1) + (i64::from(up.h) - 1);
    let fy = 100i64;
    for sx in 60..(i64::from(up.w) - 30) {
        let got = buf[(up.y + fy) as usize * w as usize + (up.x + sx) as usize];
        let want = frame_bg_rgb(acc.c2, acc.c1, sx, fy, denom);
        assert_eq!(got, want, "池内芯 LUT 行段逐点等价（sx={sx}）");
    }
    // 言二：上池左粗条中带整列段逐点扫（直边区 cov=255 直写）
    let fx = 2i64;
    let (rc_w, fh_i) = (POOL_FRAME_R as i64 + 12, i64::from(up.h));
    for ly in (rc_w + 5)..(fh_i - rc_w - 5) {
        let got = buf[(up.y + ly) as usize * w as usize + (up.x + fx) as usize];
        let want = ring_gradient_rgb(acc.c2, acc.c1, fx, ly, denom);
        assert_eq!(got, want, "环带 LUT 列段逐点等价（ly={ly}）");
    }
    // 言三：下池行框内芯行段逐点扫（行框渐变参照 = 页坐标页尺）
    tv.paint_cfg_pool_content(&mut buf, w, h, &ps, &cs, 0, acc, 1000, None, true);
    let r1 = lower_row_rect(1, &ps.lower); // 行 1 meta 空，段内无文字墨
    let page_denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    let cy = r1.y + i64::from(r1.h) / 2;
    for cx in (r1.x + 400)..(r1.x + 700) {
        let got = buf[cy as usize * w as usize + cx as usize];
        let want = frame_bg_rgb(acc.c1, acc.c2, cx, cy, page_denom);
        assert_eq!(got, want, "行框内芯 LUT 行段逐点等价（cx={cx}）");
    }
}

/// BAR-097 钉①：池框几何层与在页版**逐像素等价**——层涂装是平移语义
/// （渐变参照 = 池框局部坐标，平移不变），层像素 (lx,ly) 必须等于页
/// 像素 (area.x+lx, area.y+ly)。渐变参照错（拿页坐标采/拿错分母）或
/// 平移漏轴即红。变异：y_shift 传 0 → 红
#[test]
fn spec_bar097_池框层_逐像素等价() {
    use kfm_na::termview::TermEmu;
    use kfm_na::ui::accent::AccentPair;
    use kfm_na::ui::dual_pool::{DualPool, pool_area};
    let (w, h) = (1260u32, 2400u32);
    let acc = AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, 0);
    pool.set_upper_content_h(600);
    let ps = pool.layout(1000);
    let area = pool_area(w, h, 0);
    assert_eq!(
        (area.x, area.y),
        (ps.upper.x, ps.upper.y),
        "池区原点 ≡ 上池原点（布局数学同源）"
    );

    // BAR-104：页画布预填页底色——真实页非透明黑；透明底建模丢了半透明
    // 涂装（框环 AA 边/发光带）的底色贡献，正是 BAR-104 钉盲区根源
    let mut page = vec![kfm_na::ui::accent::CARD_PAGE_BG; (w * h) as usize];
    tv.paint_cfg_dual_pool(&mut page, w, h, &ps, 0, acc);
    let (aw, ah) = (area.w, area.h);
    let mut layer = vec![0u32; (aw * ah) as usize];
    tv.paint_pool_frames_layer(&mut layer, aw, ah, area.x, area.y, &ps, acc);

    // 两池包围盒外扩 16px（含发光带）全像素对拍
    let mut diffs = 0usize;
    for r in [&ps.upper, &ps.lower] {
        let (x0, y0) = (r.x - 16, r.y - 16);
        let (x1, y1) = (r.x + i64::from(r.w) + 16, r.y + i64::from(r.h) + 16);
        for py in y0..y1 {
            for px in x0..x1 {
                if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
                    continue;
                }
                let (lx, ly) = (px - area.x, py - area.y);
                if lx < 0 || ly < 0 || lx >= i64::from(aw) || ly >= i64::from(ah) {
                    continue;
                }
                let a = layer[ly as usize * aw as usize + lx as usize];
                let b = page[py as usize * w as usize + px as usize];
                if a != b {
                    diffs += 1;
                }
            }
        }
    }
    assert_eq!(diffs, 0, "池框层与在页版必须逐像素等价（差 {diffs}）");
    let nz = layer.iter().filter(|p| **p & 0x00FF_FFFF != 0).count();
    assert!(nz > 100_000, "池框层必须真落墨（非零像素 {nz}）");
}

/// BAR-097 钉②：下池行层与在页版**逐像素等价**（行框区域内）——渐变
/// 参照锚终点页坐标页尺（grad_ref=(x_shift, y_origin, page_denom)），
/// 贴死帧与稳态配置槽烘焙逐像素一致交接。变异：grad_ref.1 传 0 → 红
#[test]
fn spec_bar097_下池行层_逐像素等价() {
    use kfm_na::termview::TermEmu;
    use kfm_na::ui::accent::AccentPair;
    use kfm_na::ui::cfg_page::{CfgPage, RowView};
    use kfm_na::ui::dual_pool::{DualPool, pool_area};
    let (w, h) = (1260u32, 2400u32);
    let acc = AccentPair {
        c1: 0x00FF_6000,
        c2: 0x0000_80FF,
    };
    let tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    let mut pool = DualPool::new(w, h);
    pool.set_viewport(w, h, 0);
    pool.set_upper_content_h(600);
    let ps = pool.layout(1000);
    let area = pool_area(w, h, 0);
    let rows = vec![
        RowView {
            title: "SYS".into(),
            meta: "1 item".into(),
        },
        RowView {
            title: "NET".into(),
            meta: String::new(),
        },
    ];
    let mut page_core = CfgPage::new();
    page_core.set_rows(rows.clone());
    let cs = page_core.snap(1000);

    // 在页版：池框铺底 + 行内容（行文字混色底 = 行框内芯，两层同源）。
    // BAR-104：页画布预填页底色——真实页非透明黑；透明底建模丢了半透明
    // 涂装的底色贡献，正是 BAR-104 钉盲区根源
    let mut page = vec![kfm_na::ui::accent::CARD_PAGE_BG; (w * h) as usize];
    tv.paint_cfg_dual_pool(&mut page, w, h, &ps, 0, acc);
    tv.paint_cfg_pool_content(&mut page, w, h, &ps, &cs, 0, acc, 1000, None, true);
    // 层版：BAR-104 起层自填页底色+自画下池框（与 PoolFx 同源）——
    // 行框内芯不透明 → 文字混色底同在页版，层内无透明像素
    let (aw, ah) = (area.w, area.h);
    let mut layer = vec![0u32; (aw * ah) as usize];
    let page_denom = (i64::from(w) - 1) + (i64::from(h) - 1);
    tv.paint_lower_rows_layer(
        &mut layer, aw, ah, area.x, ps.lower.y, &ps.lower, &rows, acc, page_denom,
    );

    // BAR-104：对拍范围扩到下池整框（含框环 AA 边/渐变内芯/行间隙）——
    // 行内墨全是不透明落墨时行内对拍咬不住透明底变异（实测），框环半
    // 透明边才是 BAR-104 病灶的目击证人
    let mut diffs = 0usize;
    let mut inked = 0usize;
    let (bx0, by0) = (ps.lower.x, ps.lower.y);
    let (bx1, by1) = (
        ps.lower.x + i64::from(ps.lower.w),
        ps.lower.y + i64::from(ps.lower.h),
    );
    for py in by0..by1 {
        for px in bx0..bx1 {
            let (lx, ly) = (px - area.x, py - ps.lower.y);
            let a = layer[ly as usize * aw as usize + lx as usize];
            inked += 1; // BAR-104：层全不透明（自填底色），逐像素全比
            let b = page[py as usize * w as usize + px as usize];
            if a != b {
                diffs += 1;
            }
        }
    }
    assert!(inked > 50_000, "行层必须真落墨（{inked} 像素）");
    assert_eq!(diffs, 0, "下池行层与在页版必须逐像素等价（差 {diffs}）");
}

// ---------- A 档：键盘弹起视口平移（2026-09-18 用户拍板「键盘弹起改视口平移」） ----------
// 纪律：网格尺寸与可见视口解耦——键盘弹起 grid 行数纹丝不动（不上报 resize
// → tmux 零重排），渲染视口整体上移 kb_shift_rows 行让光标露出，触摸逆映射
// 补平移量。策略 = 追光标：min(需要的量, 遮挡量)——光标本就在可见区则一行
// 不动（vim 编辑文件顶部零跳动）；光标贴底（kimi CLI）退化为露出即止。

#[test]
fn spec_键盘平移_追光标钳制() {
    use kfm_na::termview::kb_shift_rows;
    // 光标在可见区内 → 0（vim 编辑文件顶部场景：一行不动）
    assert_eq!(kb_shift_rows(25, 10, false), 0);
    // 贴可见区底沿（最后一行可见）→ 0
    assert_eq!(kb_shift_rows(25, 24, false), 0);
    // 光标刚被遮一行 → 只移一行（min 钳制，不多移）
    assert_eq!(kb_shift_rows(25, 25, false), 1);
    // 光标贴 47 行屏底、键盘遮掉后半（kimi CLI 场景）→ 移到露出为止
    assert_eq!(kb_shift_rows(25, 46, false), 22);
    // 看历史（display_offset>0）→ 恒 0，不打扰阅读
    assert_eq!(kb_shift_rows(25, 46, true), 0);
    assert_eq!(kb_shift_rows(25, 10, true), 0);
    // 防御：可见区 0 行（键盘遮满）→ 0（移了也看不见，不做无用功）
    assert_eq!(kb_shift_rows(0, 46, false), 0);
    // 防御：负行（历史区游标）→ 0
    assert_eq!(kb_shift_rows(25, -3, false), 0);
}

#[test]
fn spec_键盘平移_遮挡驱动追光标全链() {
    use kfm_na::termview::{MARGIN_X, MARGIN_Y, margin_top};
    let (cw, ch) = (CELL_W, CELL_H);
    let mt = margin_top(ch);
    let mut tv = host_termview(20, 5);
    // 光标推到第 4 行（0 基）并落标记：A 在 (0,0)，Z 在 (0,4)
    tv.feed(b"A");
    for _ in 0..4 {
        tv.feed(b"\r\n");
    }
    tv.feed(b"Z");
    // 窗高 = 顶带 + 5 行格 + 底余量：无遮挡时 5 行全可见
    let win_w = 2 * MARGIN_X + 20 * cw;
    let win_h = mt + 5 * ch + MARGIN_Y;
    // 无遮挡 → 光标行 4 < 可见 5，不追
    assert!(!tv.sync_kb_shift(win_h, 0));
    assert_eq!(tv.kb_shift(), 0);
    // 遮挡带 = 2 行高 → 可见 3 行，光标行 4 → shift = 4+1-3 = 2
    assert!(tv.sync_kb_shift(win_h, 2 * ch));
    assert_eq!(tv.kb_shift(), 2);
    // 同值再调 → 判等不抖（轮询路径每 100ms 来一遍，不能假报变化）
    assert!(!tv.sync_kb_shift(win_h, 2 * ch));
    // GPU 收集：Z 从屏行 4 平移到屏行 2（py = 顶带 + 2 格高）；
    // A 所在屏行 -2 被推出顶沿 → 裁剪不收集
    let cells = tv.collect_gpu_cells(win_w, win_h);
    let z = cells.iter().find(|c| c.c == 'Z').expect("Z 必须可见");
    assert_eq!(z.py, mt + 2 * ch, "Z 必须随视口平移 2 行");
    assert!(!cells.iter().any(|c| c.c == 'A'), "被推出顶沿的行必须裁剪");
    // 遮挡收回 → 归 0，Z 回到屏行 4
    assert!(tv.sync_kb_shift(win_h, 0));
    assert_eq!(tv.kb_shift(), 0);
    let cells = tv.collect_gpu_cells(win_w, win_h);
    let z = cells.iter().find(|c| c.c == 'Z').expect("Z 必须回原地");
    assert_eq!(z.py, mt + 4 * ch);
}

#[test]
fn spec_键盘平移_光标本可见则不追() {
    use kfm_na::termview::{MARGIN_X, MARGIN_Y, margin_top};
    let (cw, ch) = (CELL_W, CELL_H);
    let mt = margin_top(ch);
    let mut tv = host_termview(20, 5);
    tv.feed(b"Q"); // 光标停在第 0 行（vim 编辑文件顶部场景）
    let win_h = mt + 5 * ch + MARGIN_Y;
    // 遮挡 2 行高：可见 3 行，光标行 0 远在可见区内 → 一行不动
    assert!(!tv.sync_kb_shift(win_h, 2 * ch));
    assert_eq!(tv.kb_shift(), 0);
    let _ = (MARGIN_X, cw);
}

#[test]
fn spec_键盘平移_看历史时零平移() {
    use kfm_na::termview::{MARGIN_Y, margin_top};
    let ch = CELL_H;
    let mt = margin_top(ch);
    let mut tv = host_termview(20, 5);
    // 灌 20 行把内容顶进历史，光标停在最后
    for i in 0..20 {
        tv.feed(format!("row{i:02}\r\n").as_bytes());
    }
    let win_h = mt + 5 * ch + MARGIN_Y;
    // 滚 2 行进历史 → display_offset>0：即使光标在「被遮」位置也恒 0
    tv.scroll_lines(2);
    assert!(tv.display_offset() > 0);
    assert!(!tv.sync_kb_shift(win_h, 2 * ch));
    assert_eq!(tv.kb_shift(), 0);
    // 回到底部贴最新 → 恢复追光标
    tv.scroll_to_bottom();
    assert!(tv.sync_kb_shift(win_h, 2 * ch));
    assert!(tv.kb_shift() > 0);
}

#[test]
fn spec_键盘平移_触摸逆映射补平移() {
    use kfm_na::termview::{MARGIN_X, MARGIN_Y, margin_top};
    let (cw, ch) = (CELL_W, CELL_H);
    let mt = margin_top(ch);
    let mut tv = host_termview(20, 5);
    for _ in 0..4 {
        tv.feed(b"\r\n");
    }
    tv.feed(b"hello world"); // 落在网格行 4
    let win_h = mt + 5 * ch + MARGIN_Y;
    assert!(tv.sync_kb_shift(win_h, 2 * ch));
    assert_eq!(tv.kb_shift(), 2);
    // 网格行 4 平移后画在屏行 2：点屏行 2 的像素必须选中网格行 4 的词
    let x = f64::from(MARGIN_X + 2 * cw) + 1.0;
    let y = f64::from(mt + 2 * ch) + 5.0;
    tv.select_word_at(x, y);
    assert_eq!(
        tv.selected_text().as_deref(),
        Some("hello"),
        "触摸点必须映射回平移前的网格行（眼手同尺）"
    );
}
