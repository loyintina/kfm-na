//! tab_bar_spec.rs — 配置卡标签栏考题（主题宪法 §四 首行布局区，A 档）。
//!
//! 钉的条款：标签行 = 1 格高、落点咬格（几何全 CELL 整数倍）、选中态 =
//! 可移动光标框（弹簧重定基+收敛贴死）、横滑区（pan clamp 不出界）、
//! 标签行命中与面板手势的仲裁边界（in_row/hit 是眼手同尺单源）。
//! 变异抽检（已实测咬人）：
//! ① TAB_GAP 改 0 → 钉①第二标签 x 坐标红；
//! ② pan 删 clamp 上界 → 钉④ scroll>0 红；
//! ③ select 删弹簧重定基（from 恒 0）→ 钉③「从当前位置续弹」红；
//! ④ text_cells 宽字改 1 格 → 钉①/钉⑥标签宽红。

use kfm_na::termview::{CELL_H, CELL_W};
use kfm_na::ui::tab_bar::{TabBar, content_origin, in_row, text_cells};

/// 钉①：咬格几何钉——单标签/双标签的矩形全按格钉死（原点、宽、间距）
#[test]
fn spec_tab_bar_咬格几何钉() {
    let (ox, oy) = content_origin();
    assert_eq!(ox, 43, "内容原点 x = 环左内缘(16+9) + 1 格(18)");
    assert_eq!(oy, 55, "内容原点 y = 环上内缘(16+3) + 1 格(36)");

    let bar = TabBar::new(&["系统管理"], 720);
    let r = &bar.tab_rects()[0];
    assert_eq!(
        (r.x, r.y, r.w, r.h),
        (43i64, 55i64, 10 * CELL_W, CELL_H * 2),
        "系统管理 = 8 文字格 + 2  padding 格，行高 2 格"
    );

    let bar2 = TabBar::new(&["系统管理", "API"], 720);
    let r2 = &bar2.tab_rects();
    assert_eq!(
        r2[1].x,
        43i64 + (10 * CELL_W + CELL_W) as i64,
        "第二标签 = 前一标签尾 + 1 格间距"
    );
    assert_eq!(r2[1].w, 5 * CELL_W, "API = 3 文字格 + 2 padding 格");
}

/// 钉②：行带命中钉（手势仲裁边界）——行内/行外/标签间隙三路
#[test]
fn spec_tab_bar_行带命中钉() {
    assert!(in_row(55.0) && in_row(126.9), "行带内（2 格高）");
    assert!(
        !in_row(54.9) && !in_row(127.0),
        "行带外（上 1px / 下缘排他）"
    );

    let bar = TabBar::new(&["系统管理"], 720);
    assert_eq!(bar.hit(50.0, 60.0), Some(0), "标签体命中");
    assert_eq!(
        bar.hit(43.0 + 180.0 + 5.0, 60.0),
        None,
        "标签尾后间隙不命中"
    );
    assert_eq!(bar.hit(50.0, 130.0), None, "行带下不命中");
}

/// 钉③：select 弹簧钉——目标 = 新标签 x；从当前位置重定基；收敛贴死
#[test]
fn spec_tab_bar_select弹簧钉() {
    let mut bar = TabBar::new(&["系统管理", "API"], 720);
    assert_eq!(bar.cursor_x(0), 43.0, "初态光标在标签 0");
    bar.select(1, 1000);
    assert_eq!(bar.selected(), 1);
    assert_eq!(bar.cursor_x(1000), 43.0, "切换瞬间从当前位置续弹（不跳变）");
    let mid = bar.cursor_x(1150);
    assert!(
        (mid - 43.0).abs() > 1.0,
        "弹簧途中必须离开起点（过冲也算在路上）"
    );
    assert_eq!(bar.cursor_x(1600), 241.0, "600ms 兜底贴死目标");
    // 途中再切换 = 从途中位置重定基（不闪回起点）
    bar.select(0, 1150);
    let back = bar.cursor_x(1150);
    assert!(
        (back - 43.0).abs() > 1.0,
        "途中反向 = 当前位置重定基（不回起点）"
    );
}

/// 钉④：pan clamp 钉——可滚范围 [-（内容宽-视口宽）, 0]，无溢出不滚
#[test]
fn spec_tab_bar_pan_clamp钉() {
    let mut bar = TabBar::new(&["系统管理", "API", "文件树管理器"], 200);
    let content_w = bar.content_w();
    assert!(content_w > 200, "夹具前提：内容必须溢出视口");
    bar.pan(-10000.0);
    assert_eq!(
        bar.scroll_px(),
        -((content_w + 43 - 200) as i64),
        "左滚到底 = 内容右缘（含原点 43）对齐视口右缘"
    );
    bar.pan(10000.0);
    assert_eq!(bar.scroll_px(), 0, "右滚回家即钳（不许正滚）");

    let mut fit = TabBar::new(&["系统管理"], 720);
    fit.pan(-100.0);
    assert_eq!(fit.scroll_px(), 0, "无溢出时 pan 是空操作");
}

/// 钉⑤：select 可见性钉——窄视口选中屏外标签，scroll 自动把它拉进视口
#[test]
fn spec_tab_bar_select可见性钉() {
    let mut bar = TabBar::new(&["系统管理", "API", "文件树管理器"], 300);
    bar.select(2, 0);
    let r = bar.tab_rects()[2].clone();
    assert!(
        r.x >= 0 && r.x + r.w as i64 <= 300,
        "选中标签必须完整落进视口（{r:?}）"
    );
}

/// 钉⑥：文字格宽钉——CJK 宽字 2 格，ASCII 1 格（量宽与画字同尺）
#[test]
fn spec_tab_bar_文字格宽钉() {
    assert_eq!(text_cells("系统管理"), 8);
    assert_eq!(text_cells("API"), 3);
    assert_eq!(text_cells("A系B"), 4);
    assert_eq!(text_cells(""), 0);
}

/// 钉⑦：常量与视图钉——行高/padding/间距常量是几何的实体（改动即红）；
/// 内容视口宽公式 = 屏宽 − 原点 − 右内缘 37
#[test]
fn spec_tab_bar_常量与视口钉() {
    use kfm_na::ui::tab_bar::{TAB_GAP, TAB_PAD_X, TAB_ROW_H, content_viewport_w};
    assert_eq!(
        TAB_ROW_H,
        CELL_H * 2,
        "标签行 = 2 格（§七，2026-09-12 实测拍板）"
    );
    assert_eq!(TAB_PAD_X, CELL_W, "标签文字两侧各 1 格");
    assert_eq!(TAB_GAP, CELL_W, "标签间距 1 格");
    assert_eq!(
        content_viewport_w(720),
        720 - 43 - 37,
        "视口宽 = 屏宽 − 原点 43 − 右内缘 37"
    );
    // 字号必须装得进行高（termview 侧标定值，跨模块咬合）
    assert!(kfm_na::termview::TAB_TEXT_PX <= TAB_ROW_H as f32);
}

/// 钉⑧：快照同源钉——snap 是涂装的唯一读数口，必须与状态逐字段咬合；
/// rects_of 自由函数与 tab_rects 同一份几何（眼手同尺）
#[test]
fn spec_tab_bar_快照同源钉() {
    use kfm_na::ui::tab_bar::rects_of;
    let mut bar = TabBar::new(&["系统管理", "API"], 720);
    bar.select(1, 500);
    let snap = bar.snap(500);
    assert_eq!(snap.tabs, bar.tabs().to_vec(), "快照池名表 = 状态池名表");
    assert_eq!(snap.selected, bar.selected());
    assert_eq!(snap.scroll_px, bar.scroll_px());
    assert_eq!(snap.cursor_x, bar.cursor_x(500));
    let rects = bar.tab_rects();
    assert_eq!(
        snap.cursor_x, rects[0].x as f32,
        "select 瞬间光标还在旧标签（弹簧起点）"
    );
    assert_eq!(
        bar.cursor_target(),
        rects[1].x as f32,
        "弹簧终点 = 新标签视口 x"
    );
    assert_eq!(
        rects_of(&snap.tabs, snap.scroll_px),
        bar.tab_rects(),
        "自由函数与状态几何同源"
    );
}

/// 钉⑨：视口放宽钳钉——窄视口滚到底再放宽视口，scroll 必须钳回新下限
///（内容右缘贴住视口右缘，不许留空洞；收窄方向 min_scroll 只会更负，
/// 不触发钳——几何推定，钉住的是放宽方向）
#[test]
fn spec_tab_bar_视口放宽钳钉() {
    let mut bar = TabBar::new(&["系统管理", "API", "文件树管理器"], 300);
    bar.pan(-1000.0);
    assert_eq!(
        bar.scroll_px(),
        300 - 43 - bar.content_w() as i64,
        "夹具前提：窄视口滚到底（内容右缘贴视口右缘）"
    );
    bar.set_viewport_w(500);
    assert_eq!(
        bar.scroll_px(),
        500 - 43 - bar.content_w() as i64,
        "放宽后 scroll 钳到新下限"
    );
}

/// 钉⑩：共享句柄钉（D9 同源）——注册后 gate 侧拿得到同一份
#[test]
fn spec_tab_bar_共享句柄钉() {
    use kfm_na::ui::tab_bar::{register_tab_bar, tab_bar_handle};
    use std::sync::{Arc, Mutex};
    let bar = Arc::new(Mutex::new(TabBar::new(&["系统管理"], 720)));
    register_tab_bar(bar.clone());
    let h = tab_bar_handle().expect("注册后句柄必须在");
    // h 与 bar 是同一个 Arc<Mutex>：两把锁必须分时取，同表达式双锁 = 死锁
    let h_tabs = h.lock().unwrap().tabs().to_vec();
    let bar_tabs = bar.lock().unwrap().tabs().to_vec();
    assert_eq!(h_tabs, bar_tabs, "句柄与注册同一份");
}
