//! tests/web_page_spec.rs — 全局页（第七公民 Panel::Web）最小可跑件考题
//! （BAR-260／NA0189 楼1；白露 MAIN0175 楼23 形态定义、楼25 点头）。
//!
//! 判卷维度：
//! - 入口两跳：终端左滑→解析页；解析页再左滑→全局页（不是第三条滑槽）
//! - 退出唯一：右滑空操作；顶部原生条右端「×」命中 → 弹栈回解析页
//! - 顶条常驻几何：条高 > 0、× 命中盒在条内右端、涂装/命中不读页面状态
//! - 接线源码锚：弹卡几何与自动推开派发已删，JNI 注册桥保留

use kfm_na::ai_presence::{AiPresenceState, Panel};
use kfm_na::ui::web_page;

const W: u32 = 1260;
const H: u32 = 2800;

fn new_state() -> AiPresenceState {
    let ai = AiPresenceState::new();
    ai.set_bounds(W, H, 0);
    ai
}

fn read_src(rel: &str) -> String {
    let root = env!("CARGO_MANIFEST_DIR");
    std::fs::read_to_string(format!("{root}/{rel}")).unwrap_or_else(|e| panic!("读不到 {rel}: {e}"))
}

/// 取函数体（从首个 `{` 到配对 `}`；源码锚用，不求完美词法）
fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src
        .find(sig)
        .unwrap_or_else(|| panic!("源码里找不到 {sig}"));
    let open = start + src[start..].find('{').expect("函数无左括号");
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..=open + i];
                }
            }
            _ => {}
        }
    }
    panic!("{sig} 函数体未闭合");
}

// ---- 钉 1：入口两跳 ----

#[test]
fn spec_bar260_入口_解析页再左滑() {
    // 第一跳：终端左滑 → 解析页
    let ai = new_state();
    ai.swipe_left();
    let s = ai.snap(0);
    assert_eq!(s.top, Some(Panel::Parser), "终端左滑 = 召唤解析页");
    assert_eq!(s.covered, None, "第一跳只有解析页");
    // 第二跳：解析页再左滑 → 全局页（解析页留栈底，供「×」弹回）
    ai.swipe_left();
    let s = ai.snap(0);
    assert_eq!(s.top, Some(Panel::Web), "解析页左滑 = 召唤全局页");
    assert_eq!(s.covered, Some(Panel::Parser), "解析页留栈底");

    // 其余栈顶：左滑仍召唤解析页，不是全局页（全局页只从解析页进）
    let ai2 = new_state();
    ai2.swipe_left();
    ai2.swipe_right(); // 回终端
    ai2.swipe_left();
    let s = ai2.snap(0);
    assert_eq!(s.top, Some(Panel::Parser), "终端顶左滑 = 解析页");
    assert_ne!(s.top, Some(Panel::Web), "终端左滑不许一跳直达全局页");

    // 配置顶左滑 = 空操作（不占召唤滑槽，也不进全局页）
    let ai3 = new_state();
    ai3.summon_panel(Panel::Config);
    ai3.swipe_left();
    assert_eq!(ai3.snap(0).top, Some(Panel::Config), "配置顶左滑空操作");
}

// ---- 钉 2：退出只走点「×」 ----

#[test]
fn spec_bar260_退出_只走点叉() {
    let ai = new_state();
    ai.swipe_left();
    ai.swipe_left();
    assert_eq!(ai.snap(0).top, Some(Panel::Web));

    // 右滑必须空操作（不给第二条退路）
    let before = ai.snap(0);
    ai.swipe_right();
    let after = ai.snap(0);
    assert_eq!(after.top, Some(Panel::Web), "全局页右滑空操作");
    assert_eq!(
        (after.top, after.covered),
        (before.top, before.covered),
        "右滑栈不变"
    );
    assert_eq!(after.covered, Some(Panel::Parser), "解析页仍在栈底");

    // 「×」命中：只算右端方块
    let (rx, ry, rw, rh) = web_page::close_rect(W);
    let (cx, cy) = (
        f64::from(rx) + f64::from(rw) / 2.0,
        f64::from(ry) + f64::from(rh) / 2.0,
    );
    assert!(web_page::hit_close(cx, cy, W), "× 中心必中");
    assert!(!web_page::hit_close(10.0, cy, W), "条左端不中");
    assert!(!web_page::hit_close(
        cx,
        f64::from(web_page::BAR_H) + 10.0,
        W
    ));

    // 命中 → 弹栈回解析页（唯一动作）
    assert!(ai.dismiss_top(Panel::Web), "× = 收顶");
    let s = ai.snap(0);
    assert_eq!(s.top, Some(Panel::Parser), "× 弹栈回解析页");
    assert_eq!(s.covered, None);
}

// ---- 钉 3：顶条常驻几何 ----

#[test]
fn spec_bar260_顶条常驻几何() {
    const {
        assert!(web_page::BAR_H > 0, "条高常量必须为正");
    }
    // 44dp @3x 等价值（1260 宽真机）：132 物理 px
    assert_eq!(web_page::BAR_H, 132, "条高 = 44dp @3x");
    let (rx, ry, rw, rh) = web_page::close_rect(W);
    assert!(ry < web_page::BAR_H, "× 盒顶在条内");
    assert!(ry + rh <= web_page::BAR_H, "× 盒底在条内（不越条）");
    assert!(rx > W / 2, "× 盒在右半屏");
    assert!(rx + rw <= W, "× 盒不出屏");
    assert_eq!(rx + rw, W - web_page::CLOSE_MARGIN, "× 盒贴右端内边距");
    assert!(web_page::hit_close(f64::from(rx), f64::from(ry), W));
    assert!(!web_page::hit_close(f64::from(rx + rw), f64::from(ry), W));

    // 常驻纪律源码锚：涂装/命中不许出现页面状态标识（白屏/加载/滚动都照画）
    let src = read_src("src/ui/web_page.rs").to_lowercase();
    for needle in ["webview", "loadurl", "web_status", "web-status", "mweb"] {
        assert!(
            !src.contains(needle),
            "web_page.rs 出现页面状态标识「{needle}」——常驻纪律破了"
        );
    }
}

// ---- 钉 4：弹卡整删守卫（源码锚） ----

#[test]
fn spec_bar260_弹卡整删守卫() {
    // ① Java 侧弹卡几何已删，整页几何在位
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    assert!(!java.contains("* 3 / 5"), "弹卡几何「屏高 3/5」必须删净");
    assert!(!java.contains("heightPixels * 3"), "弹卡高度公式残留");
    assert!(!java.contains("刻意不全屏"), "弹卡注释尸体残留");
    assert!(java.contains("lp.y = bar"), "整页几何：纵向偏移让出顶条");
    assert!(java.contains("44f * dm.density"), "条高 44dp 与 Rust 同源");

    // ② gate：web-req 只落 pending，不自动推 UI；桥入口在
    let gate = read_src("src/gate.rs");
    let body = fn_body(&gate, "pub fn web_req_check(");
    assert!(body.contains("PENDING_WEB"), "web-req 必须落 pending");
    assert!(
        !body.contains("WEB_HOOK") && !body.contains("open_web"),
        "web_req_check 不许自动推 UI（自动派发整删）"
    );
    assert!(
        gate.contains("pub fn open_web_pending("),
        "进入全局页的加载入口"
    );
    assert!(gate.contains("pub fn close_web("), "退出全局页的收起入口");
    assert!(gate.contains("register_web_hook"), "桥注册必须保留");

    // ③ 壳层：JNI 注册桥保留（删过头 = 全局页没有落地通道）
    let app = read_src("src/android_app.rs");
    assert!(
        app.contains("register_web_hook")
            && app.contains("startWebViewFromGate")
            && app.contains("open_web_pending")
            && app.contains("close_web"),
        "android_app 必须保留 JNI 桥并在进出栈时调用"
    );
}
