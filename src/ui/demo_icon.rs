//! ui/demo_icon.rs — md 渲染打样 demo 页入口钮（2026-09-26 研究线拍板：
//! 设置齿轮正下方的第二枚小图标，烧瓶形 = 「打样/试制」语义）。
//!
//! 工艺与 gear.rs 同族：几何+命中 = 本册纯逻辑（A 档考题
//! tests/demo_icon_spec.rs），hit_rect 几何单源（命中与涂装同读）；
//! 字形 = 程序化像素掩码（圆底三角身 + 细颈的烧瓶，解析式轮廓描边，
//! 不赌字体字形）；涂装挂 termview::paint_term_card_chrome 齿轮旁
//! （终卡槽整层隐语义白拿——只在裸终端页出现）；触发 = 壳层点按 →
//! ai_presence.summon_panel(Demo)。笔触 = accent 135° 渐变（同
//! paint_ft_eye 一把尺），α 与齿轮同档（浮在终端文字上不抢读）。

/// 命中盒边长（px）：同族缩小档（齿轮 116 → 本钮 88，触控补偿同比缩）
pub const DEMO_HIT_PX: u32 = 88;
/// 字形盒边长（px）：齿轮 72 按命中盒比例缩（88×72/116 ≈ 54.6 → 56 取偶）
pub const DEMO_GLYPH_PX: u32 = 56;
/// 不透明度：与齿轮同档（GEAR_ALPHA 0.85——看得见又不挡终端文字）
const DEMO_ALPHA: f32 = 0.85;

/// 命中盒（x, y, w, h）：齿轮命中盒正下方留 0.5 格（CELL_H/2）间距，
/// 与齿轮同中轴。几何单源：涂装与命中同读此函数
pub fn hit_rect(buf_w: u32) -> (u32, u32, u32, u32) {
    let (ghx, ghy, ghw, ghh) = crate::ui::gear::hit_rect(buf_w);
    let cx = ghx + ghw / 2;
    let top = ghy + ghh + crate::termview::CELL_H / 2;
    (
        cx.saturating_sub(DEMO_HIT_PX / 2),
        top,
        DEMO_HIT_PX,
        DEMO_HIT_PX,
    )
}

/// 命中判定（纯函数）：点在命中盒内即中
pub fn hit(x: f64, y: f64, buf_w: u32) -> bool {
    let (hx, hy, hw, hh) = hit_rect(buf_w);
    x >= f64::from(hx) && x < f64::from(hx + hw) && y >= f64::from(hy) && y < f64::from(hy + hh)
}

/// 涂装：终卡槽烘焙齿轮涂装旁调用。掩码外一字节不动；小缓冲早退保命
pub fn paint(buf: &mut [u32], buf_w: u32, buf_h: u32, accent: crate::ui::accent::AccentPair) {
    if buf_w < crate::termview::MARGIN_X * 2 + DEMO_GLYPH_PX
        || buf_h < crate::termview::MARGIN_Y * 2 + DEMO_GLYPH_PX
    {
        return;
    }
    let (hx, hy, hw, _) = hit_rect(buf_w);
    let (cx, cy) = (hx + hw / 2, hy + hw / 2); // 字形心 = 命中盒心（单源）
    paint_at(buf, buf_w, buf_h, cx, cy, accent);
}

/// 任意心位涂装（宪法 §六 样式唯一来源，与 gear::paint_at 同纪律）。
/// 烧瓶掩码（解析式轮廓描边，笔触厚 2.6 同 ft_eye）：
/// 圆底 = 圆环；三角身 = 颈底两角向圆底两侧张的两条肩线；
/// 细颈 = 两条竖线 + 顶缘一条横沿
pub fn paint_at(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    cx: u32,
    cy: u32,
    accent: crate::ui::accent::AccentPair,
) {
    let g = f64::from(DEMO_GLYPH_PX);
    let (fx, fy) = (f64::from(cx), f64::from(cy));
    let r_body = g * 0.30; // 圆底半径
    let ccy = fy + g * 0.14; // 圆心底心（偏下给颈让位）
    let neck_hw = g * 0.09; // 细颈半宽
    let neck_top = fy - g / 2.0 + 2.0; // 颈顶（盒顶内缩 2）
    let shoulder_y = fy - g * 0.18; // 肩线起点高（颈底）
    let th = 2.6f64; // 笔触厚度（ft_eye 同尺）
    let denom = (i64::from(DEMO_GLYPH_PX) - 1) * 2;
    let (x0, y0) = (cx - DEMO_GLYPH_PX / 2, cy - DEMO_GLYPH_PX / 2);
    // 肩线终点 = 圆底两侧切点（±55° 族）
    let (sx, sy) = (r_body * 0.92, r_body * 0.35);
    for py in y0..(y0 + DEMO_GLYPH_PX) {
        if py >= buf_h {
            break;
        }
        for px in x0..(x0 + DEMO_GLYPH_PX) {
            if px >= buf_w {
                break;
            }
            let dx = f64::from(px) + 0.5 - fx;
            let dy = f64::from(py) + 0.5 - fy;
            let ry = f64::from(py) + 0.5 - ccy;
            // 圆底环带
            let mut on = (dx.hypot(ry) - r_body).abs() < th;
            // 细颈两竖线 + 顶横沿
            if !on && dy >= neck_top - fy && dy <= shoulder_y - fy {
                on = (dx.abs() - neck_hw).abs() < th;
            }
            if !on && (dy - (neck_top - fy)).abs() < th && dx.abs() <= neck_hw + th {
                on = true; // 顶缘横沿
            }
            // 两条肩线（颈底角 → 圆底切点，点到线段距离）
            if !on {
                let (ax, ay) = (neck_hw, shoulder_y - fy);
                let (bx, by) = (sx, ccy - sy - fy);
                for sgn in [1.0f64, -1.0] {
                    let (ax, bx) = (ax * sgn, bx * sgn);
                    let (vx, vy) = (bx - ax, by - ay);
                    let t =
                        (((dx - ax) * vx + (dy - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
                    let (qx, qy) = (ax + t * vx, ay + t * vy);
                    if (dx - qx).hypot(dy - qy) < th {
                        on = true;
                        break;
                    }
                }
            }
            if !on {
                continue;
            }
            let i = (py * buf_w + px) as usize;
            let dst = buf[i];
            let c = crate::termview::ring_gradient_rgb(
                accent.c1,
                accent.c2,
                i64::from(px - x0),
                i64::from(py - y0),
                denom,
            );
            let (sr, sg, sb) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
            let (dr, dg, db) = (
                ((dst >> 16) & 0xFF) as f32,
                ((dst >> 8) & 0xFF) as f32,
                (dst & 0xFF) as f32,
            );
            let a = DEMO_ALPHA;
            let r = (dr * (1.0 - a) + sr as f32 * a) as u32;
            let g2 = (dg * (1.0 - a) + sg as f32 * a) as u32;
            let b = (db * (1.0 - a) + sb as f32 * a) as u32;
            buf[i] = (r << 16) | (g2 << 8) | b;
        }
    }
}
