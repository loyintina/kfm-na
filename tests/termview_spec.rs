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

fn host_termview(cols: u32, rows: u32) -> TermView {
    let bytes = std::fs::read(HOST_FONT).expect("host 测试字体缺失");
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .expect("fontdue 不认 DejaVuSansMono");
    TermView::new(font, cols, rows, CELL_W, CELL_H)
}

// ---------- A 档：布局数学 ----------

#[test]
fn spec_布局_整除与非整除() {
    // 整除：100x48 窗口 10x24 格 → 10x2
    assert_eq!(grid_dims(100, 48, 10, 24), (10, 2));
    // 非整除向下取整：105x50 → 10x2（余下的半格不算）
    assert_eq!(grid_dims(105, 50, 10, 24), (10, 2));
    // 尖刺常量：1080x2400 屏 12x24 格 → 90x100
    assert_eq!(grid_dims(1080, 2400, CELL_W, CELL_H), (90, 100));
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
    assert_eq!(cell_origin(89, 99, CELL_W, CELL_H), (1068, 2376));
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
    let (cx, cy) = cell_origin(5, 0, CELL_W, CELL_H);
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

#[test]
fn spec_字体_候选全灭返回none() {
    assert!(termview::load_font(&["/nonexistent/a.ttf", "/nonexistent/b.ttf"]).is_none());
}

#[test]
fn spec_字体_host候选命中() {
    let (path, _font) = termview::load_font(&["/nonexistent/x.ttf", HOST_FONT])
        .expect("DejaVuSansMono 必须加载成功");
    assert_eq!(path, HOST_FONT);
}
