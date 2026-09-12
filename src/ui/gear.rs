//! ui/gear.rs — 设置钮控件（2026-09-12 用户拍板：配置池卡入口 = 终端页
//! 右上角设置按钮，**唯一入口**——配置页退出手势槽，左滑槽冻结留给
//! 浏览器卡 SPKE-web 解冻）。
//!
//! 分层：几何+命中 = 本册纯逻辑（A 档考题 tests/gear_spec.rs）；涂装挂
//! termview::paint_term_card_chrome 终卡槽内（面板靠泊时终卡槽整层隐
//! ——「按钮只在裸终端页出现」的可见性语义白拿，零新逻辑）；触发 = 壳层
//! 点按 → ai_presence.summon_panel(Config)，入栈动作与旧手势同一路。
//!
//! 字形 = 程序化齿轮（极坐标解析掩码：环带 + 八齿 + 中孔），与 orb 同族
//! 工艺——不赌字体里有没有 ⚙ 字形。硬边像素风，与内嵌像素字体同美学。

/// 命中盒边长（px）：字形盒外扩 22px 触控补偿（2026-09-12 随字形放大同步）
pub const GEAR_HIT_PX: u32 = 116;
/// 字形盒边长（px）：两行终端行高（CELL_H 36×2，2026-09-12 用户拍板
/// 「放大到两行高」——原 52 约一行半被读成一行，不够醒目）
pub const GEAR_GLYPH_PX: u32 = 72;
/// 齿轮墨色（碳灰族低饱和，与 TERM_FRAME_C2 同族；终卡涂装族待 token
/// 化是既有债，本控件随族不单独立项）
pub const GEAR_INK: u32 = 0x008A_93A3;
/// 不透明度：浮在终端文字上要不抢读（光球「看得见又不挡」同原则）
const GEAR_ALPHA: f32 = 0.85;

/// 命中盒（x, y, w, h）：贴终卡内右上——字形右缘距卡内右缘 6px、顶缘
/// 距顶带 6px；命中盒 = 字形盒中心外扩到 GEAR_HIT_PX（触控补偿）。
/// 几何单源：涂装与命中同读此函数
pub fn hit_rect(buf_w: u32) -> (u32, u32, u32, u32) {
    let gx1 = buf_w.saturating_sub(crate::termview::MARGIN_X + 6);
    let gx0 = gx1.saturating_sub(GEAR_GLYPH_PX);
    let gy0 = crate::termview::MARGIN_Y + 6;
    let cx = gx0 + GEAR_GLYPH_PX / 2;
    let cy = gy0 + GEAR_GLYPH_PX / 2;
    (
        cx.saturating_sub(GEAR_HIT_PX / 2),
        cy.saturating_sub(GEAR_HIT_PX / 2),
        GEAR_HIT_PX,
        GEAR_HIT_PX,
    )
}

/// 命中判定（纯函数）：点在命中盒内即中
pub fn hit(x: f64, y: f64, buf_w: u32) -> bool {
    let (hx, hy, hw, hh) = hit_rect(buf_w);
    x >= f64::from(hx) && x < f64::from(hx + hw) && y >= f64::from(hy) && y < f64::from(hy + hh)
}

/// 涂装：终卡槽烘焙末尾调用（画在环之上）。直接像素级 alpha 混合——
/// 掩码外一字节不动（终端文字透出来）。小缓冲早退保命
pub fn paint(buf: &mut [u32], buf_w: u32, buf_h: u32) {
    if buf_w < crate::termview::MARGIN_X * 2 + GEAR_GLYPH_PX
        || buf_h < crate::termview::MARGIN_Y * 2 + GEAR_GLYPH_PX
    {
        return;
    }
    let (hx, hy, hw, _) = hit_rect(buf_w);
    let (cx, cy) = (hx + hw / 2, hy + hw / 2); // 字形心 = 命中盒心（单源）
    let r_out = f64::from(GEAR_GLYPH_PX) / 2.0 - 1.0;
    let (ir, ig, ib) = (
        (GEAR_INK >> 16) & 0xFF,
        (GEAR_INK >> 8) & 0xFF,
        GEAR_INK & 0xFF,
    ); // XRGB 0x00RRGGBB
    let (x0, y0) = (cx - GEAR_GLYPH_PX / 2, cy - GEAR_GLYPH_PX / 2);
    for py in y0..(y0 + GEAR_GLYPH_PX) {
        if py >= buf_h {
            break;
        }
        for px in x0..(x0 + GEAR_GLYPH_PX) {
            if px >= buf_w {
                break;
            }
            let dx = px as f64 + 0.5 - f64::from(cx);
            let dy = py as f64 + 0.5 - f64::from(cy);
            let rn = dx.hypot(dy) / r_out; // 归一半径
            if rn > 1.0 {
                continue;
            }
            // 掩码：环带 0.34..0.70 连续；齿带 0.70..1.0 按八齿角域；
            // 中孔 <0.34 全透
            let on = if rn <= 0.70 {
                rn >= 0.34
            } else {
                let sector = (dx.atan2(-dy) / std::f64::consts::TAU + 0.5) * 8.0;
                sector.fract() < 0.5 // 每齿占半扇区（0°=正上，顺时针）
            };
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
            let a = GEAR_ALPHA;
            let r = (dr * (1.0 - a) + ir as f32 * a) as u32;
            let g = (dg * (1.0 - a) + ig as f32 * a) as u32;
            let b = (db * (1.0 - a) + ib as f32 * a) as u32;
            buf[i] = (r << 16) | (g << 8) | b;
        }
    }
}
