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
    (
        f64::from(W) * ai_presence::DEFAULT_X_RATIO - f64::from(ORB_RADIUS_PX),
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
    let expect = f64::from(H) - f64::from(keybar::HEIGHT_PX) - f64::from(ORB_RADIUS_PX);
    assert_eq!(ai.snap(0).y, expect, "下越界让位快捷键行（D9）");
}

#[test]
fn spec_drag_钳制_键盘弹起再让位() {
    let ai = new_state();
    ai.set_bounds(W, H, 300); // 键盘 inset 300px
    ai.drag_to(500.0, 99999.0);
    let expect = f64::from(H) - 300.0 - f64::from(keybar::HEIGHT_PX) - f64::from(ORB_RADIUS_PX);
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
fn spec_冒烟_ai页占位画紫底与文字() {
    let (tv, _, _) = kfm_na::termview::build_vendored().expect("内嵌字体必须建得成");
    let (w, h) = (400u32, 300u32);
    let mut buf = vec![0u32; (w * h) as usize];
    tv.render_ai_page(&mut buf, w, h);
    assert_eq!(buf[0], kfm_na::termview::AI_PAGE_BG, "整屏深紫暗底");
    // 居中标记文字：屏心一带必须出现非底色的文字像素（浅紫 AI_PAGE_FG）
    let mid = &buf[((h / 2 - 20) * w) as usize..((h / 2 + 20) * w) as usize];
    assert!(
        mid.iter().any(|&p| p != kfm_na::termview::AI_PAGE_BG),
        "标记文字必须真画出来"
    );
    assert!(
        mid.iter().any(|&p| {
            p != kfm_na::termview::AI_PAGE_BG
                && (p >> 16) & 0xFF > (kfm_na::termview::AI_PAGE_BG >> 16) & 0xFF
        }),
        "文字色必须亮于底色（AI_PAGE_FG 方向）"
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
    let sprite = kfm_na::termview::build_orb_sprite(60.0, 1.0);
    let (w, h) = (660u32, 660u32);
    let mut buf = vec![0x0060_6060u32; (w * h) as usize]; // 亮灰底（模拟文字笔画）
    kfm_na::termview::blit_orb_sprite(&mut buf, w, h, &sprite, 330.0, 330.0, 1.0);
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
    let sprite = kfm_na::termview::build_orb_sprite(64.25, 1.0);
    let (w, h) = (660u32, 660u32);
    // 预填参考图底色 BG=(11,10,15)（0x000B0A0F）
    let mut buf = vec![0x000B_0A0Fu32; (w * h) as usize];
    kfm_na::termview::blit_orb_sprite(&mut buf, w, h, &sprite, 326.0, 330.0, 1.0);
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
