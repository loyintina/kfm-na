//! ai_presence_spec.rs — AI 外显状态核+光球考题（期 0 组件一，A 档纯逻辑，
//! 答案 src/ai_presence.rs + src/plugins/ai_presence.rs + gate.rs 通道十）
//!
//! 依据：docs/active/ai-presence.md v3 §五（两布尔模型全部规则）/§八 D7-D9。
//! 判卷维度：
//! - 两布尔状态机：ai_running × page；浮层可见性 = f(running, dismissed, 驻留)
//! - 光球：默认位/拖动钳制（不出屏、让位快捷键行与键盘）/pressed/命中半径
//! - fake_run（debug 长按钩子）到期自动 run_end 等价行为
//! - 插件面：服务键注册/disabled 一键关；探针注入行解析（通道十）
//! - 观测：stats ai_presence 字段族（机器轨）
//! - B 档冒烟钉：雾球 sprite 与 AI 页占位真画进帧缓冲

use std::sync::Arc;

use kfm_na::ai_presence::{
    self, AiPresenceState, LINGER_MS, ORB_HIT_RADIUS_PX, ORB_RADIUS_PX, Page,
};
use kfm_na::base::{Base, FiberState, GetError, PluginEntry};
use kfm_na::input_bar;
use kfm_na::keybar;

// 假想屏 1080x2400（真机净宽量级）；默认位 = 右缘 × 屏高 60%
const W: u32 = 1080;
const H: u32 = 2400;

fn new_state() -> AiPresenceState {
    let ai = AiPresenceState::new();
    ai.set_bounds(W, H, 0);
    ai
}

fn default_pos() -> (f64, f64) {
    // 新出生位 (0.859, 0.556) 在屏内、钳制不触发（旧「右缘」位才会被钳进半径）
    (
        f64::from(W) * ai_presence::DEFAULT_X_RATIO,
        f64::from(H) * ai_presence::DEFAULT_Y_RATIO,
    )
}

// ---- 状态机：初始态 ----

#[test]
fn spec_初始态_终端页球在默认位暗浮层隐() {
    let ai = new_state();
    let s = ai.snap(0);
    assert_eq!(s.page, Page::Terminal, "开考必须在终端页");
    assert!(!s.ai_running, "开考 AI 不在跑");
    assert!(!s.pressed, "开考无按压");
    assert!(!s.overlay_visible, "开考浮层隐");
    let (dx, dy) = default_pos();
    assert_eq!((s.x, s.y), (dx, dy), "默认位 = 右缘 × 屏高 60%（D6）");
    // 球心在屏内（右缘内缩一个可视半径）
    assert!(s.x > 0.0 && s.x < f64::from(W));
    assert!(s.y > 0.0 && s.y < f64::from(H));
    // 闲态增益 = 暗（D8 定稿：几乎透明但确实有球）
    assert_eq!(
        ai_presence::orb_gain(s.ai_running, s.pressed, s.page),
        (ai_presence::GAIN_IDLE, 1.0)
    );
}

// ---- 状态机：run_start / run_end / 驻留 ----

#[test]
fn spec_run_start_浮层现灯亮() {
    let ai = new_state();
    ai.run_start(1000);
    let s = ai.snap(1000);
    assert!(s.ai_running, "run_start 后灯亮");
    assert!(s.overlay_visible, "run_start → 浮层现（D7）");
    assert_eq!(s.page, Page::Terminal, "run_start 不抢全屏（D7）");
    assert_eq!(
        ai_presence::orb_gain(s.ai_running, s.pressed, s.page),
        (ai_presence::GAIN_RUNNING, ai_presence::HALO_GAIN_RUNNING),
        "运行态 = 整 sprite 全亮 + 光晕增益（D8 硬切）"
    );
}

#[test]
fn spec_run_end_驻留期内仍现灯灭() {
    let ai = new_state();
    ai.run_start(1000);
    ai.run_end(2000);
    // 驻留期内：浮层仍现（尾随读完短回复的窗口）
    let s = ai.snap(2000 + LINGER_MS - 1);
    assert!(!s.ai_running, "run_end 灯灭");
    assert!(s.overlay_visible, "驻留期内浮层仍现");
    assert_eq!(
        ai_presence::orb_gain(s.ai_running, s.pressed, s.page),
        (ai_presence::GAIN_IDLE, 1.0),
        "灯灭 = 回闲态增益"
    );
}

#[test]
fn spec_run_end_驻留期过浮层隐() {
    let ai = new_state();
    ai.run_start(1000);
    ai.run_end(2000);
    let s = ai.snap(2000 + LINGER_MS);
    assert!(!s.overlay_visible, "过 LINGER_MS 浮层必须隐");
}

#[test]
fn spec_甩掉浮层_本次运行不现下次run_start复位() {
    let ai = new_state();
    ai.run_start(1000);
    ai.dismiss_overlay();
    assert!(!ai.overlay_visible(1100), "甩掉后本次运行不现");
    // 甩掉后 run_end 的驻留期也不现（per-run dismissed 覆盖整次运行）
    ai.run_end(2000);
    assert!(!ai.overlay_visible(2100), "甩掉后驻留期也不现");
    // 下一次 run_start 复位
    ai.run_start(5000);
    assert!(ai.overlay_visible(5000), "下次 run_start 浮层复位重现");
}

// ---- 状态机：page 往返 / tap_overlay / 全屏不踢人 ----

#[test]
fn spec_tap_orb_往返切页() {
    let ai = new_state();
    ai.tap_orb();
    assert_eq!(ai.snap(0).page, Page::AiFullscreen, "tap → AI 全屏");
    ai.tap_orb();
    assert_eq!(ai.snap(0).page, Page::Terminal, "再 tap → 回终端");
}

#[test]
fn spec_tap_overlay_跳全屏() {
    let ai = new_state();
    ai.run_start(1000);
    ai.tap_overlay();
    assert_eq!(
        ai.snap(1000).page,
        Page::AiFullscreen,
        "点浮层 → 跳 AI 全屏"
    );
}

#[test]
fn spec_全屏时run_end不踢人() {
    let ai = new_state();
    ai.tap_orb(); // 人在 AI 全屏
    ai.run_start(1000);
    ai.run_end(2000);
    let s = ai.snap(2000 + LINGER_MS + 1);
    assert_eq!(
        s.page,
        Page::AiFullscreen,
        "run_end 不许把人踢出 AI 全屏（D7）"
    );
    assert!(!s.ai_running);
}

// ---- 光球：拖动与钳制 ----

#[test]
fn spec_drag_更新位置() {
    let ai = new_state();
    ai.drag_to(500.0, 800.0);
    let s = ai.snap(0);
    assert_eq!((s.x, s.y), (500.0, 800.0), "拖动更新球位（D9）");
}

#[test]
fn spec_drag_钳制_左越界() {
    let ai = new_state();
    ai.drag_to(-100.0, 800.0);
    assert_eq!(
        ai.snap(0).x,
        f64::from(ORB_RADIUS_PX),
        "左越界钳到可视半径处"
    );
}

#[test]
fn spec_drag_钳制_右越界() {
    let ai = new_state();
    ai.drag_to(99999.0, 800.0);
    assert_eq!(
        ai.snap(0).x,
        f64::from(W) - f64::from(ORB_RADIUS_PX),
        "右越界钳到屏右内缩一个半径"
    );
}

#[test]
fn spec_drag_钳制_上越界() {
    let ai = new_state();
    ai.drag_to(500.0, -50.0);
    assert_eq!(
        ai.snap(0).y,
        f64::from(ORB_RADIUS_PX),
        "上越界钳到可视半径处"
    );
}

#[test]
fn spec_drag_钳制_下越界让位快捷键行() {
    let ai = new_state();
    ai.drag_to(500.0, 99999.0);
    // 期 0 组件三：chrome 叠高一层（快捷键行 + 全局输入栏）
    let expect = f64::from(H)
        - f64::from(keybar::HEIGHT_PX)
        - f64::from(input_bar::HEIGHT_PX)
        - f64::from(ORB_RADIUS_PX);
    assert_eq!(ai.snap(0).y, expect, "下越界让位快捷键行+输入栏（D9）");
}

#[test]
fn spec_drag_钳制_键盘弹起再让位() {
    let ai = new_state();
    ai.set_bounds(W, H, 300); // 键盘 inset 300px
    ai.drag_to(500.0, 99999.0);
    let expect = f64::from(H)
        - 300.0
        - f64::from(keybar::HEIGHT_PX)
        - f64::from(input_bar::HEIGHT_PX)
        - f64::from(ORB_RADIUS_PX);
    assert_eq!(ai.snap(0).y, expect, "键盘弹起时球不许钻进键盘区");
}

// ---- 光球：pressed / 命中 ----

#[test]
fn spec_pressed_置位复位() {
    let ai = new_state();
    ai.press_down();
    let s = ai.snap(0);
    assert!(s.pressed, "按下置位");
    assert_eq!(
        ai_presence::orb_gain(s.ai_running, s.pressed, s.page),
        (ai_presence::GAIN_PRESSED, 1.0),
        "pressed = 整 sprite 提亮（D8 硬切第四态）"
    );
    ai.press_up();
    assert!(!ai.snap(0).pressed, "抬起复位");
}

#[test]
fn spec_gain_ai页与四态优先级() {
    // D8 定稿四态：闲/运行/pressed/AI页 = 整 sprite 增益硬切（无动画帧），
    // 光晕增益只属运行态
    assert_eq!(
        ai_presence::orb_gain(false, false, Page::AiFullscreen),
        (ai_presence::GAIN_AI_PAGE, 1.0)
    );
    // 优先级：pressed > running > AI 页 > 闲
    assert_eq!(
        ai_presence::orb_gain(true, true, Page::AiFullscreen),
        (ai_presence::GAIN_PRESSED, 1.0),
        "pressed 压过 running 与 AI 页"
    );
    assert_eq!(
        ai_presence::orb_gain(false, true, Page::Terminal),
        (ai_presence::GAIN_PRESSED, 1.0)
    );
    // 增益排序钉：pressed 最亮，闲态最暗（几乎透明但确实有球）——
    // 数值钉死 GAIN_* 常量本体（排序走 orb_gain 读数，避开常量断言 lint）。
    // 加法语义（2026-08-30 压字反馈后重调，alpha 时代旧值不复用）；
    // 二调（同日）：闲 = 1.0 = 样式参考基准亮度（用户实机裁图定量，
    // 闲 0.7 全面 ~60% 偏暗实锤），运行/按压顺势抬档保持排序
    assert_eq!(ai_presence::GAIN_PRESSED, 1.4);
    assert_eq!(ai_presence::GAIN_RUNNING, 1.15);
    assert_eq!(ai_presence::GAIN_IDLE, 1.0);
    assert_eq!(ai_presence::HALO_GAIN_RUNNING, 1.2);
    let (g_idle, _) = ai_presence::orb_gain(false, false, Page::Terminal);
    let (g_run, h_run) = ai_presence::orb_gain(true, false, Page::Terminal);
    let (g_press, _) = ai_presence::orb_gain(false, true, Page::Terminal);
    assert!(g_press > g_run && g_run > g_idle, "pressed > running > 闲");
    assert!(h_run > 1.0, "运行态光晕必须真加大");
}

#[test]
fn spec_常量_手势阈值家族钉死() {
    // D8「常量可调」的反面钉：调整是有意的，改这些数必须过考题眼睛
    assert_eq!(ai_presence::LONG_PRESS_MS, 600, "长按阈值（规格书 §五）");
    assert_eq!(ai_presence::DRAG_THRESHOLD_PX, 20.0, "拖动阈值");
    assert_eq!(LINGER_MS, 3000, "浮层驻留（D7 初值）");
}

#[test]
fn spec_hit_orb_命中半径() {
    let ai = new_state();
    let (cx, cy) = default_pos();
    assert!(ai.hit_orb(cx, cy), "球心必中");
    assert!(
        ai.hit_orb(cx + f64::from(ORB_HIT_RADIUS_PX) - 1.0, cy),
        "命中半径内必中"
    );
    assert!(
        !ai.hit_orb(cx + f64::from(ORB_HIT_RADIUS_PX) + 1.0, cy),
        "命中半径外不中"
    );
    assert!(!ai.hit_orb(10.0, 10.0), "远点不中");
}

// ---- fake_run（debug 长按钩子，echo-brain 就位后可拆） ----

#[test]
fn spec_fake_run_到期自动run_end等价行为() {
    let ai = new_state();
    ai.fake_run(3000, 10_000);
    let s = ai.snap(10_000);
    assert!(s.ai_running, "fake_run 期间灯亮");
    assert!(s.overlay_visible, "fake_run 期间浮层现");
    // 到期前一刻仍在跑
    assert!(ai.snap(12_999).ai_running);
    // 到期 = run_end 等价行为：灯灭 + 驻留期浮层仍现
    let s = ai.snap(13_000);
    assert!(!s.ai_running, "到期自动 run_end（灯灭）");
    assert!(s.overlay_visible, "到期后驻留期浮层仍现");
    // 驻留期过后隐
    assert!(!ai.snap(13_000 + LINGER_MS).overlay_visible);
}

#[test]
fn spec_fake_run_中途真run_end_到期不复发() {
    let ai = new_state();
    ai.fake_run(3000, 10_000);
    ai.run_end(11_000); // 真结束抢先
    assert!(!ai.snap(13_000).ai_running, "到期不许把已结束的run复活");
    assert!(
        !ai.snap(11_000 + LINGER_MS).overlay_visible,
        "驻留从真 run_end 起算"
    );
}

// ---- 插件面（cordis-na：服务键 + disabled 一键关） ----

#[test]
fn spec_插件_装载即提供状态核服务() {
    let base = Base::new(vec![]);
    base.load(kfm_na::plugins::ai_presence::AiPresence::new())
        .expect("装载应成功");
    assert_eq!(
        base.state(kfm_na::plugins::ai_presence::PLUGIN_NAME),
        Some(FiberState::Active),
        "apply 只注册共享实例，应瞬时 Active"
    );
    let ai = base
        .ctx()
        .get::<AiPresenceState>()
        .expect("AiPresenceState 服务键应可取回");
    // 人走触摸、AI 走服务，同一状态核（D9）：服务句柄就是事件接口
    ai.run_start(1000);
    assert!(ai.snap(1000).overlay_visible);
}

#[test]
fn spec_插件_disabled一键关() {
    let base = Base::new(vec![PluginEntry {
        id: kfm_na::plugins::ai_presence::PLUGIN_NAME,
        disabled: true,
        config: None,
    }]);
    base.load(kfm_na::plugins::ai_presence::AiPresence::new())
        .expect("disabled 也该注册得进（只是不激活）");
    assert!(
        matches!(
            base.ctx().get::<AiPresenceState>(),
            Err(GetError::DeclaredButInactive(_))
        ),
        "disabled → 服务键取不到（回退第一层：一键关）"
    );
}

// ---- 通道十：orb-inject 探针注入行解析（纯函数） ----

#[test]
fn spec_orb注入_行解析() {
    use kfm_na::gate::{OrbCmd, parse_orb_line};
    assert_eq!(parse_orb_line("tap"), Some(Ok(OrbCmd::Tap)));
    assert_eq!(
        parse_orb_line("drag 500 800"),
        Some(Ok(OrbCmd::Drag { x: 500.0, y: 800.0 }))
    );
    assert_eq!(
        parse_orb_line("run 3000"),
        Some(Ok(OrbCmd::Run { ms: 3000 }))
    );
    assert_eq!(parse_orb_line("end"), Some(Ok(OrbCmd::End)));
    assert_eq!(parse_orb_line("dismiss"), Some(Ok(OrbCmd::Dismiss)));
    // 空行/注释跳过；坏行报 Err
    assert_eq!(parse_orb_line(""), None);
    assert_eq!(parse_orb_line("# 注释"), None);
    assert!(matches!(parse_orb_line("drag 1"), Some(Err(_))));
    assert!(matches!(parse_orb_line("run abc"), Some(Err(_))));
    assert!(matches!(parse_orb_line("fly"), Some(Err(_))));
}

#[test]
fn spec_orb注入_脚本解析_坏行隔离() {
    use kfm_na::gate::{OrbCmd, parse_orb_script};
    let (cmds, errs) = parse_orb_script("tap\n坏行 here\ndrag 1 2\n");
    assert_eq!(cmds, vec![OrbCmd::Tap, OrbCmd::Drag { x: 1.0, y: 2.0 }]);
    assert_eq!(errs.len(), 1, "坏行隔离上报不拖垮好行");
}

// ---- 观测：stats ai_presence 字段族（机器轨） ----

#[test]
fn spec_stats_ai_presence字段族() {
    let ai = Arc::new(AiPresenceState::new());
    ai.set_bounds(W, H, 0);
    ai.drag_to(500.0, 600.0);
    ai.run_start(1000); // 不 run_end：running=true、overlay 必现（与墙钟无关）
    kfm_na::gate::register_ai_presence(&ai);
    let snap = kfm_na::gate::stats_snap();
    assert_eq!(snap.ai_page, "terminal");
    assert!(snap.ai_running, "stats 看得到灯亮");
    assert_eq!((snap.ai_orb_x, snap.ai_orb_y), (500, 600));
    assert!(!snap.ai_pressed);
    assert!(snap.ai_overlay, "stats 看得到浮层现");
    let out = kfm_na::gate::format_stats(&snap);
    for key in [
        "ai_page=terminal\n",
        "ai_running=true\n",
        "ai_orb_x=500\n",
        "ai_orb_y=600\n",
        "ai_pressed=false\n",
        "ai_overlay=true\n",
    ] {
        assert!(out.contains(key), "format_stats 缺 {key}");
    }
    ai.tap_orb();
    let snap = kfm_na::gate::stats_snap();
    assert_eq!(snap.ai_page, "ai", "tap 后 stats 看得到 page 翻转");
}

// ---- B 档冒烟钉：chrome 真画进帧缓冲 ----

#[test]
fn spec_冒烟_ai页空态纯底零墨() {
    // 空态内容区 = 纯底零墨（2026-09-04 用户拍板撤占位提示：对话框没说话
    // 时就是空的——游戏对话框语言；占位期小字已退役）。
    // 2026-09-04 装修修订：边框（仿 kfmv4 orb-panel）是页面装修不是占位
    // 提示——边框带必须有墨，框内内容区零墨的契约不变
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (400u32, 300u32);
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &[], 0, 0, false);
    assert_eq!(buf[0], kfm_na::termview::AI_PAGE_BG, "整屏深紫暗底");
    // 框内内容区（远离边框带与发光晕）必须零墨——任何非底色像素都是
    // 占位提示复活
    for y in 80..(h - 80) {
        let row = &buf[(y * w + 80) as usize..(y * w + w - 80) as usize];
        assert!(
            row.iter().all(|&p| p == kfm_na::termview::AI_PAGE_BG),
            "空态内容区必须零墨（y={y} 见非底色）"
        );
    }
    // 边框带必须有墨（装修契约：空态也画框）——左缘粗边中点必非底色
    let left_edge = buf[((h / 2) * w + 18) as usize];
    assert_ne!(
        left_edge,
        kfm_na::termview::AI_PAGE_BG,
        "边框左缘必须有墨（空态也画框）"
    );
}

#[test]
fn spec_冒烟_ai页边框配方钉() {
    // 2026-09-04 装修：边框几何/渐变的像素级配方钉（kfmv4 orb-panel 直译
    // 的判卷）——探测点全部从常量推导（改配方必红），渐变期望值用同款
    // lerp_rgb 从 C1/C2 现算（不猜死值）
    use kfm_na::termview as tvv;
    let (tv, _, _) = tvv::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let (m, fw) = (tvv::AI_PAGE_FRAME_MARGIN, tvv::AI_PAGE_FRAME_W);
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &[], 0, 0, false);
    let bg = tvv::AI_PAGE_BG;
    let px = |x: u32, y: u32| buf[(y * w + x) as usize];
    let (ow, oh) = (w - 2 * m, h - 2 * m); // 外环尺寸（bottom_inset=0）
    let denom = (ow - 1) + (oh - 1); // 与 fill_round_rect_grad diag 同尺
    let grad = |lx: u32, ly: u32| {
        tvv::lerp_rgb(
            tvv::AI_PAGE_FRAME_C1,
            tvv::AI_PAGE_FRAME_C2,
            ((lx + ly) * 255 / denom).min(255),
        )
    };
    // 屏边留白（发光晕之外）无墨
    assert_eq!(px(1, h / 2), bg, "边框外留白必须纯底");
    // 左缘 3 倍粗（kfmv4 border-left-width:3px）：x ∈ [m, m+3W) 通墨，
    // x = m+3W 起是内芯底色
    let mid_y = h / 2;
    let ly = mid_y - m;
    assert_eq!(px(m + 2, mid_y), grad(2, ly), "左缘粗边内点必须是渐变墨");
    assert_eq!(
        px(m + 3 * fw - 1, mid_y),
        grad(3 * fw - 1, ly),
        "左缘粗边末列仍是墨"
    );
    assert_eq!(px(m + 3 * fw, mid_y), bg, "左缘粗边到此为止（内芯 punch）");
    // 上缘单倍厚：y ∈ [m, m+W) 通墨，y = m+W 起内芯
    let mid_x = w / 2;
    let lx = mid_x - m;
    assert_eq!(px(mid_x, m + fw - 1), grad(lx, fw - 1), "上缘描边末行是墨");
    assert_eq!(px(mid_x, m + fw), bg, "上缘描边到此为止");
    // 圆角（AI_PAGE_FRAME_R > 0 的判据：直角配方角点必有墨）
    assert_eq!(px(m, m), bg, "圆角必须切掉外环角点");
    // 圆角弧起点（角点正下 R 处）回到描边上——弧参数真参与配方的判据
    let fr = tvv::AI_PAGE_FRAME_R;
    assert_eq!(px(m, m + fr), grad(0, fr), "圆角弧起点必须是渐变墨");
}

#[test]
fn spec_冒烟_ai页真消息行画在顶部区() {
    // 期 0③ 真对话页：消息行从顶部边距起画（尾随锁定前的短对话形态）——
    // 顶部区必须出现文字像素，屏心保持纯底（与空态零墨同一把尺）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let msgs = vec![
        (true, "你好".to_string(), String::new()),
        (false, "你好，有什么可以帮你？".to_string(), String::new()),
    ];
    tv.render_ai_page(&mut buf, w, h, &msgs, 0, 0, false);
    // 顶部区探针收窄到框内内容区（2026-09-04 装修：边框墨会让全宽探针
    // 的 any!=BG 恒真，失去「消息行真画了」的判别力）
    let mut top_has_ink = false;
    for y in 48..180 {
        let row = &buf[(y * w + 80) as usize..(y * w + w - 80) as usize];
        if row.iter().any(|&p| p != kfm_na::termview::AI_PAGE_BG) {
            top_has_ink = true;
            break;
        }
    }
    assert!(top_has_ink, "消息行必须真画在顶部区");
    // 屏心探针只框内内容区（2026-09-04 装修：左右边框竖边全高通墨，
    // 探针要避开边框带）
    for y in (h / 2)..(h / 2 + 40) {
        let row = &buf[(y * w + 80) as usize..(y * w + w - 80) as usize];
        assert!(
            row.iter().all(|&p| p == kfm_na::termview::AI_PAGE_BG),
            "短对话屏心必须是纯底（尾随锁定前不会有字，y={y}）"
        );
    }
}

#[test]
fn spec_冒烟_ai页角色标签配色钉() {
    // AI 名标签用 AI_PAGE_FG、用户标签用 MAG_BORDER——像素字体无抗锯齿，
    // 字形像素必等于 fg 原值（判卷成本不倒挂：这是配色契约不是 getter）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let msgs = vec![
        (true, "你好".to_string(), String::new()),
        (false, "你好，有什么可以帮你？".to_string(), String::new()),
    ];
    tv.render_ai_page(&mut buf, w, h, &msgs, 0, 0, false);
    assert!(
        buf.contains(&kfm_na::termview::AI_PAGE_FG),
        "AI 标签必须用 AI_PAGE_FG 画"
    );
    assert!(
        buf.contains(&kfm_na::termview::MAG_BORDER),
        "用户标签必须用 MAG_BORDER 画"
    );
}

#[test]
fn spec_冒烟_ai页视口滚动_追底与翻顶() {
    // 期 0④：scroll_rows 是距底行数。30 条消息 = 60 展示行，一屏 fit=7。
    // 只有 msg0 是用户（标签 MAG_BORDER），其余全 AI——追底帧不该出现
    // 用户色（msg0 早滚出去了），翻到顶帧必须在顶部区看见它。
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let fit = (h - kfm_na::termview::AI_PAGE_TOP - kfm_na::termview::AI_PAGE_BOTTOM)
        / kfm_na::termview::AI_PAGE_LINE_H;
    assert_eq!(fit, 7, "排版尺变了这题要重算（600 高 = 7 行）");
    let mut msgs = vec![(true, "first".to_string(), String::new())];
    for i in 1..30 {
        msgs.push((false, format!("reply-{i:02}"), String::new()));
    }
    // 追底：返回布局 (60, 7)；msg0 的用户标签不可见
    let mut buf = vec![0u32; (w * h) as usize];
    let (total, got_fit) = tv.render_ai_page(&mut buf, w, h, &msgs, 0, 0, false);
    assert_eq!((total, got_fit), (60, 7), "30 条消息 = 60 展示行");
    assert!(
        !buf.contains(&kfm_na::termview::MAG_BORDER),
        "追底帧：最早的用户消息必须滚出视野"
    );
    // 翻到顶（offset 拉满 = total - fit = 53）：msg0 标签必须在顶部区
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &msgs, 53, 0, false);
    let top = &buf[..(200 * w) as usize];
    assert!(
        top.contains(&kfm_na::termview::MAG_BORDER),
        "翻到顶：第一条用户消息的标签必须可见"
    );
    // offset 超上界不许 panic 不许画错位（钳制语义同状态机）
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &msgs, 10_000, 0, false);
    let top = &buf[..(200 * w) as usize];
    assert!(
        top.contains(&kfm_na::termview::MAG_BORDER),
        "超界 offset 按到顶钳制"
    );
}

#[test]
fn spec_冒烟_光球sprite画紫晕角落不染色() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (400u32, 300u32);
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_orb(&mut buf, w, h, 200.0, 150.0, 1.0, 1.0, false);
    let center = buf[(150 * w + 200) as usize];
    let (r, g, b) = ((center >> 16) & 0xFF, (center >> 8) & 0xFF, center & 0xFF);
    assert!(center != 0, "球心必须出墨（D8：确实有个球）");
    assert!(b > r && r > g, "球心必须是紫的: {center:#x}");
    assert_eq!(buf[0], 0, "远离球的角落不许染色");
    // gain=0 = 不画（透明系数 0 不许出墨）
    let mut buf2 = vec![0u32; (w * h) as usize];
    tv.render_orb(&mut buf2, w, h, 200.0, 150.0, 0.0, 1.0, false);
    assert!(buf2.iter().all(|&p| p == 0), "gain=0 不许出墨");
}

#[test]
fn spec_d8加法合成_只加光不遮光() {
    // 压字回归钉（2026-08-30 用户反馈：alpha 混合把球体暗面盖在文字上，
    // 球内笔画亮度 −32%）：sprite 改加法合成后，画在亮底上任何像素
    // 任何通道都不许变暗——球只加光不遮光（参考图 orb-on-white-ref.jpg
    // 的「文字全亮透过+球加光」效果）
    let sprite = kfm_na::ui::orb::build_orb_sprite(60.0, 1.0);
    let (w, h) = (660u32, 660u32);
    let mut buf = vec![0x0060_6060u32; (w * h) as usize]; // 亮灰底（模拟文字笔画）
    kfm_na::ui::orb::blit_orb_sprite(&mut buf, w, h, &sprite, 330.0, 330.0, 1.0);
    for (i, &p) in buf.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        assert!(
            (p >> 16) & 0xFF >= 0x60 && (p >> 8) & 0xFF >= 0x60 && p & 0xFF >= 0x60,
            "({x},{y}) 变暗了：{p:#x} < 0x606060——加法合成不许遮光"
        );
    }
}

#[test]
fn spec_d8光球配方_逐像素钉() {
    // 与 docs/assets/orb-fit-generated.png 逐像素对拍（D8 校准专跑的验收钉）：
    // build_orb_sprite + blit_orb_sprite 与 scripts/orb-fit.py render() 同公式，
    // 9 采样点容差 ±3/255（整型合成量化差 ≤1.5）。采样值 = 拟合产物 PNG 实测
    // （2026-08-30 仲裁：Python 复算公式 vs PNG 最大差 1.4/255，球心 326,330、
    // Rs=64.25、halo_gain=1.0）。加法合成后依然成立：sprite 存「拟合合成结果
    // 减底 (11,10,15)」的加值，BG 底上 底+加值 = 原合成结果，尺度由 rs 显式
    // 传参（默认球径 48→60 不影响本钉）
    let sprite = kfm_na::ui::orb::build_orb_sprite(64.25, 1.0);
    let (w, h) = (660u32, 660u32);
    // 预填参考图底色 BG=(11,10,15)（0x000B0A0F）
    let mut buf = vec![0x000B_0A0Fu32; (w * h) as usize];
    kfm_na::ui::orb::blit_orb_sprite(&mut buf, w, h, &sprite, 326.0, 330.0, 1.0);
    let ch = |x: u32, y: u32| {
        let p = buf[(y * w + x) as usize];
        ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF)
    };
    // (x, y, 目标 r/g/b)：光晕远点 / 光晕中带 / 球上缘 / 亮侧 / 高光邻 /
    // 球心 / 暗侧 / 暗面 / 球下缘
    let samples: [(u32, u32, u32, u32, u32); 9] = [
        (50, 50, 11, 10, 15),
        (326, 160, 12, 10, 18),
        (326, 230, 31, 19, 58),
        (300, 270, 51, 28, 98),
        (302, 301, 93, 47, 185),
        (326, 330, 73, 37, 144),
        (352, 330, 47, 25, 91),
        (380, 365, 52, 28, 100),
        (320, 390, 19, 12, 34),
    ];
    for (x, y, tr, tg, tb) in samples {
        let (r, g, b) = ch(x, y);
        assert!(
            r.abs_diff(tr) <= 3 && g.abs_diff(tg) <= 3 && b.abs_diff(tb) <= 3,
            "({x},{y}) 实测 ({r},{g},{b}) 偏离目标 ({tr},{tg},{tb}) 超 ±3"
        );
    }
}

#[test]
fn spec_冒烟_ai页思考块_三行钳制暗色() {
    // 期 0④½：思考折成 8 行（短行不折），流式中（live_tail=true）渲染
    // 只许出 3 行暗色（尾随自滚：正文行必须紧跟在第 3 行思考后）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let thinking = (0..8)
        .map(|i| format!("think-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let msgs = vec![(false, "正文一句话".to_string(), thinking)];
    tv.render_ai_page(&mut buf, w, h, &msgs, 0, 0, true);
    // 暗色像素聚在几个行桶里？>3 = 钳制漏了
    let line_h = kfm_na::termview::AI_PAGE_LINE_H;
    let mut buckets = std::collections::BTreeSet::new();
    for (i, &p) in buf.iter().enumerate() {
        if p == kfm_na::termview::AI_THINK_FG {
            buckets.insert((i as u32 / w) / line_h);
        }
    }
    assert!(
        buckets.len() <= 3,
        "思考块必须钳在 3 行内，实测占了 {:?} 个行桶",
        buckets.len()
    );
    assert!(!buckets.is_empty(), "思考块必须真画出来（暗色）");
    // 尾随语义：正文（DEFAULT_FG 白）必须出现在思考块下方不远处——
    // 若画的是头部 3 行，正文位置不变，这题抓不住……补一刀：思考总行
    // 8 行时若全画，正文会被推到第 10 行后；钳制后正文在第 5 行区
    let body_y = (0..h)
        .find(|&y| (0..w).any(|x| buf[(y * w + x) as usize] == kfm_na::termview::DEFAULT_FG))
        .expect("正文必须画出来");
    assert!(
        body_y < kfm_na::termview::AI_PAGE_TOP + 6 * line_h,
        "思考 8 行若不钳制正文会被推到 6 行外（实测 y={body_y}）"
    );
}

#[test]
fn spec_常量_ai页排版尺家族钉死() {
    // 排版尺 = 手势 px→行换算、渲染、考题三方的公共尺——任何一方私改
    // 就是眼手两张皮（BAR-062 同类病）。钉死防随手调
    use kfm_na::termview::{
        AI_PAGE_BOTTOM, AI_PAGE_LINE_H, AI_PAGE_MARGIN_X, AI_PAGE_PX, AI_PAGE_TOP,
    };
    assert_eq!(AI_PAGE_LINE_H, 64);
    assert_eq!(AI_PAGE_TOP, 48);
    assert_eq!(AI_PAGE_BOTTOM, 48);
    assert_eq!(AI_PAGE_MARGIN_X, 60);
    assert_eq!(AI_PAGE_PX, 40.0);
}

#[test]
fn spec_冒烟_ai页思考块_收流折叠一行() {
    // 2026-09-04 用户拍板：输出完成后思考自动折叠——全文存档不丢，
    // 渲染只留一行暗色占位（live_tail=false）。8 行思考只许出 1 行暗色
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let thinking = (0..8)
        .map(|i| format!("think-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let msgs = vec![(false, "正文一句话".to_string(), thinking)];
    let (total, _) = tv.render_ai_page(&mut buf, w, h, &msgs, 0, 0, false);
    assert_eq!(
        total, 3,
        "标签1 + 折叠占位1 + 正文1 = 3 行（8 行思考必须折没）"
    );
    let line_h = kfm_na::termview::AI_PAGE_LINE_H;
    let top = kfm_na::termview::AI_PAGE_TOP;
    let mut rowset = std::collections::BTreeSet::new();
    for (i, &p) in buf.iter().enumerate() {
        if p == kfm_na::termview::AI_THINK_FG {
            let y = i as u32 / w;
            assert!(y >= top, "暗色墨不许画到顶边距里");
            rowset.insert((y - top) / line_h); // 第几展示行（顶边距起算）
        }
    }
    assert_eq!(
        rowset.iter().copied().collect::<Vec<_>>(),
        vec![1],
        "折叠占位只许占第 2 展示行（标签行之后），实测 {:?}",
        rowset
    );
}

#[test]
fn spec_冒烟_ai页键盘让位_视口下沿收紧() {
    // 2026-09-04 用户拍板：键盘+输入栏弹起时，AI 内容追底追到栏带上沿，
    // 不许越过栏带往下画。bottom_inset=192（3 行高）→ fit 7→4，
    // 让位区必须零墨
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let inset = 3 * kfm_na::termview::AI_PAGE_LINE_H; // 192
    let mut msgs = vec![(true, "first".to_string(), String::new())];
    for i in 1..30 {
        msgs.push((false, format!("reply-{i:02}"), String::new()));
    }
    let mut buf = vec![0u32; (w * h) as usize];
    let (total, fit) = tv.render_ai_page(&mut buf, w, h, &msgs, 0, inset, false);
    assert_eq!(total, 60, "30 条消息 = 60 展示行（inset 不动总行数）");
    assert_eq!(fit, 4, "600 高 - 192 inset = 4 行视口（无 inset 是 7）");
    // 让位区（屏底 192px）零墨——任何非底色像素 = 越过栏带往下画
    let zone = &buf[((h - inset) * w) as usize..];
    assert!(
        zone.iter().all(|&p| p == kfm_na::termview::AI_PAGE_BG),
        "键盘让位区必须零墨（追底追到栏带上沿为止）"
    );
    // 追底语义保住：最新的 reply-28 必须画出来（视野里有字才算追到底）
    let view = &buf[..((h - inset) * w) as usize];
    assert!(
        view.iter().any(|&p| p != kfm_na::termview::AI_PAGE_BG),
        "收紧后的视口必须仍贴底画出最新消息"
    );
}

// ---- 期 1 第 2 层 C 档：AI 页文字接入图集管线（GPU 实例） ----
// 病根：CPU 逐字 fontdue 光栅化每帧 ~48ms。答案 = ai_page_glyphs 收集
// + ai_glyphs_to_instances 转换 + paint_ai_page_chrome 底装修。判卷：
// 与 CPU 画字路径逐像素咬合（vendored 像素字体 coverage ∈ {0,255}）。

#[test]
fn spec_gpu_ai页布局读数_cpu与gpu同尺() {
    // 眼手同尺：render_ai_page 的返回布局与 ai_page_glyphs 的读数，
    // 在 scroll × inset 全组合下逐条相等（scroll_sync_layout 只吃一份）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let mut msgs = vec![(true, "first".to_string(), String::new())];
    for i in 1..30 {
        msgs.push((false, format!("reply-{i:02}"), String::new()));
    }
    for scroll in [0u32, 10, 53, 10_000] {
        for inset in [0u32, 150, 400] {
            let mut buf = vec![0u32; (w * h) as usize];
            let cpu = tv.render_ai_page(&mut buf, w, h, &msgs, scroll, inset, false);
            let (gpu, glyphs) = tv.ai_page_glyphs(w, h, &msgs, scroll, inset, false, 0);
            assert_eq!(cpu, gpu, "scroll={scroll} inset={inset} 布局读数必须同尺");
            assert!(!glyphs.is_empty(), "有消息必有墨");
        }
    }
    // 空消息：读数 (0, fit)，零实例（空态对话框语言）
    let mut buf = vec![0u32; (w * h) as usize];
    let cpu = tv.render_ai_page(&mut buf, w, h, &[], 0, 0, false);
    let (gpu, glyphs) = tv.ai_page_glyphs(w, h, &[], 0, 0, false, 0);
    assert_eq!(cpu, gpu);
    assert!(glyphs.is_empty(), "空对话零实例");
    assert_eq!(cpu.0, 0, "空态总行数 0");
}

#[test]
fn spec_gpu_ai页字形几何_行栅格与笔位() {
    // y = 行顶 + panel_off 刚体平移，行距恒 LINE_H；x = 边距 + 18 起笔
    // （draw_items_left 同式），推进 = 逐字步进宽累加
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let off = -123i32;
    let (_, glyphs) = tv.ai_page_glyphs(
        w,
        h,
        &[(false, "AB你好".to_string(), String::new())],
        0,
        0,
        false,
        off,
    );
    assert!(!glyphs.is_empty());
    for g in &glyphs {
        let rel = g.y as i32 - off - kfm_na::termview::AI_PAGE_TOP as i32;
        assert_eq!(
            rel % kfm_na::termview::AI_PAGE_LINE_H as i32,
            0,
            "字形必须落在行栅格上（y={}）",
            g.y
        );
    }
    let first = &glyphs[0];
    assert_eq!(
        first.x,
        kfm_na::termview::AI_PAGE_MARGIN_X as f32 + 18.0,
        "起笔 = 边距 + 18 内缩（draw_items_left 同式）"
    );
    // panel_off 进 y：同输入不同平移，x 不动 y 平移
    let (_, glyphs0) = tv.ai_page_glyphs(
        w,
        h,
        &[(false, "AB你好".to_string(), String::new())],
        0,
        0,
        false,
        0,
    );
    assert_eq!(glyphs.len(), glyphs0.len(), "平移不改实例数");
    for (a, b) in glyphs.iter().zip(&glyphs0) {
        assert_eq!(a.x, b.x, "x 与平移无关");
        assert_eq!(a.y, b.y + off as f32, "y = 行顶 + 刚体平移");
    }
}

#[test]
fn spec_gpu_ai页底装修_刚体平移与直画逐像素咬合() {
    // chrome 路径（平移 k）与 CPU 直画路径（off=0）必须逐像素咬合：
    // 渐变相位/圆角弧/发光衰减全是「面板刚体」——行 y ≡ 直画行 y+k。
    // 相位若被钳原点破坏（i64 裁剪语义丢了），这题必红
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (400u32, 500u32);
    let inset = 120u32;
    let mut base = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut base, w, h, &[], 0, inset, false);
    // k=0：chrome 与直画（空消息）完全等价
    let mut b0 = vec![0u32; (w * h) as usize];
    let fit = kfm_na::termview::paint_ai_page_chrome(&mut b0, w, h, inset, 0);
    assert_eq!(fit, (h - 48 - 48 - inset) / 64, "fit 读数同尺");
    assert_eq!(b0, base, "off=0 时底装修与直画逐像素等价");
    // k=137：面板整体上移，可见区行行咬合
    let k = 137i32;
    let mut bk = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_ai_page_chrome(&mut bk, w, h, inset, -k);
    assert_eq!(
        &bk[..((h - k as u32) * w) as usize],
        &base[(k as u32 * w) as usize..],
        "平移后的可见区必须与直画逐像素咬合（相位保持）"
    );
    // 屏外余部（面板底边之下）不落墨
    assert!(
        bk[((h - k as u32) * w) as usize..].iter().all(|&p| p == 0),
        "面板底边之下必须透明（GPU 网格层透出的前提）"
    );
}

#[test]
fn spec_gpu_ai页文字实例_与cpu画字逐像素咬合() {
    // 终审判卷：GPU 实例路径（底装修 + 图集实例软件合成）与 CPU 全路径
    // 的整帧像素必须完全相等——折行/视口/路由/放置/裁剪任何一处语义
    // 漂移都会在这里现形
    use kfm_na::glyph_atlas::{GLYPH_SIZE_AI, GlyphAtlas, GlyphKey, ai_glyphs_to_instances};
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (600u32, 500u32);
    let msgs = vec![
        (true, "你好 HELLO 123".to_string(), String::new()),
        (
            false,
            "像素逐字节对拍 ABC xyz[神]".to_string(),
            String::new(),
        ),
    ];
    let mut cpu = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut cpu, w, h, &msgs, 0, 0, false);
    // GPU 路径：chrome 底装修 + 实例收集 + 图集装载（同 android_app 顺序）
    let mut gpu = vec![0u32; (w * h) as usize];
    kfm_na::termview::paint_ai_page_chrome(&mut gpu, w, h, 0, 0);
    let (_, glyphs) = tv.ai_page_glyphs(w, h, &msgs, 0, 0, false, 0);
    assert!(!glyphs.is_empty());
    let mut atlas = GlyphAtlas::new(2048, 2048);
    let baseline = tv.ai_text_baseline_off();
    // 装载遍（同 android_app：misses 补墨），转换遍只查表——借用两清
    for g in &glyphs {
        let k0 = GlyphKey {
            font: g.font,
            c: g.c,
            size: GLYPH_SIZE_AI,
        };
        if atlas.slot(&k0).is_some() {
            continue;
        }
        let (fid, m, bmp) = tv
            .rasterize_for_atlas_px(
                g.c,
                kfm_na::termview::AI_PAGE_PX,
                kfm_na::termview::AI_PAGE_PX,
            )
            .unwrap_or_else(|| panic!("字符 {} 必须可路由", g.c));
        let off_y = kfm_na::termview::ai_glyph_off_y(baseline, m.ymin as f32, m.height as f32);
        let k = GlyphKey {
            font: fid,
            c: g.c,
            size: GLYPH_SIZE_AI,
        };
        atlas.insert(
            k,
            m.width as u32,
            m.height as u32,
            &bmp,
            m.xmin as i16,
            off_y,
        );
    }
    let out = ai_glyphs_to_instances(&glyphs, &atlas, |c, font| {
        let k0 = GlyphKey {
            font,
            c,
            size: GLYPH_SIZE_AI,
        };
        match atlas.slot(&k0) {
            Some(s) => (k0, Some(s)),
            None => {
                let k1 = GlyphKey {
                    font: 1 - font,
                    c,
                    size: GLYPH_SIZE_AI,
                };
                (k0, atlas.slot(&k1))
            }
        }
    });
    assert!(out.misses.is_empty(), "装载后不许再缺墨");
    // 软件合成（GPU 语义的逐像素直译：x/y 截断同 GPU 实例公式；混合 =
    // blend_px 同款整数公式 blend(fg, dst, cov)——40px 下像素字体轮廓
    // 缩放会出反锯齿中间 coverage，判卷按真公式不复述 {0,255} 假设）
    let blend = |fg: u32, dst: u32, a: u32| {
        let inv = 255 - a;
        let ch = |f: u32, d: u32| (f * a + d * inv) / 255;
        (ch((fg >> 16) & 0xFF, (dst >> 16) & 0xFF) << 16)
            | (ch((fg >> 8) & 0xFF, (dst >> 8) & 0xFF) << 8)
            | ch(fg & 0xFF, dst & 0xFF)
    };
    for gi in &out.glyph {
        let page = &atlas.pages()[gi.page as usize];
        // 实例的 u0/v0 是归一化 UV（GPU shader 直接采样）；软件合成要还
        // 原成像素坐标（as usize 截断会把 39/2048 砍成 0——拿邻字位图
        // 画本字，2026-09-05 对拍考题逮住的第一个假凶）
        let u0 = (gi.u0 * page.w as f32).round() as usize;
        let v0 = (gi.v0 * page.h as f32).round() as usize;
        for gy in 0..gi.h as usize {
            for gx in 0..gi.w as usize {
                let cov = page.coverage[(v0 + gy) * page.w as usize + u0 + gx] as u32;
                if cov == 0 {
                    continue;
                }
                let px = gi.x as i64 + gx as i64;
                let py = gi.y as i64 + gy as i64;
                if px < 0 || py < 0 || px >= w as i64 || py >= h as i64 {
                    continue; // 视口裁（GPU 自然裁的同款）
                }
                let dst = &mut gpu[py as usize * w as usize + px as usize];
                *dst = blend(gi.fg, *dst, cov);
            }
        }
    }
    // 差异定位（首 8 个不咬合像素）——判卷失败直接给凶手坐标
    let mut diffs = String::new();
    let mut n = 0usize;
    for (i, (a, b)) in cpu.iter().zip(&gpu).enumerate() {
        if a != b {
            let (x, y) = (i % w as usize, i / w as usize);
            diffs.push_str(&format!("({x},{y},cpu={a:#010x},gpu={b:#010x}) "));
            n += 1;
        }
        if n >= 8 {
            break;
        }
    }
    assert!(diffs.is_empty(), "CPU 与 GPU 整帧咬合失败: {diffs}");
}

#[test]
fn spec_gpu_ai供墨_字号类真实生效() {
    // rasterize_for_atlas_px 的字号参数必须真实生效（图集键的 size 维
    // 就是为此而生——AI 页 40px 与终端字号两套位图共存）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (_, m_big, _) = tv
        .rasterize_for_atlas_px(
            'A',
            kfm_na::termview::AI_PAGE_PX,
            kfm_na::termview::AI_PAGE_PX,
        )
        .expect("A 必有字形");
    let (_, m_small, _) = tv
        .rasterize_for_atlas_px('A', 20.0, 20.0)
        .expect("A 必有字形");
    assert_ne!(
        (m_big.width, m_big.height),
        (m_small.width, m_small.height),
        "不同 px 光栅化必须产出不同位图"
    );
    // 同 px 两次调用确定性一致（图集缓存的正确性前提）
    let (_, m_again, _) = tv
        .rasterize_for_atlas_px(
            'A',
            kfm_na::termview::AI_PAGE_PX,
            kfm_na::termview::AI_PAGE_PX,
        )
        .unwrap();
    assert_eq!((m_big.width, m_big.height), (m_again.width, m_again.height));
    // AI 行基线：行内正偏移、小于两倍行高（放置合理性）
    let bl = tv.ai_text_baseline_off();
    assert!(bl > 0.0 && bl < kfm_na::termview::AI_PAGE_LINE_H as f32 * 2.0);
}

// ---- BAR-066：光球半透写出 + chrome 条件 alpha 直通（2026-09-05） ----
// 病灶：加法 sprite 画进透明 chrome 画布（dst=黑），条件 alpha 又把暗
// 色增量强转不透明——光球背后一整块黑。修法：(α,E) 半透写出 + 扫描
// 对自带 α 的像素直通。

#[test]
fn spec_bar066_光球半透写出_守恒与透形() {
    use kfm_na::ui::orb::{OrbSprite, blit_orb_sprite_alpha};
    // 合成 sprite：已知加量（饱和加的增量语义），判 (α,E) 写出契约
    let sprite = OrbSprite {
        size: 2,
        px: vec![
            0x0000_0000, // 全黑增量 = 不贡献
            0x0030_1020, // 弱增量（α 小）
            0x00FF_8040, // 强增量（红通道满 → α=255）
            0x0040_4040, // 等值增量
        ],
    };
    let mut buf = vec![0u32; 4];
    blit_orb_sprite_alpha(&mut buf, 2, 2, &sprite, 1.0, 1.0, 1.0);
    // 弱增量像素：α = 最大通道 0x30，E = 去预乘满亮色相
    let weak = buf[1]; // (sx1,sy0) 行主序
    let a = (weak >> 24) & 0xFF;
    let (r, g, b) = ((weak >> 16) & 0xFF, (weak >> 8) & 0xFF, weak & 0xFF);
    assert_eq!(a, 0x30, "α = 加量最大通道");
    assert_eq!(
        (r, g, b),
        (0xFF, 0x10 * 255 / 0x30, 0x20 * 255 / 0x30),
        "E = 去预乘满亮色相（整除）"
    );
    // 守恒：α·E == 原加量（GPU 标准混合的加亮项逐像素还原增量）
    let recover = |a: u32, e: u32| (a * e + 127) / 255;
    assert!(
        (recover(a, r) as i32 - 0x30).abs() <= 1
            && (recover(a, g) as i32 - 0x10).abs() <= 1
            && (recover(a, b) as i32 - 0x20).abs() <= 1,
        "α·E 必须逐像素还原加量（加亮项守恒）"
    );
    // 强增量像素：α=255 直写满色（球心等价区）
    let strong = buf[2]; // (sx0,sy1) 行主序
    assert_eq!((strong >> 24) & 0xFF, 0xFF, "满增量 α=255");
    // 全黑增量不落墨（雾外透形）
    assert_eq!(buf[0], 0, "零增量像素保持透明");
    // 雾形存在：buffer 里必须有 α<255 的像素（全不透明 = 黑块病复发）
    assert!(
        buf.iter().any(|p| *p != 0 && (*p >> 24) < 0xFF),
        "半透雾必须存在（强转不透明 = BAR-066 复发）"
    );
}

#[test]
fn spec_bar066_光球over层入口_实弹契约() {
    // render_alpha（over 层入口）实弹：真 D8 sprite 写进画布，判四条——
    // 有墨、雾外透明、画出的像素全部 α=最大通道（写出契约）、半透雾存在
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let _ = &tv;
    let (w, h) = (400u32, 400u32);
    let mut buf = vec![0u32; (w * h) as usize];
    kfm_na::ui::orb::render_alpha(&mut buf, w, h, 200.0, 200.0, 1.0, 1.0);
    let painted: Vec<u32> = buf.iter().copied().filter(|p| *p != 0).collect();
    assert!(!painted.is_empty(), "光球必须出墨");
    // 去预乘契约：E 的最大通道恒为 255（α = 原加量最大通道已挪进高字节）
    assert!(
        painted.iter().all(|p| {
            let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
            r.max(g).max(b) == 0xFF
        }),
        "E 必须是满亮色相（去预乘契约）"
    );
    assert!(
        painted.iter().any(|p| (*p >> 24) < 0xFF),
        "半透雾必须存在（黑块病复发探针）"
    );
    // 远角不染色
    assert_eq!(buf[0], 0, "远离球的角落不许染色");
}

#[test]
fn spec_bar066_条件alpha_自带透明度直通() {
    // mark_chrome_alpha 契约：纯零→透明；RGB 非零且无 α→强转不透明；
    // 自带 α（光球）→原样直通（一刀切 |= 是黑块病根）
    let mut px = vec![
        0x0000_0000, // 纯黑 → 透明
        0x0014_0a24, // AI 页暗紫（无 α）→ 强转不透明
        0x3030_1020, // 光球半透（自带 α）→ 原样
        0xFF14_0A24, // 已不透明 → 原样
    ];
    kfm_na::termview::mark_chrome_alpha(&mut px);
    assert_eq!(px[0], 0);
    assert_eq!(px[1], 0xFF14_0A24, "无 α 的可见内容必须不透明");
    assert_eq!(px[2], 0x3030_1020, "自带 α 的光球像素必须直通");
    assert_eq!(px[3], 0xFF14_0A24);
}

// ---- BAR-068：光球雾尾在半透带上不许整像素替换（2026-09-06 装机实看） ----
// 病灶：(α,E) 写出整像素替换栏带——雾尾微小 α（1%雾+99%背后）盖掉栏带
// 85% 底，背后是网格空行（GPU 清屏黑）→ 光球下缘拖出纯黑半圆盖住输入栏。
// 修：加量叠加——底有效 α（条件 alpha 三态）×E = 底已带加量，与本
// sprite 加量求和再去预乘；不透明底退化为饱和加法（与旧加法逐像素等价）。

#[test]
fn spec_bar068_光球叠加_半透带贡献不丢() {
    use kfm_na::ui::orb::{OrbSprite, blit_orb_sprite_alpha};
    // 底 = 栏带半透像素（BAR-067 写出形态）；sprite = 已知弱加量
    let mut buf = vec![0xD911_1119; 4];
    let sprite = OrbSprite {
        size: 2,
        px: vec![0x0000_0000, 0x0010_1020, 0x0000_0000, 0x0000_0000],
    };
    blit_orb_sprite_alpha(&mut buf, 2, 2, &sprite, 1.0, 1.0, 1.0);
    let out = buf[1]; // 弱加量落点
    let (a, r, g, b) = (
        (out >> 24) & 0xFF,
        (out >> 16) & 0xFF,
        (out >> 8) & 0xFF,
        out & 0xFF,
    );
    // 总加量 = 底贡献(α·E/255，同款整数舍入) + sprite 加量：
    //   r: (0x11×0xD9+127)/255 + 0x10 = 14 + 16 = 0x1E
    //   g: 同上 = 0x1E
    //   b: (0x19×0xD9+127)/255 + 0x20 = 21 + 32 = 0x35
    let band = |e: u32| (e * 0xD9 + 127) / 255;
    let (tr, tg, tb) = (band(0x11) + 0x10, band(0x11) + 0x10, band(0x19) + 0x20);
    assert_eq!(a, tb, "α = 叠加后总加量最大通道");
    assert_eq!(
        (r, g, b),
        (tr * 255 / tb, tg * 255 / tb, tb * 255 / tb),
        "E = 去预乘"
    );
    // 守恒：α·E == 总加量（底贡献 + 雾，逐通道 ±1）
    let recover = |e: u32| (a * e + 127) / 255;
    assert!((recover(r) as i32 - tr as i32).abs() <= 1);
    assert!((recover(g) as i32 - tg as i32).abs() <= 1);
    assert!((recover(b) as i32 - tb as i32).abs() <= 1);
    // 透明底像素不落墨照旧
    assert_eq!(buf[0], 0xD911_1119, "零加量像素保持底样");
}

#[test]
fn spec_bar068_光球叠加_不透明底退化饱和加法() {
    use kfm_na::ui::orb::{OrbSprite, blit_orb_sprite_alpha};
    // 底 = 无 α 但 RGB 非零（条件 alpha 语义 = 不透明，如内芯渐变像素）
    // → 退化为旧饱和加法：dst + add（逐通道钳 255）
    let mut buf = vec![0x0018_1532, 0x0018_1532, 0x0018_1532, 0x0018_1532];
    let sprite = OrbSprite {
        size: 2,
        px: vec![0x0000_0000, 0x00FF_F0F0, 0x0000_0000, 0x0000_0000],
    };
    blit_orb_sprite_alpha(&mut buf, 2, 2, &sprite, 1.0, 1.0, 1.0);
    let out = buf[1];
    // (0x18+0xFF, 0x15+0xF0, 0x32+0xF0) 全饱和 → 白
    assert_eq!(
        out & 0x00FF_FFFF,
        0x00FF_FFFF,
        "不透明底 = 旧饱和加法（逐像素等价）"
    );
    assert_eq!((out >> 24) & 0xFF, 0xFF, "饱和加出全亮 = α 满");
}
