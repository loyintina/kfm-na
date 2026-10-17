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
    let bar = web_page::bar_px(3.0);
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
    let (rx, ry, rw, rh) = web_page::close_rect(W, bar);
    let (cx, cy) = (
        f64::from(rx) + f64::from(rw) / 2.0,
        f64::from(ry) + f64::from(rh) / 2.0,
    );
    assert!(web_page::hit_close(cx, cy, W, bar), "× 中心必中");
    assert!(!web_page::hit_close(10.0, cy, W, bar), "条左端不中");
    assert!(!web_page::hit_close(cx, f64::from(bar) + 10.0, W, bar));

    // 命中 → 弹栈回解析页（唯一动作）
    assert!(ai.dismiss_top(Panel::Web), "× = 收顶");
    let s = ai.snap(0);
    assert_eq!(s.top, Some(Panel::Parser), "× 弹栈回解析页");
    assert_eq!(s.covered, None);
}

// ---- 钉 3：顶条常驻几何 ----

#[test]
fn spec_bar260_顶条常驻几何() {
    // 条高单源（bar_px(3.0)＝132，1260 宽真机档）
    let bar = web_page::bar_px(3.0);
    let (rx, ry, rw, rh) = web_page::close_rect(W, bar);
    assert!(ry < bar, "× 盒顶在条内");
    assert!(ry + rh <= bar, "× 盒底在条内（不越条）");
    assert!(rx > W / 2, "× 盒在右半屏");
    assert!(rx + rw <= W, "× 盒不出屏");
    assert_eq!(rx + rw, W - web_page::CLOSE_MARGIN, "× 盒贴右端内边距");
    assert!(web_page::hit_close(f64::from(rx), f64::from(ry), W, bar));
    assert!(!web_page::hit_close(
        f64::from(rx + rw),
        f64::from(ry),
        W,
        bar
    ));

    // 常驻纪律源码锚：涂装/命中不许出现页面状态标识（白屏/加载/滚动都照画）
    let src = read_src("src/ui/web_page.rs").to_lowercase();
    for needle in ["webview", "loadurl", "web_status", "web-status", "mweb"] {
        assert!(
            !src.contains(needle),
            "web_page.rs 出现页面状态标识「{needle}」——常驻纪律破了"
        );
    }
}

// ---- 段①钉（NA0189 楼6 判据①：条高只留一份真源） ----

#[test]
fn spec_bar260_段1_条高公式唯一位点() {
    // 条高＝density × 44dp，四舍五入、下限 1——全进程唯一算式。
    // 变异抽检：改成 floor／去掉下限／把 44.0 挪成别的数，本钉必红。
    assert_eq!(web_page::bar_px(3.0), 132, "3x 真机档 = 132px");
    assert_eq!(web_page::bar_px(2.0), 88, "2x 档 = 88px");
    assert_eq!(web_page::bar_px(2.75), 121, "2.75x 档 = 121px");
    assert_eq!(web_page::bar_px(0.0), 1, "密度 0 也不许出 0 高条");
    assert_eq!(web_page::bar_px(-1.0), 1, "负密度兜底 1");
    assert_eq!(web_page::bar_px(f64::NAN), 1, "NaN 兜底 1");
    // 单源守卫：常量 BAR_H 不许复活（复活＝又出现第二个位点）
    let src = read_src("src/ui/web_page.rs");
    assert!(src.contains("pub fn bar_px"), "条高算式 bar_px 必须在");
    assert!(
        !src.contains("pub const BAR_H"),
        "pub const BAR_H 不许复活——条高只有 bar_px 一个位点"
    );
    assert!(
        src.contains("pub fn bar_h"),
        "运行期单源读数 bar_h 必须在（渲染/命中都读它）"
    );
}

#[test]
fn spec_bar260_段1_java只报密度不自算() {
    // 判据①的跨语言半边：Java 报密度这个平台事实，条高由 Rust 算好传过去。
    // 变异抽检：Java 里重新写回 44f*dm.density 本钉必红。
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("public float webDensity()"),
        "密度查询口必须在"
    );
    assert!(
        !java.contains("44f * dm.density") && !java.contains("Math.round(44"),
        "Java 不许自算条高（两端各算一份＝判据①破）"
    );
    assert!(
        java.contains("startWebViewFromGate(final String url, final int barPx)"),
        "承载几何必须吃 Rust 传回的条高"
    );
    assert!(
        java.contains("int bar = Math.max(1, barPx)"),
        "页体几何用传入条高（高＝屏高−条高、纵向偏移＝条高）"
    );
}

#[test]
fn spec_bar260_段1_桥传的就是那一份() {
    // 桥上一份、两端同一份：Rust 侧先问密度→set_from_density 算一次→
    // 把结果作为 int 传给 Java（同一值）。变异抽检：桥改回单参、或传常量
    // 而非计算结果，本钉必红。
    let app = read_src("src/android_app.rs");
    let body = fn_body(&app, "register_web_hook(Box::new(move |url: String|");
    assert!(body.contains("webDensity"), "桥必须问 Java 要密度");
    assert!(
        body.contains("set_from_density"),
        "条高必须由 web_page::set_from_density 算一次并落单源"
    );
    assert!(
        body.contains("java.lang.String, int"),
        "桥签名必须带条高参数（两端同一份）"
    );
    assert!(
        body.contains("JValue::Int(bar as i32)"),
        "传过去的必须是算出的 bar 值"
    );
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
    assert!(
        !java.contains("44f * dm.density"),
        "Java 不许自算条高（NA0189 楼6 判据①：一端算、两端取值）"
    );
    assert!(
        java.contains("int bar = Math.max(1, barPx)"),
        "Java 条高取 Rust 传值"
    );

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
