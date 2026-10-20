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

/// 条高的**唯一位点**（NA0189 楼6 判据①）：dp 值只此一处，物理 px 一律经
/// bar_px(density) 算；Java 侧只报密度、不再自算（一端算两端取值）。密度
/// 未就位时的兜底档 = 3.0（1260 宽真机实拍档 → 132px）。
pub const BAR_DP: f64 = 44.0;
/// 兜底密度（桥未通时用；真机 3x）
pub const DENSITY_FALLBACK: f64 = 3.0;
/// 标题左内边距（物理 px）
pub const PAD_X: u32 = 28;
/// 「×」命中方块距条右/上下的边距（物理 px）
pub const CLOSE_MARGIN: u32 = 24;
/// 页面标题
pub const TITLE: &str = "全局页";

/// 顶条可见性**唯一判据**（BAR-276，MAIN0192 楼1088-1460）。
///
/// 两条渲染腿（CPU 的 `paint_over`、GLES 的 Over 槽烘焙）都必须调它，
/// 不许各自复制表达式——复制出来的第二份就是「抄真源」，会与实现分叉，
/// 而贴在测试里的副本更坏：它让钉测的是副本、不是产品代码（楼1460 悬铃）。
///
/// 语义：`web_on_top` 是 z 序末位派生量（**背景量**，空栈下按 BAR-214 的
/// 垫底设计恒为真）；真正的可见性还要叠加「Web 是否真在栈」——`top` 或
/// `covered` 其一为 Web。⇒ 空栈（两者皆 None）时返回 false，顶条不画。
pub fn bar_visible(
    web_on_top: bool,
    top: Option<crate::ai_presence::Panel>,
    covered: Option<crate::ai_presence::Panel>,
) -> bool {
    let is_web = |p: Option<crate::ai_presence::Panel>| p == Some(crate::ai_presence::Panel::Web);
    web_on_top && (is_web(top) || is_web(covered))
}
/// 标题/关闭墨色（近白，与页面深底拉开）
const INK: u32 = 0x00E6_E6EE;
/// 「×」笔触半径（物理 px）
const CLOSE_STROKE_R: i64 = 5;

/// 条高（物理 px）＝density × BAR_DP，四舍五入、下限 1——**全进程唯一算式**。
pub fn bar_px(density: f64) -> u32 {
    let v = (BAR_DP * density).round();
    if !v.is_finite() || v < 1.0 {
        return 1;
    }
    v.min(f64::from(u32::MAX)) as u32
}

/// 运行期条高（单源读数，bar_px 算出的那份；线程安全静态）
pub fn bar_h() -> u32 {
    BAR_H_PX.load(std::sync::atomic::Ordering::Relaxed)
}

/// 运行期条高设定：桥报来密度，这里只调 bar_px 一次并落库——Java 不再算。
pub fn set_from_density(density: f64) -> u32 {
    let h = bar_px(density);
    BAR_H_PX.store(h, std::sync::atomic::Ordering::Relaxed);
    h
}

/// 进程内条高（px）；初始＝兜底密度档，桥通后用 set_from_density 覆写
static BAR_H_PX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(132); // = bar_px(DENSITY_FALLBACK)，见钉

/// 「×」命中方块边长（物理 px）：条高 − 上下边距×2
pub fn close_side(bar_h: u32) -> u32 {
    bar_h.saturating_sub(CLOSE_MARGIN * 2)
}

/// 「×」命中方块（x, y, w, h）：条内右端，唯一命中区（无平台占用带的基准形）
pub fn close_rect(buf_w: u32, bar_h: u32) -> (u32, u32, u32, u32) {
    close_rect_in(buf_w, bar_h, 0, 0)
}

/// 带平台占用带的命中盒（NA0189 楼22/23）：右界让出右缘手势条、纵原点
/// 从状态栏下缘起——条与命中盒都不许落进系统吃触摸的那两条带。
pub fn close_rect_in(
    buf_w: u32,
    bar_h: u32,
    inset_top: u32,
    inset_right: u32,
) -> (u32, u32, u32, u32) {
    let side = close_side(bar_h);
    (
        avail_w(buf_w, inset_right).saturating_sub(CLOSE_MARGIN + side),
        inset_top + CLOSE_MARGIN,
        side,
        side,
    )
}

/// 「×」命中方块（x, y, w, h）：条内右端，唯一命中区。
/// off = 该帧 Web 页位移（段②，见 bar_offset_px）——条随页面走，命中盒同走。
pub fn close_rect_off(buf_w: u32, bar_h: u32, off: i32) -> (i32, i32, u32, u32) {
    let (rx, ry, w, h) = close_rect_in(buf_w, bar_h, inset_top(), inset_right());
    (rx as i32 + off, ry as i32, w, h)
}

/// 命中判定：只算右端「×」方块（条内其余位置与条外一律不中）。
/// 与 close_rect_off 同一份 off——眼手同尺（段②判据②的一半）。
pub fn hit_close_off(x: f64, y: f64, buf_w: u32, bar_h: u32, off: i32) -> bool {
    let (rx, ry, rw, rh) = close_rect_off(buf_w, bar_h, off);
    x >= f64::from(rx)
        && x < f64::from(rx + rw as i32)
        && y >= f64::from(ry)
        && y < f64::from(ry + rh as i32)
}

/// 【段②位移单源·唯一取整点】Web 页位移（物理 px，向右为正）：
/// seam 采样出的浮点 off 在这里**只取整一次**，渲染顶条、命中「×」盒、
/// 桥下发给 Java 窗三处一律吃这个整数——三处各自 round 就会各抖一像素
/// （NA0189 楼6 判据②「位移单源无抖动」）。取整规则：四舍五入、NaN 兜底 0。
pub fn bar_offset_px(web_off: f32) -> i32 {
    if web_off.is_finite() {
        web_off.round() as i32
    } else {
        0
    }
}

/// 平台占用带（NA0189 楼22 定罪：条原先画在系统状态栏带里、× 又压 vivo
/// 右缘手势条——两处都系统窗吃触摸，点按永远到不了 na）。**条一律画在
/// 系统不占用的带里**：纵向从状态栏下缘起，横向右端让出手势条宽。
/// 两个值都是平台事实（Java 报），布局算式仍只在本模块一处。
static INSET_TOP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static INSET_RIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 设平台占用带（状态栏高、右缘手势条宽；桥在开页时问一次）
pub fn set_insets(top: u32, right: u32) {
    INSET_TOP.store(top, std::sync::atomic::Ordering::Relaxed);
    INSET_RIGHT.store(right, std::sync::atomic::Ordering::Relaxed);
}

/// 当前状态栏高（条带纵原点＝它，不再从 0 起）
pub fn inset_top() -> u32 {
    INSET_TOP.load(std::sync::atomic::Ordering::Relaxed)
}

/// 当前右缘手势条宽（条带可用宽＝屏宽 − 它）
pub fn inset_right() -> u32 {
    INSET_RIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

/// 条带可用宽（右端让出手势条）——「×」盒与标题排布的右界
pub fn avail_w(w: u32, inset_right: u32) -> u32 {
    w.saturating_sub(inset_right)
}

/// 段②缝目标值语义（基础层）：在栈 = 0 靠泊；不在栈 = +屏宽 屏外右缘。
/// 三个消费点（渲染顶条／命中盒／桥下发）都用它算 target——目标值也只有
/// 一份，避免「谁把符号写反」这类各算一份。
pub fn web_off_target(in_stack: bool, w: u32) -> f32 {
    if in_stack { 0.0 } else { w as f32 }
}

/// 条区触摸裁决（BAR-260 出口红修复，NA0189 楼19–20 一手判据）：Web 在顶
/// 时**先判条区、再吞其余**——顺序反了（先吞后判）就永远不进弹栈。
/// 返回 None = 不归 Web 管（交后续路由）；Dismiss = 条上「×」；Swallow =
/// Web 在顶的其余触摸（页体 Java 侧吃，到不了 Rust 的只剩条带与手势残段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebTouch {
    Dismiss,
    Swallow,
}

/// 纯裁决：web_top 才接管；只有 Started 能命中；命中「×」即弹栈，其余吞。
pub fn web_touch_decide(web_top: bool, phase_started: bool, in_close: bool) -> Option<WebTouch> {
    if !web_top {
        return None;
    }
    if phase_started && in_close {
        return Some(WebTouch::Dismiss);
    }
    Some(WebTouch::Swallow)
}

/// BACK 键裁决（BAR-260 出口重修第二件，NA0189 楼31/32 清和裁）：浮窗还
/// 挂着时 BACK 先关页、**不许直接 finish Activity**（后者会在浮窗未摘时销毁
/// 宿主 → WindowLeaked）。纯函数，宿主可判卷。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackAction {
    /// 关全局页（回解析页），消费掉这次 BACK
    ClosePage,
    /// 交原路（无页可关）
    Super,
}

pub fn back_action(web_open: bool) -> BackAction {
    if web_open {
        BackAction::ClosePage
    } else {
        BackAction::Super
    }
}

/// 桥下发限频（BAR-260 出口第五件，NA0189 楼61：括号原文定乙＝已投递输入
/// 未在 5s 内处理完；凶手是**逐帧搬窗把 UI 线程堵住**）。判据两条：值变了
/// 才发（bridge_should_send）＋**距上次下发不足 interval 就跳帧**。
/// 跳帧不丢最新值：Java 侧留「最新值覆盖」（迟到帧覆盖，不排队）。
pub const OFFSET_MIN_INTERVAL_MS: u64 = 33;

/// 限频判据（纯函数，宿主可判卷）：变了 + 距上次 ≥ interval 才发。
pub fn offset_should_send(last_off: i32, cur_off: i32, last_at_ms: u64, now_ms: u64) -> bool {
    if !bridge_should_send(last_off, cur_off) {
        return false;
    }
    if last_at_ms == 0 {
        return true; // 首次（尚无下发时刻）：只要求变了，别让限频把首帧吃掉
    }
    now_ms.saturating_sub(last_at_ms) >= OFFSET_MIN_INTERVAL_MS
}

/// 桥下发判据（防抖）：整数位移与上次相同就不发——同一帧重复下发会让
/// Java 侧 updateViewLayout 反复触发布局，画面抖。纯函数，供桥与考题共用。
pub fn bridge_should_send(last: i32, cur: i32) -> bool {
    last != cur
}

/// 命中判定：只算右端「×」方块（条内其余位置与条外一律不中）
pub fn hit_close(x: f64, y: f64, buf_w: u32, bar_h: u32) -> bool {
    hit_close_off(x, y, buf_w, bar_h, 0)
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
    bar_h: u32,
    off: i32,
) {
    if buf_w == 0 || buf_h == 0 {
        return;
    }
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    // 条带底（深底与各页同色）+ 底缘 2px accent 双色线（左 c1 右 c2）。
    // off = 本帧位移（段②单源）；纵原点从状态栏下缘起、横向止于可用带右界
    // （NA0189 楼22/23：条不许落进系统状态栏带与右缘手势条——那里系统吃触摸）
    let y0 = inset_top();
    let usable = avail_w(buf_w, inset_right());
    let (bx, bw) = shift(0, usable, off);
    frame.fill_rect(bx, y0, bw, bar_h, CARD_PAGE_BG);
    let div_y = y0 + bar_h.saturating_sub(2);
    let half = usable / 2;
    let (l1x, l1w) = shift(0, half, off);
    frame.fill_rect(l1x, div_y, l1w, 2, accent.c1);
    let (l2x, l2w) = shift(half, usable - half, off);
    frame.fill_rect(l2x, div_y, l2w, 2, accent.c2);
    // 左端标题：网格引擎定尺（与页内文字同尺），纵向在条带内居中
    let (cell_w, cell_h) = term.cell_size();
    if cell_w > 0 {
        let (items, _) = term.measure_items_grid(TITLE);
        let y_top = i64::from(y0 + bar_h.saturating_sub(cell_h) / 2);
        let max_w = usable.saturating_sub(PAD_X * 2 + close_side(bar_h)).max(1);
        term.draw_grid_text_left(
            &mut frame,
            &items,
            i64::from(PAD_X) + i64::from(off),
            y_top,
            cell_w,
            max_w,
            INK,
            0,
            None,
        );
    }
    // 右端「×」：细框 + 两段对角笔画（命中盒 = 视觉盒，眼手同尺）
    let (cx, cy, cs, _) = close_rect_off(buf_w, bar_h, off);
    if cx >= 0 {
        paint_close_box(&mut frame, cx as u32, cy as u32, cs, accent.c2);
    }
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

/// 位移后矩形（越界裁剪）：三条带与标题/「×」共用，防负坐标进 u32。
fn shift(x: u32, w: u32, off: i32) -> (u32, u32) {
    let x0 = x as i64 + i64::from(off);
    let x1 = x0 + i64::from(w);
    let x0c = x0.max(0);
    let x1c = x1.max(0);
    (
        x0c.min(i64::from(u32::MAX)) as u32,
        (x1c - x0c).min(i64::from(u32::MAX)) as u32,
    )
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
