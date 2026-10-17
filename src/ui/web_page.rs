//! ui/web_page.rs — 全局页顶部原生条（BAR-260／NA0189 楼1，白露
//! MAIN0175 楼23 形态定义、楼25 点头）。
//!
//! 页体由 Java 侧整页承载（屏高 − 条高，纵向偏移 = 条高）；Rust 只画
//! 恒驻顶部的这一行：左端页面标题文字、右端「×」。条高常量 = 44dp
//! 等价值（1260 宽 3x 真机 = 132 物理 px），Java 侧同源取 44dp。
//!
//! 常驻纪律：绘制与命中只吃条几何与 accent，**不读页面加载/滚动/白屏
//! 任何状态**——页面白屏、超时、崩溃都照画照中。本条是全局页唯一退出
//! 口：命中「×」→ 弹栈回解析页 + 通知 Java 收起页体。

use crate::termview::Frame;
use crate::ui::accent::{AccentPair, CARD_PAGE_BG};

/// 条高（物理 px）：44dp @3x（1260 宽真机）；与 MainActivity 的
/// 44dp 取值同源（跨语言两处常量，改一处必改另一处）
pub const BAR_H: u32 = 132;
/// 标题左内边距（物理 px）
pub const PAD_X: u32 = 28;
/// 「×」命中方块距条右/上下的边距（物理 px）
pub const CLOSE_MARGIN: u32 = 24;
/// 「×」命中方块边长（物理 px）
pub const CLOSE_SIDE: u32 = BAR_H - CLOSE_MARGIN * 2;
/// 页面标题
pub const TITLE: &str = "全局页";
/// 标题/关闭墨色（近白，与页面深底拉开）
const INK: u32 = 0x00E6_E6EE;
/// 「×」笔触半径（物理 px）
const CLOSE_STROKE_R: i64 = 5;

/// 「×」命中方块（x, y, w, h）：条内右端，唯一命中区
pub fn close_rect(buf_w: u32) -> (u32, u32, u32, u32) {
    (
        buf_w.saturating_sub(CLOSE_MARGIN + CLOSE_SIDE),
        CLOSE_MARGIN,
        CLOSE_SIDE,
        CLOSE_SIDE,
    )
}

/// 命中判定：只算右端「×」方块（条内其余位置与条外一律不中）
pub fn hit_close(x: f64, y: f64, buf_w: u32) -> bool {
    let (rx, ry, rw, rh) = close_rect(buf_w);
    x >= f64::from(rx) && x < f64::from(rx + rw) && y >= f64::from(ry) && y < f64::from(ry + rh)
}

/// 顶部原生条涂装（只画条带；条外一字节不动）。跟随 page 模块惯例：
/// 深底 + accent 底缘线，标题走网格文字引擎（实例格随 pinch），「×」为
/// 两段斜线笔画。
pub fn paint(
    term: &crate::termview::TermView,
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    accent: AccentPair,
) {
    if buf_w == 0 || buf_h == 0 {
        return;
    }
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    // 条带底（深底与各页同色）+ 底缘 2px accent 双色线（左 c1 右 c2）
    frame.fill_rect(0, 0, buf_w, BAR_H, CARD_PAGE_BG);
    let div_y = BAR_H.saturating_sub(2);
    let half = buf_w / 2;
    frame.fill_rect(0, div_y, half, 2, accent.c1);
    frame.fill_rect(half, div_y, buf_w - half, 2, accent.c2);
    // 左端标题：网格引擎定尺（与页内文字同尺），纵向在条带内居中
    let (cell_w, cell_h) = term.cell_size();
    if cell_w > 0 {
        let (items, _) = term.measure_items_grid(TITLE);
        let y_top = i64::from(BAR_H.saturating_sub(cell_h) / 2);
        let max_w = buf_w.saturating_sub(PAD_X * 2 + CLOSE_SIDE).max(1);
        term.draw_grid_text_left(
            &mut frame,
            &items,
            i64::from(PAD_X),
            y_top,
            cell_w,
            max_w,
            INK,
            0,
            None,
        );
    }
    // 右端「×」：细框 + 两段对角笔画（命中盒 = 视觉盒，眼手同尺）
    let (cx, cy, cs, _) = close_rect(buf_w);
    paint_close_box(&mut frame, cx, cy, cs, accent.c2);
}

/// 「×」方块涂装：1px accent 细框 + 中心两段斜线
fn paint_close_box(frame: &mut Frame<'_>, x: u32, y: u32, side: u32, edge: u32) {
    if side < 8 {
        return;
    }
    // 细框（上下左右各 1px；accent c2 压暗到 60% 与深底混）
    let edge_dim = dim(edge, 0.55);
    frame.fill_rect(x, y, side, 1, edge_dim);
    frame.fill_rect(x, y + side - 1, side, 1, edge_dim);
    frame.fill_rect(x, y, 1, side, edge_dim);
    frame.fill_rect(x + side - 1, y, 1, side, edge_dim);
    // 两段斜线：从盒内 1/4 到 3/4，笔触 CLOSE_STROKE_R
    let lo = i64::from(side / 4);
    let hi = i64::from(side - side / 4);
    let (x, y, side_i) = (i64::from(x), i64::from(y), i64::from(side));
    for t in lo..hi {
        for (dx, dy) in [(t, t), (t, side_i - t)] {
            for oy in -CLOSE_STROKE_R..=CLOSE_STROKE_R {
                for ox in -CLOSE_STROKE_R..=CLOSE_STROKE_R {
                    if ox * ox + oy * oy > CLOSE_STROKE_R * CLOSE_STROKE_R {
                        continue;
                    }
                    let px = x + dx + ox;
                    let py = y + dy + oy;
                    if px >= 0 && py >= 0 && px < i64::from(frame.w) && py < i64::from(frame.h) {
                        frame.blend_px(px as u32, py as u32, INK, 255);
                    }
                }
            }
        }
    }
}

/// accent 色压暗（α 混合到深底；只用于细框，保持「不抢标题」）
fn dim(c: u32, k: f32) -> u32 {
    let ch = |shift: u32| {
        let v = ((c >> shift) & 0xFF) as f32;
        let bg = ((CARD_PAGE_BG >> shift) & 0xFF) as f32;
        (bg + (v - bg) * k).round().clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}
