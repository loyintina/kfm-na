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
        java.contains("startWebViewFromGate(final String url, final int barPx, final int yOffPx)"),
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
    assert!(
        java.contains("lp.y = Math.max(0, yOffPx)"),
        "整页几何：纵向偏移让出「状态栏＋顶条」（yOff 由 Rust 算好传回）"
    );
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

// ---- 段②钉（NA0189 楼6 判据②：逐帧同步位移单源无抖动） ----

#[test]
fn spec_bar260_段2_位移单点取整且条与命中同吃() {
    // 位移的**唯一取整点**是 bar_offset_px：渲染顶条、命中「×」盒、桥下发
    // Java 窗三处都吃它的返回值。三处各自 round 就会各抖一像素。
    // 变异抽检：把 round 改 floor／把 NaN 兜底去掉／命中盒不吃 off，本钉必红。
    assert_eq!(web_page::bar_offset_px(37.4), 37, "37.4 → 37（四舍五入）");
    assert_eq!(web_page::bar_offset_px(37.5), 38, "37.5 → 38（四舍五入）");
    assert_eq!(web_page::bar_offset_px(0.0), 0);
    assert_eq!(
        web_page::bar_offset_px(f32::NAN),
        0,
        "NaN 兜底 0（不许出随机位移）"
    );
    assert_eq!(web_page::bar_offset_px(f32::INFINITY), 0, "Inf 兜底 0");
    // 条与命中同源：off 平移多少，命中盒就平移多少
    let bar = web_page::bar_px(3.0);
    let (rx0, _, _, _) = web_page::close_rect(W, bar);
    for off in [0i32, 37, -37, 400] {
        let (rx, _, rw, rh) = web_page::close_rect_off(W, bar, off);
        assert_eq!(rx, rx0 as i32 + off, "命中盒左缘必须随 off 平移");
        let cx = f64::from(rx) + f64::from(rw) / 2.0;
        let cy = f64::from(rh) / 2.0;
        assert!(
            web_page::hit_close_off(cx, cy, W, bar, off),
            "off={off} 时盒心必中"
        );
        // 盒外一像素必须不中（眼手同尺：命中不许比画出来的盒更大）
        assert!(
            !web_page::hit_close_off(f64::from(rx) - 1.0, cy, W, bar, off),
            "off={off} 时盒左缘外一像素不许中"
        );
        assert!(
            !web_page::hit_close_off(cx, cy + f64::from(rh) + 1.0, W, bar, off),
            "off={off} 时盒下缘外一像素不许中"
        );
    }
    // 目标值语义也只有一份：在栈=0 靠泊 / 不在栈=+屏宽
    assert_eq!(web_page::web_off_target(true, W), 0.0);
    assert_eq!(web_page::web_off_target(false, W), W as f32);
}

#[test]
fn spec_bar260_段2_桥只在值变时下发() {
    // 防抖判据：同一份整数位移重复下发会让 Java 侧反复 updateViewLayout，
    // 画面抖。变异抽检：判据改成恒真（每次都发）本钉必红；桥不发/发常量也红。
    assert!(!web_page::bridge_should_send(37, 37), "同值不发");
    assert!(web_page::bridge_should_send(37, 38), "变了要发");
    assert!(
        web_page::bridge_should_send(i32::MIN, 0),
        "首次必发（基线是哨兵）"
    );
    assert!(web_page::bridge_should_send(0, -1), "负向也发");

    // 源码锚：三处必须真的接上（防「钉绿但没人调用」）
    let gate = read_src("src/gate.rs");
    let body = fn_body(&gate, "pub fn web_offset_sync(");
    assert!(
        body.contains("offset_should_send"),
        "下发必须过判据（变了才发＋限频，判据内部含 bridge_should_send）"
    );
    assert!(body.contains("WEB_OFF_HOOK"), "下发必须走钩子（唯一通路）");

    let app = read_src("src/android_app.rs");
    assert!(
        app.contains("gate::web_offset_sync(off)"),
        "渲染用的同一份 off 必须逐帧下发给 Java 窗"
    );
    assert!(
        app.contains("register_web_offset_hook"),
        "位移钩子必须注册（JNI webOffset）"
    );

    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("public void webOffset(final int dx)"),
        "Java 必须有位移入口"
    );
    let jbody = fn_body(&java, "public void webOffset(");
    assert!(
        !jbody.contains("density") && !jbody.contains("44"),
        "Java 不许自己算位移（单源在 Rust）"
    );
    assert!(
        jbody.contains("lp.x = want") && jbody.contains("want = mPendWebDx"),
        "照最新值摆窗（覆盖不排队）"
    );
}

#[test]
fn spec_bar260_段2_第八道缝_直通与占槽可拔() {
    // 缝纪律（ui-base §三）：无占槽 = 渲染值 == 目标值（硬切基座）；占槽 =
    // 过采样器；拔槽 = 回直通。第八道缝是 Web 位移的唯一来源——它若被「优化」
    // 成不经缝的直通，位移就没了单一采样点（判据②失效）。
    // 变异抽检：sample 改成恒返回 target（绕过采样器）本钉必红。
    use kfm_na::ui::seam;
    use std::sync::Arc;
    seam::release_web_panel_offset_x();
    assert_eq!(
        seam::sample_web_panel_offset_x(W as f32, 0),
        W as f32,
        "无占槽必须直通目标值"
    );
    assert!(!seam::web_panel_offset_x_active(), "无占槽 = 零额外帧");
    seam::occupy_web_panel_offset_x(seam::Occupier {
        sampler: Arc::new(|tgt: f32, _now: u64| tgt / 2.0),
        is_active: Arc::new(|| true),
        replay: None,
    });
    assert_eq!(
        seam::sample_web_panel_offset_x(W as f32, 0),
        W as f32 / 2.0,
        "占槽后必须过采样器"
    );
    assert!(seam::web_panel_offset_x_active(), "占槽活跃 = 帧时钟启");
    seam::replay_web_panel_offset_x(W as f32, 0); // 无 replay = 空操作，不 panic
    seam::release_web_panel_offset_x();
    assert_eq!(
        seam::sample_web_panel_offset_x(W as f32, 0),
        W as f32,
        "拔槽回硬切直通"
    );
    assert!(!seam::web_panel_offset_x_active());
}

// ---- 出口红修复钉（NA0189 楼19–20：修出口＝修窗口层序，禁扩热区）----

#[test]
fn spec_bar260_出口_条区触摸必达并弹栈() {
    // 病根形态：Web 在顶时「先吞后判」⇒ 条区点击永远进不了弹栈。本钉把
    // 裁决做成可喂坐标的真值表，顺序反了必红。
    // 变异抽检：把 web_touch_decide 改成「web_top 即恒 Swallow（先吞后判）」，
    // 或把「只有 Started 能命中」去掉，本钉必红。
    use web_page::{WebTouch, web_touch_decide};
    // ① Web 不在顶：一律不归它管（交后续路由——别的面板/终端要能收到）
    assert_eq!(web_touch_decide(false, true, false), None);
    assert_eq!(web_touch_decide(false, true, true), None);
    // ② Web 在顶 + 落在「×」盒 + Started → 弹栈
    assert_eq!(
        web_touch_decide(true, true, true),
        Some(WebTouch::Dismiss),
        "条区「×」点击必须弹栈（这就是出口红要的那一条）"
    );
    // ③ Web 在顶 + 不在「×」盒 → 吞（页体 Java 侧吃，条带残余不留手）
    assert_eq!(web_touch_decide(true, true, false), Some(WebTouch::Swallow));
    // ④ Moved/Ended 不许弹栈（一次点击只弹一次）
    assert_eq!(web_touch_decide(true, false, true), Some(WebTouch::Swallow));
    // ⑤ 与几何同源：命中盒吃的是 close_rect_off 那一份（不是另算的矩形）
    let bar = web_page::bar_px(3.0);
    let (rx, ry, rw, rh) = web_page::close_rect_off(W, bar, 0);
    let cx = f64::from(rx) + f64::from(rw) / 2.0;
    let cy = f64::from(ry) + f64::from(rh) / 2.0;
    assert!(web_page::hit_close_off(cx, cy, W, bar, 0));
}

#[test]
fn spec_bar260_出口_条区回na且窗自身吃back() {
    // 硬判据（白露拍）：不许扩热区绕过——热区/绘制同源（本钉先咬这一点），
    // 再由窗口标志把「条区触摸」还回 na；且返回键不许被页体窗吃掉。
    // 变异抽检：删 FLAG_NOT_TOUCH_MODAL → 必红（条区又被吞）；
    //           删 FLAG_NOT_FOCUSABLE → 必红（BACK 又被吃＝页面困死）。
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    // 锚**赋值行本身**，不锚注释（「注释不是防线」：注释里提到常量名不算数）
    let flags_kv = java
        .split("lp.flags =")
        .nth(1)
        .and_then(|t| t.split(';').next())
        .expect("页体窗必须显式设 lp.flags");
    assert!(
        flags_kv.contains("FLAG_NOT_TOUCH_MODAL"),
        "页体窗必须 NOT_TOUCH_MODAL（窗外条区触摸回 na＝出口的唯一来路）"
    );
    // 裁乙（NA0189 楼37 判据戊）：**不许**再带 NOT_FOCUSABLE——它让我们这个
    // 任务里没有可聚焦窗，vivo 据此把前台判定回退到 launcher（无 FATAL 却掉桌面）
    assert!(
        !flags_kv.contains("FLAG_NOT_FOCUSABLE"),
        "页体窗不许带 NOT_FOCUSABLE（判据戊：任务必须有可聚焦窗）"
    );
    // 热区没被放大：命中盒仍由 close_rect_off 派生，边距常量没动
    assert_eq!(web_page::CLOSE_MARGIN, 24, "关闭盒边距不许为「好点」而放大");
    let jbody = fn_body(&java, "public void startWebViewFromGate(");
    assert!(
        jbody.contains("int bar = Math.max(1, barPx)")
            && jbody.contains("lp.y = Math.max(0, yOffPx)"),
        "页体窗顶必须从「状态栏高+条高」起（条带内无窗压着＝真高于）"
    );
}

// ---- 安全区钉（NA0189 楼22/23：条不许落进系统占用带；安全区从 WindowInsets 取）----

#[test]
fn spec_bar260_出口_条与命中盒不落系统带() {
    // 楼22 层序读数的结论：条原先整条画在系统状态栏带（0–135）里、「×」又压
    // 右缘手势条（x≥1176）——两处系统窗吃触摸，点按永远到不了 na。
    // 本钉咬几何：条带纵原点＝状态栏高、命中盒右界让出手势条宽。
    // 变异抽检：把 inset_top/inset_right 当 0（回退到全屏摆）本钉必红。
    let bar = web_page::bar_px(3.0);
    let (w, itop, iright) = (1260u32, 135u32, 84u32);
    web_page::set_insets(itop, iright);
    let (rx, ry, rw, rh) = web_page::close_rect_in(w, bar, itop, iright);
    assert!(ry >= itop, "命中盒顶必须在状态栏下缘之下（实测 y={ry}）");
    assert!(
        rx + rw <= web_page::avail_w(w, iright) - web_page::CLOSE_MARGIN,
        "命中盒右界必须让出手势条（实测右缘 {} vs 可用带 {}）",
        rx + rw,
        web_page::avail_w(w, iright)
    );
    // 命中判定吃同一份（close_rect_off 内部读单源 insets）
    let cx = f64::from(rx) + f64::from(rw) / 2.0;
    let cy = f64::from(ry) + f64::from(rh) / 2.0;
    assert!(
        web_page::hit_close_off(cx, cy, w, bar, 0),
        "盒心必中（inset 单源）"
    );
    assert!(
        !web_page::hit_close_off(cx, 60.0, w, bar, 0),
        "状态栏带里的点不许命中（那里是系统窗的地盘）"
    );
    // 恢复默认，免得污染同卷其他用例
    web_page::set_insets(0, 0);
}

#[test]
fn spec_bar260_出口_安全区从系统问不硬编码() {
    // 硬判据（清和楼23）：安全区**一律从 WindowInsets 取**，禁止 135／1176
    // 这类机型数值进代码；可用带要给实测边界数（由判卷取，不写死）。
    // 变异抽检：把 inset 改成字面常量、或退回全屏摆，本钉必红。
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    for bad in ["= 135", "= 1176", "135;", "1176;"] {
        assert!(
            !java.contains(bad),
            "Java 里不许出现硬编码安全区数值「{bad}」"
        );
    }
    let top_body = fn_body(&java, "public int webInsetTop()");
    assert!(
        top_body.contains("WindowInsets") && top_body.contains("statusBars"),
        "状态栏高必须从 WindowInsets 取"
    );
    let right_body = fn_body(&java, "public int webInsetRight()");
    assert!(
        right_body.contains("systemGestures") || right_body.contains("getSystemGestureInsets"),
        "右缘手势条宽必须从 WindowInsets 取"
    );
    assert!(
        right_body.contains("Math.max"),
        "右缘取 max（systemGestures/mandatory/systemWindowInset 三者最大）"
    );
    // 桥必须真把这两条问回来并落单源；起页命令把算好的 yOff 传下去
    let app = read_src("src/android_app.rs");
    assert!(
        app.contains("webInsetTop") && app.contains("webInsetRight"),
        "桥必须问回两条 inset"
    );
    assert!(
        app.contains("set_insets(itop, iright)"),
        "inset 必须落 web_page 单源（几何只在一处算）"
    );
    assert!(
        java.contains("final String url, final int barPx, final int yOffPx"),
        "起页命令必须带 yOff（= 状态栏高 + 条高，Rust 算好）"
    );
}

#[test]
fn spec_bar260_出口_条带绘制不越系统带() {
    // 像素级：条带涂装不许落进「状态栏带（y<inset_top）」和「右缘手势条
    // （x≥可用带右界）」——那两处系统窗吃触摸，画在那里等于画给用户看不见
    // 也点不着。变异抽检：绘制不吃 inset（回退全屏摆）本钉必红。
    let (cell_w, cell_h) = (kfm_na::termview::CELL_W, kfm_na::termview::CELL_H);
    let host = {
        let p = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf";
        let b = std::fs::read(p).expect("host 夹具字体缺失");
        fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).expect("host 字体不可用")
    };
    let tv = kfm_na::termview::TermView::new(host, None, 8, 2, cell_w, cell_h);
    let (w, h) = (8 * cell_w, 3 * cell_h);
    let (itop, iright) = (cell_h, 2 * cell_w);
    web_page::set_insets(itop, iright);
    let mut buf = vec![0u32; (w * h) as usize];
    web_page::paint(
        &tv,
        &mut buf,
        w,
        h,
        kfm_na::ui::accent::FALLBACK,
        web_page::bar_px(3.0),
        0,
    );
    web_page::set_insets(0, 0);
    // 状态栏带内不许有条带墨（三处：条底、标题、关闭盒）
    for y in 0..itop {
        for x in 0..w {
            assert_eq!(
                buf[(y * w + x) as usize],
                0,
                "状态栏带 ({x},{y}) 出现了条带墨——条画进系统占用带了"
            );
        }
    }
    // 右缘手势条内不许有条带墨
    let usable = web_page::avail_w(w, iright);
    for y in 0..h {
        for x in usable..w {
            assert_eq!(
                buf[(y * w + x) as usize],
                0,
                "手势条带 ({x},{y}) 出现了条带墨——条伸出可用带"
            );
        }
    }
    // 条带本体内必须真有墨（不是把整条画没了）
    assert!(
        buf.iter().any(|&p| p != 0),
        "可用带内必须真画出了条（防「让位让成不画」）"
    );
}

// ---- 出口重修第三件·生命周期与 BACK（NA0189 楼31/32 清和裁：三件全修＋四条判据）----

#[test]
fn spec_bar260_出口_back先关页不退activity() {
    // 判据乙：浮窗挂着时 BACK 先关页（回解析页），**不退 Activity**——直接
    // finish 会在浮窗未摘时销毁宿主，这正是 WindowLeaked 的来路。
    // 变异抽检：把裁决改回恒 Super（直接交给 Activity finish）本钉必红。
    use web_page::{BackAction, back_action};
    assert_eq!(back_action(true), BackAction::ClosePage, "页开着 = 先关页");
    assert_eq!(back_action(false), BackAction::Super, "页没开 = 交原路");
    // Java 侧必须真按这个语义接线：先摘窗、再问 Rust 弹栈、消费掉就不 super
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    let back = fn_body(&java, "public void onBackPressed()");
    assert!(
        back.contains("webPageTearDown(\"back\")"),
        "BACK 必须先摘浮窗（否则 finish 时留残窗）"
    );
    assert!(
        back.contains("nativeWebBackRequest()"),
        "BACK 必须请 Rust 弹面板栈（异步请求，栈仍是唯一真源）"
    );
    assert!(
        back.contains("return;"),
        "页开着时本端消费掉，不许再 super（不许退 Activity）"
    );
    assert!(
        back.contains("super.onBackPressed();"),
        "无页可关时仍要交原路（不许把 BACK 吃掉）"
    );
}

#[test]
fn spec_bar260_出口_生命周期幂等收尾() {
    // 判据甲／丙的机制面：三处生命周期钩子都收尾，且收尾**幂等**
    // （先置空再摘窗销毁——重复调用不二次 destroy，炸在钉上不炸在真机）。
    // 变异抽检：删 onDestroy 那处 → 红；去掉判空/置空幂等 → 红。
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    for (hook, why) in [
        ("public void onPause()", "pause"),
        ("public void onStop()", "stop"),
        ("public void onDestroy()", "destroy"),
    ] {
        let body = fn_body(&java, hook);
        assert!(
            body.contains(&format!("webPageTearDown(\"{why}\")")),
            "{hook} 必须收尾浮窗（缺一处＝后台/旋转时留残窗）"
        );
    }
    let td = fn_body(&java, "private void webPageTearDown(");
    assert!(
        td.contains("if (mWeb == null)") && td.contains("return;"),
        "收尾必须判空（幂等第一步）"
    );
    assert!(
        td.contains("mWeb = null;"),
        "收尾必须先置空再摘窗销毁（否则重复调用会二次 destroy）"
    );
    assert!(
        td.contains("catch (Throwable"),
        "收尾异常必须兜住（Activity 销毁期摘窗本就会抛）"
    );
    // 起页侧仍只有「新建」路径，不许在两处各写一份销毁逻辑
    assert_eq!(
        java.matches("removeView(").count(),
        1,
        "摘窗只许一处（收尾助手），别处再写一份就是双份销毁逻辑"
    );
}

#[test]
fn spec_bar260_出口_浮窗自己吃back不漏给activity() {
    // 判据庚（NA0189 楼37 裁乙）：「谁盖着屏谁处理返回」——BACK 由浮窗自己吃：
    // 拆窗 + 请 Rust 弹栈，且两个动作都 return true（既关页也不漏给 Activity）。
    // 变异抽检：摘掉 dispatchKeyEvent／把 BACK 改成 super 透传，本钉必红。
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("public boolean dispatchKeyEvent(android.view.KeyEvent ev)"),
        "浮窗必须自己处理按键（dispatchKeyEvent 覆写）"
    );
    let disp = fn_body(
        &java,
        "public boolean dispatchKeyEvent(android.view.KeyEvent ev)",
    );
    assert!(
        disp.contains("KEYCODE_BACK") && disp.contains("ACTION_UP"),
        "只认 BACK 的抬起（按下不重复触发关页）"
    );
    assert!(
        disp.contains("webPageTearDown(\"back-in-window\")"),
        "窗内 BACK 必须先拆窗（不留残窗）"
    );
    assert!(
        disp.contains("nativeWebBackRequest()"),
        "窗内 BACK 必须走异步请求（UI 线程不进 Rust 取锁）"
    );
    assert!(
        disp.contains("return true; // 吃掉"),
        "BACK 必须吃掉并返回 true（关页且不漏给 Activity）"
    );
    assert!(
        disp.contains("return super.dispatchKeyEvent(ev);"),
        "非 BACK 键仍要交父类（别把窗的其它按键一起吞了）"
    );
    // 判据己的机制半边：NOT_TOUCH_MODAL 必须在（条区触摸回 na 的唯一来路）
    let flags_kv = java
        .split("lp.flags =")
        .nth(1)
        .and_then(|t| t.split(';').next())
        .expect("页体窗必须显式设 lp.flags");
    assert!(
        flags_kv.contains("FLAG_NOT_TOUCH_MODAL"),
        "判据己：NOT_TOUCH_MODAL 必须留着（否则条区触摸又被吞）"
    );
}

// ---- 保活面（NA0189 楼52/53：判活判进展＋FGS 启动兜底；清和批准并要钉）----

#[test]
fn spec_bar260_保活_心跳落账与启动兜底() {
    // 两条都要：①启动兜底——建渠道/startForeground/拿锁任一步抛，不许静默
    // 无痕；②心跳落账——每 30s 一条可观察进展（序号＋时刻），死前最后一条
    // 就是死亡时刻证据（MAIN0182 第四通则「进程活着≠在推进」）。
    // 变异抽检：摘心跳→红；摘 try/catch→红；把心跳间隔改掉→红。
    let svc = read_src("android/java/dev/kfm/na/KfmKeepAliveService.java");
    assert!(
        svc.contains("private static final long HB_MS = 30000L;"),
        "心跳间隔必须是 30s 常量（判据要可核，不许散落字面量）"
    );
    let create = fn_body(&svc, "public void onCreate()");
    assert!(
        create.contains("try {") && create.contains("catch (Throwable"),
        "onCreate 必须兜住异常（否则 FGS 静默没起来，查都无从查）"
    );
    assert!(
        create.contains("fgs-boot-fail"),
        "启动失败必须落痕（logcat 一行 fgs-boot-fail）"
    );
    assert!(
        create.contains("mH.postDelayed(mBeat, HB_MS)"),
        "onCreate 成功后必须起心跳"
    );
    // 心跳本身：序号＋时刻＋FGS 存活标记，且自续
    assert!(
        svc.contains("hb=\" + mHb + \" at=\" + now"),
        "心跳必须带序号与时刻（两采样点无变化即判不推进）"
    );
    assert!(
        svc.contains("mH.postDelayed(this, HB_MS)"),
        "心跳必须自续（一锤子不算心跳）"
    );
    let destroy = fn_body(&svc, "public void onDestroy()");
    assert!(
        destroy.contains("mH.removeCallbacksAndMessages(null)") && destroy.contains("hb-stop"),
        "onDestroy 必须停心跳并落 hb-stop（给死亡时刻留边界）"
    );
    // 通知构造单源（首帧与心跳同源，不各写一份）
    // 通知构造集中在一处（buildNotification 里的 SDK 分档两条），首帧与心跳都
    // 调它——外面再 new 一个 Builder 就是第二份构造
    assert_eq!(
        svc.matches("new Notification.Builder(").count(),
        2,
        "Builder 只许出现在 buildNotification 的 SDK 分档里"
    );
    let bn = fn_body(&svc, "private Notification buildNotification(");
    assert!(
        bn.matches("new Notification.Builder(").count() == 2,
        "两处 Builder 都必须在 buildNotification 内"
    );
    assert!(
        svc.matches("buildNotification(").count() >= 3,
        "buildNotification 必须被首帧与心跳共用（定义 1 + 调用 2）"
    );
}

#[test]
fn spec_bar260_出口_ui线程不进rust取锁() {
    // 判据（NA0189 楼57 清和裁）＝**主线程不再被 Rust 锁阻塞**：Java 的 UI 线程
    // 只调异步请求（返回 void、只置原子标志），面板栈弹出由渲染循环下一帧消费。
    // 变异抽检：改回同步调用（returns boolean／JNI 体内取锁）→ 本钉必红。
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("private static native void nativeWebBackRequest();"),
        "请求必须返回 void（Java 不等 Rust 结果＝UI 线程零等待）"
    );
    assert!(
        !java.contains("nativeWebBack()"),
        "同步版 nativeWebBack 必须整删（留着就会被再调）"
    );
    assert!(
        !java.contains("boolean consumed = nativeWebBackRequest()"),
        "不许把请求当返回值用（那就又同步了）"
    );
    let app = read_src("src/android_app.rs");
    let jni_body = fn_body(
        &app,
        "pub extern \"system\" fn Java_dev_kfm_na_MainActivity_nativeWebBackRequest(",
    );
    assert!(jni_body.contains("WEB_BACK_REQ.store"), "JNI 体只许置标志");
    for bad in ["ai_presence_handle", "Mutex", "lock()", "report("] {
        assert!(
            !jni_body.contains(bad),
            "JNI 体内不许出现 {bad}（取锁／落盘都在 UI 线程上，正是要摘掉的东西）"
        );
    }
    // 消费点在渲染循环里（同帧），且消费后真弹栈
    let drain = fn_body(&app, "fn drain_web_back_request(&mut self)");
    assert!(
        drain.contains("take_web_back_request()")
            && drain.contains("ai.dismiss_top(crate::ai_presence::Panel::Web)"),
        "每帧排水函数必须消费请求并弹栈（栈是唯一真源）"
    );
    assert!(
        app.contains("self.drain_web_back_request();"),
        "排水函数必须真被每帧调用（钉绿但没人调＝没接上）"
    );
}

#[test]
fn spec_bar260_出口_搬窗限频与最新值覆盖() {
    // 判据（NA0189 楼61/62：括号原文＝「已投递输入没处理完」，注入 2 个 MotionEvent
    // 等 5000ms）：逐帧搬窗把 UI 线程堵住。两条收敛——**源头限频**（Rust 侧 33ms）
    // 与**最新值覆盖不排队**（Java 侧一个 UI 任务带着最新值跑）。
    // 变异抽检：去掉限频→红；改成排队（每帧 post 一个 runnable）→红。
    use web_page::offset_should_send;
    // 限频真值表：变了但太快→不发；变了且够久→发；没变→一律不发
    assert!(
        !offset_should_send(0, 5, 1000, 1010),
        "10ms 内变了也不发（限频）"
    );
    assert!(offset_should_send(0, 5, 1000, 1033), "33ms 且变了→发");
    assert!(
        !offset_should_send(5, 5, 1000, 5000),
        "没变→不发（防抖优先）"
    );
    assert!(offset_should_send(5, 6, 0, 1), "首次（last_at=0）不挡");
    // Java 侧：覆盖不排队
    let java = read_src("android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("mPendWebDx = dx; // 覆盖，不排队"),
        "迟到帧必须覆盖字段，不许排队"
    );
    assert!(
        java.contains("if (mWebDxPosted)") && java.contains("return; // 已有一次 UI 任务在跑"),
        "已有 UI 任务在跑时不许再 post（一次任务带最新值）"
    );
    let off_body = fn_body(&java, "public void webOffset(");
    assert_eq!(
        off_body.matches("runOnUiThread(new Runnable()").count(),
        1,
        "webOffset 内只许排一个 runnable（出现两处＝排队）"
    );
    // Rust 侧：限频常量与调用点都在
    let gate = read_src("src/gate.rs");
    assert!(
        gate.contains("offset_should_send(last, off, last_at, now)"),
        "下发必须过限频判据"
    );
    assert!(
        read_src("src/ui/web_page.rs").contains("pub const OFFSET_MIN_INTERVAL_MS: u64 = 33;"),
        "限频间隔必须是 33ms 常量（可核，不散落字面量）"
    );
}

#[test]
fn spec_bar265_尖峰落盘阈值与分项() {
    // BAR-265／NA0194 楼4/5（清和批）：均值口径证不了单帧尖峰，加「整帧 ≥
    // 阈值即逐帧落一行，带时刻与分项」。阈值 500ms 可核，分项四段齐。
    // 变异抽检：阈值改 0（全报）或改 i64 级大数（全不报）→ 本钉必红。
    use kfm_na::gate::{FRAME_SPIKE_MS, frame_spike_hit};
    assert_eq!(FRAME_SPIKE_MS, 500, "阈值必须是 500ms 常量（可核、不散落）");
    assert!(!frame_spike_hit(0), "0ms 不报");
    assert!(!frame_spike_hit(499), "499ms 不报（阈值下方是噪声区）");
    assert!(frame_spike_hit(500), "500ms 恰达即报");
    assert!(frame_spike_hit(999), "999ms 报");
    assert!(frame_spike_hit(60000), "整分钟级更报");
    // 落盘行必须带时刻与**真有的**分项：口径＝gen+ras 两段（upload／draw 在
    // present 段取不到真值，不许假填 0 —— NA0194 楼7 清和点名的一处）
    let gate = read_src("src/gate.rs");
    let body = fn_body(&gate, "pub fn report_frame_spike(");
    for f in ["total=", "at=", "gen=", "ras="] {
        assert!(body.contains(f), "尖峰行必须含字段 {f}");
    }
    assert!(
        body.contains("不含 present"),
        "落盘行必须自报口径（否则下一个人会以为 total 是整帧）"
    );
    for fake in ["upload=", "draw="] {
        assert!(
            !body.contains(fake),
            "不许再出现假分项 {fake}（传 0＝名存实亡）"
        );
    }
    // 限流：200 行/小时（清和批准时定的，第一版漏了）
    use kfm_na::gate::{FRAME_SPIKE_PER_HOUR, FRAME_SPIKE_WINDOW_MS, frame_spike_admit};
    assert_eq!(FRAME_SPIKE_PER_HOUR, 200, "限流上限必须 200 行/小时");
    assert_eq!(FRAME_SPIKE_WINDOW_MS, 3_600_000, "窗长必须 1 小时");
    // 真值表：首帧放行建窗；窗内到顶拒；跨窗重置
    let (ok, st, c) = frame_spike_admit(1000, 0, 0);
    assert!(ok && st == 1000 && c == 1, "首帧放行并建窗");
    let (ok, _, c) = frame_spike_admit(1500, 1000, 199);
    assert!(ok && c == 200, "第 200 行放行");
    let (ok, _, c) = frame_spike_admit(2000, 1000, 200);
    assert!(!ok && c == 200, "第 201 行拒");
    let (ok, st2, c2) = frame_spike_admit(3_602_000, 1000, 200);
    assert!(ok && st2 == 3_602_000 && c2 == 1, "跨窗重置后放行");
    // 调用点必须真接上（钉绿但没人调＝没接上）
    let app = read_src("src/android_app.rs");
    assert!(
        app.contains("crate::gate::frame_spike_hit(")
            && app.contains("crate::gate::frame_spike_gate()")
            && app.contains("crate::gate::report_frame_spike("),
        "绘制路径必须按判据调用尖峰落盘，且过限流"
    );
}
