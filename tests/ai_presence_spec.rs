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
    // 空态 = 纯底零墨（2026-09-04 用户拍板撤占位提示：对话框没说话时
    // 就是空的——游戏对话框语言；占位期小字已退役）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (400u32, 300u32);
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &[], 0);
    assert_eq!(buf[0], kfm_na::termview::AI_PAGE_BG, "整屏深紫暗底");
    assert!(
        buf.iter().all(|&p| p == kfm_na::termview::AI_PAGE_BG),
        "空态必须零墨——任何非底色像素都是占位提示复活"
    );
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
    tv.render_ai_page(&mut buf, w, h, &msgs, 0);
    let top = &buf[(48 * w) as usize..(180 * w) as usize];
    assert!(
        top.iter().any(|&p| p != kfm_na::termview::AI_PAGE_BG),
        "消息行必须真画在顶部区"
    );
    let mid = &buf[((h / 2) * w) as usize..((h / 2 + 40) * w) as usize];
    assert!(
        mid.iter().all(|&p| p == kfm_na::termview::AI_PAGE_BG),
        "短对话屏心必须是纯底（尾随锁定前不会有字）"
    );
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
    tv.render_ai_page(&mut buf, w, h, &msgs, 0);
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
    let (total, got_fit) = tv.render_ai_page(&mut buf, w, h, &msgs, 0);
    assert_eq!((total, got_fit), (60, 7), "30 条消息 = 60 展示行");
    assert!(
        !buf.contains(&kfm_na::termview::MAG_BORDER),
        "追底帧：最早的用户消息必须滚出视野"
    );
    // 翻到顶（offset 拉满 = total - fit = 53）：msg0 标签必须在顶部区
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &msgs, 53);
    let top = &buf[..(200 * w) as usize];
    assert!(
        top.contains(&kfm_na::termview::MAG_BORDER),
        "翻到顶：第一条用户消息的标签必须可见"
    );
    // offset 超上界不许 panic 不许画错位（钳制语义同状态机）
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h, &msgs, 10_000);
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
    tv.render_orb(&mut buf, w, h, 200.0, 150.0, 1.0, 1.0);
    let center = buf[(150 * w + 200) as usize];
    let (r, g, b) = ((center >> 16) & 0xFF, (center >> 8) & 0xFF, center & 0xFF);
    assert!(center != 0, "球心必须出墨（D8：确实有个球）");
    assert!(b > r && r > g, "球心必须是紫的: {center:#x}");
    assert_eq!(buf[0], 0, "远离球的角落不许染色");
    // gain=0 = 不画（透明系数 0 不许出墨）
    let mut buf2 = vec![0u32; (w * h) as usize];
    tv.render_orb(&mut buf2, w, h, 200.0, 150.0, 0.0, 1.0);
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
    // 期 0④½：思考折成 8 行（短行不折），渲染只许出 3 行暗色（尾随
    // 自滚：正文行必须紧跟在第 3 行思考后，不是第 8 行后）
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (800u32, 600u32);
    let mut buf = vec![0u32; (w * h) as usize];
    let thinking = (0..8)
        .map(|i| format!("think-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let msgs = vec![(false, "正文一句话".to_string(), thinking)];
    tv.render_ai_page(&mut buf, w, h, &msgs, 0);
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
