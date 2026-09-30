//! ui/term_btn.rs — 终端钮控件（2026-09-30 BAR-208 用户拍板：阅读页退出
//! 口 = 右上角设置钮**左边**的终端按钮，点击回中央终端页面——阅读页
//! 「盖在中央页上」存在逻辑重构的配套：右滑/左滑让给文件树与解析
//! 占位页，退出只认本钮）。
//!
//! 分层：几何+命中 = 本册纯逻辑（A 档考题 tests/term_btn_spec.rs）；
//! 涂装挂阅读页槽烘焙末尾（GLES）与兜底路径阅读页内容墨之后——只在
//! 阅读页顶出现（可见性语义跟阅读页槽走，零新逻辑）；触发 = 壳层点按
//! → ai_presence.dismiss_top(Reader)。
//!
//! 字形 = 程序化「>‿」提示符（折线 chevron + 底横，点到线段距离掩码，
//! 与 demo_icon 同族工艺——不赌字体字形）。墨色/不透明度与齿轮同档
//! （浮在页面上不抢读）。

/// 命中盒边长（px）：与齿轮同尺（GEAR_HIT_PX 116，两钮视觉/触控同族）
pub const TERM_BTN_HIT_PX: u32 = 116;
/// 字形盒边长（px）：与齿轮同尺（GEAR_GLYPH_PX 72）
pub const TERM_BTN_GLYPH_PX: u32 = 72;
/// 与齿轮字形盒的横向净距（px）
pub const TERM_BTN_GAP_PX: u32 = 12;
/// 墨色：与齿轮同族（GEAR_INK——终卡涂装族 token 化是既有债，随族）
pub const TERM_BTN_INK: u32 = crate::ui::gear::GEAR_INK;
/// 不透明度：与齿轮同档
const TERM_BTN_ALPHA: f32 = 0.85;

/// 命中盒（x, y, w, h）：齿轮字形盒正左 GAP 处，与齿轮同中轴同尺。
/// 几何单源：涂装与命中同读此函数
pub fn hit_rect(buf_w: u32) -> (u32, u32, u32, u32) {
    let (ghx, ghy, _, _) = crate::ui::gear::hit_rect(buf_w);
    (
        ghx.saturating_sub(TERM_BTN_HIT_PX + TERM_BTN_GAP_PX),
        ghy,
        TERM_BTN_HIT_PX,
        TERM_BTN_HIT_PX,
    )
}

/// 命中判定（纯函数）：点在命中盒内即中
pub fn hit(x: f64, y: f64, buf_w: u32) -> bool {
    let (hx, hy, hw, hh) = hit_rect(buf_w);
    x >= f64::from(hx) && x < f64::from(hx + hw) && y >= f64::from(hy) && y < f64::from(hy + hh)
}

/// 涂装：阅读页槽烘焙末尾/兜底路径内容墨之后调用（画在正文之上）。
/// 掩码外一字节不动（页面内容透出来）。小缓冲早退保命
pub fn paint(buf: &mut [u32], buf_w: u32, buf_h: u32) {
    if buf_w < crate::termview::MARGIN_X * 2 + TERM_BTN_GLYPH_PX * 2 + TERM_BTN_GAP_PX
        || buf_h < crate::termview::MARGIN_Y * 2 + TERM_BTN_GLYPH_PX
    {
        return;
    }
    let (hx, hy, hw, _) = hit_rect(buf_w);
    let (cx, cy) = (hx + hw / 2, hy + hw / 2); // 字形心 = 命中盒心（单源）
    paint_at(buf, buf_w, buf_h, cx, cy);
}

/// 任意心位涂装（宪法 §六 样式唯一来源，与 gear::paint_at 同纪律）。
/// 「>‿」掩码（笔触厚 4.5）：
/// chevron = (0.20,0.18)→(0.46,0.50)→(0.20,0.82) 两段折线（盒归一坐标）；
/// 底横 = (0.56,0.82)→(0.86,0.82) 一段
pub fn paint_at(buf: &mut [u32], buf_w: u32, buf_h: u32, cx: u32, cy: u32) {
    let g = f64::from(TERM_BTN_GLYPH_PX);
    let (fx, fy) = (f64::from(cx), f64::from(cy));
    let th = 4.5f64;
    let (x0, y0) = (cx - TERM_BTN_GLYPH_PX / 2, cy - TERM_BTN_GLYPH_PX / 2);
    // 三段折线（盒归一坐标）
    let segs: [((f64, f64), (f64, f64)); 3] = [
        ((g * 0.20, g * 0.18), (g * 0.46, g * 0.50)),
        ((g * 0.46, g * 0.50), (g * 0.20, g * 0.82)),
        ((g * 0.56, g * 0.82), (g * 0.86, g * 0.82)),
    ];
    let (ir, ig, ib) = (
        (TERM_BTN_INK >> 16) & 0xFF,
        (TERM_BTN_INK >> 8) & 0xFF,
        TERM_BTN_INK & 0xFF,
    );
    for py in y0..(y0 + TERM_BTN_GLYPH_PX) {
        if py >= buf_h {
            break;
        }
        for px in x0..(x0 + TERM_BTN_GLYPH_PX) {
            if px >= buf_w {
                break;
            }
            let dx = f64::from(px) + 0.5 - fx + g / 2.0; // 盒内坐标
            let dy = f64::from(py) + 0.5 - fy + g / 2.0;
            let mut on = false;
            for ((ax, ay), (bx, by)) in segs {
                let (vx, vy) = (bx - ax, by - ay);
                let t = (((dx - ax) * vx + (dy - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
                let (qx, qy) = (ax + t * vx, ay + t * vy);
                if (dx - qx).hypot(dy - qy) < th {
                    on = true;
                    break;
                }
            }
            if !on {
                continue;
            }
            let i = (py * buf_w + px) as usize;
            let dst = buf[i];
            let (dr, dg, db) = (
                ((dst >> 16) & 0xFF) as f32,
                ((dst >> 8) & 0xFF) as f32,
                (dst & 0xFF) as f32,
            );
            let a = TERM_BTN_ALPHA;
            let r = (dr * (1.0 - a) + ir as f32 * a) as u32;
            let g2 = (dg * (1.0 - a) + ig as f32 * a) as u32;
            let b = (db * (1.0 - a) + ib as f32 * a) as u32;
            buf[i] = (r << 16) | (g2 << 8) | b;
        }
    }
}
