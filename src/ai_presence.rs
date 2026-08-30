//! ai_presence.rs — AI 外显状态核（期 0 组件一；A 档纯逻辑，考题
//! tests/ai_presence_spec.rs）。规格书 docs/active/ai-presence.md v3 §五/§八。
//!
//! 两布尔模型（D7，取代三档旋钮）：`ai_running`（AI 在跑与否）×
//! `page`（终端 / AI 全屏）——状态从真事实派生，用户不管模式、只理解因果。
//! 浮层可见性 = f(ai_running, dismissed)：run_start → 现；run_end → 驻留
//! LINGER_MS 后隐；运行中上滑甩掉 → per-run dismissed，本次运行（含驻留期）
//! 不现，下次 run_start 复位；点浮层 → 跳 AI 全屏；人在 AI 全屏时 run_end
//! 不踢人（本核从不在 run_end 改 page）。
//!
//! 光球（D8/D9）：tap → page 往返；拖动 → 更新 (x,y)，边界钳制不出屏、
//! 让位快捷键行（keybar::HEIGHT_PX）与键盘 inset；pressed = 第四视觉态硬切。
//! 方法同时服务人（android_app 触摸路由）与 AI（服务调用/探针注入）——
//! 同一状态核同一套考题（D9 同源）。
//!
//! 时钟注入铁律：一切时间判定吃 `now_ms` 参数，不碰墙钟——考题喂时间戳即判。
//! 生产侧 now_ms = report::boot_ms()（进程单钟，stats/绘制/注入同一把尺）。
//!
//! 常量集中在此（D8：常量可调是硬要求；C 档实拍后微调只动这里）：

use std::sync::Mutex;

use crate::keybar;

/// 球可视半径（px，物理像素）——120px 直径，与 kfmv4 36 CSS px × DPR≈3
/// 同级（2026-08-30 用户实测参考球径）；命中半径比它大一圈（拇指友好）
pub const ORB_RADIUS_PX: u32 = 60;
/// 命中半径（px）：触摸落点与球心距离 ≤ 此值即算按住球（1.5·Rs）
pub const ORB_HIT_RADIUS_PX: u32 = 90;
/// 默认出生位（D6）：右缘（x = 屏宽 - 半径）× 屏高 60%
pub const DEFAULT_X_RATIO: f64 = 1.0;
pub const DEFAULT_Y_RATIO: f64 = 0.6;
/// run_end 后浮层驻留时长（ms）：短回复在浮层内读完的窗口
pub const LINGER_MS: u64 = 3000;
/// 长按阈值（ms）：长按球 = fake_run（debug 钩子，echo-brain 就位后可拆）
pub const LONG_PRESS_MS: u64 = 600;
/// 拖动阈值（px）：按下后位移超此值才算拖动（否则抬手 = tap）
pub const DRAG_THRESHOLD_PX: f64 = 20.0;
/// 四态增益硬切（D8；2026-08-30 加法合成后重调——加法语义下增益直接
/// 缩放光贡献量，alpha 时代旧值不复用）：整 sprite 增益 闲/运行/pressed/
/// AI页 + 运行态光晕增益；优先级 pressed > running > AI 页 > 闲。
/// 闲 = 1.0（2026-08-30 二调：用户实机裁图定量——闲 0.7 时峰值/球区/光晕
/// 全面为样式参考的 ~60%，「不明显」实锤；闲态即应 = 样式参考基准亮度，
/// 「几乎透明」由加法结构保证，不靠整体压暗）
pub const GAIN_IDLE: f32 = 1.0;
pub const GAIN_RUNNING: f32 = 1.15;
pub const HALO_GAIN_RUNNING: f32 = 1.2;
pub const GAIN_PRESSED: f32 = 1.4;
pub const GAIN_AI_PAGE: f32 = 1.0;

/// 页：终端 / AI 全屏（两布尔之一）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Terminal,
    AiFullscreen,
}

/// 状态快照（绘制/stats/探针回执的同源读数；Copy 便于逐帧比对置脏）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresenceSnap {
    pub page: Page,
    pub ai_running: bool,
    pub x: f64,
    pub y: f64,
    pub pressed: bool,
    pub overlay_visible: bool,
}

/// 四态增益（纯函数，D8）：(整 sprite 增益, 光晕增益)。光晕增益只在 running
/// 态加大（HALO_GAIN_RUNNING），其余 1.0；pressed 优先于一切（硬切无动画帧）
pub fn orb_gain(running: bool, pressed: bool, page: Page) -> (f32, f32) {
    if pressed {
        (GAIN_PRESSED, 1.0)
    } else if running {
        (GAIN_RUNNING, HALO_GAIN_RUNNING)
    } else if page == Page::AiFullscreen {
        (GAIN_AI_PAGE, 1.0)
    } else {
        (GAIN_IDLE, 1.0)
    }
}

struct Inner {
    ai_running: bool,
    page: Page,
    x: f64,
    y: f64,
    /// 首次 set_bounds 落默认出生位的标记（之后 set_bounds 只钳制不搬家）
    positioned: bool,
    pressed: bool,
    /// per-run 甩掉标记：run_start 复位
    dismissed: bool,
    /// 最近一次 run_end 的时刻（驻留期判定原料）
    run_ended_ms: Option<u64>,
    /// fake_run 的到期时刻（snap 到点自动转 run_end 等价行为）
    fake_end_ms: Option<u64>,
    /// 屏幕边界 (w, h, ime_bottom)：钳制原料，壳层 resize/键盘变化时喂
    bounds: (u32, u32, u32),
}

/// AI 外显状态核。Sync 内部可变（Mutex），形态判别同 ModifierState：
/// 共享实例直挂服务键（插件 src/plugins/ai_presence.rs），无 trait 擦除。
pub struct AiPresenceState {
    inner: Mutex<Inner>,
}

impl AiPresenceState {
    pub fn new() -> Self {
        AiPresenceState {
            inner: Mutex::new(Inner {
                ai_running: false,
                page: Page::Terminal,
                x: 0.0,
                y: 0.0,
                positioned: false,
                pressed: false,
                dismissed: false,
                run_ended_ms: None,
                fake_end_ms: None,
                bounds: (0, 0, 0),
            }),
        }
    }

    /// 喂屏幕边界（壳层：建窗/resize/键盘 inset 变化时）。首次调用落默认
    /// 出生位（右缘 × 屏高 60%，钳制后）；之后只把现位钳进新边界
    pub fn set_bounds(&self, w: u32, h: u32, ime_bottom: u32) {
        let mut g = self.inner.lock().unwrap();
        g.bounds = (w, h, ime_bottom);
        if !g.positioned {
            g.positioned = true;
            g.x = f64::from(w) * DEFAULT_X_RATIO;
            g.y = f64::from(h) * DEFAULT_Y_RATIO;
        }
        let (x, y) = clamp_pos(g.x, g.y, g.bounds);
        g.x = x;
        g.y = y;
    }

    /// AI 开跑：灯亮 + 浮层现（非用户发送触发同样现——D7 一致信号）；
    /// 不抢全屏；per-run dismissed 复位
    pub fn run_start(&self, _now_ms: u64) {
        let mut g = self.inner.lock().unwrap();
        g.ai_running = true;
        g.dismissed = false;
        g.run_ended_ms = None;
        g.fake_end_ms = None;
    }

    /// AI 跑完：灯灭，浮层进驻留期。永不改 page（人在 AI 全屏不踢人，D7）。
    /// 幂等：没在跑时调用只刷新驻留起点的反面——直接忽略
    pub fn run_end(&self, now_ms: u64) {
        let mut g = self.inner.lock().unwrap();
        if !g.ai_running {
            return;
        }
        g.ai_running = false;
        g.run_ended_ms = Some(now_ms);
        g.fake_end_ms = None;
    }

    /// 点球：终端 ↔ AI 全屏往返（光球唯一职责之一，D7）
    pub fn tap_orb(&self) {
        let mut g = self.inner.lock().unwrap();
        g.page = match g.page {
            Page::Terminal => Page::AiFullscreen,
            Page::AiFullscreen => Page::Terminal,
        };
    }

    /// 点浮层：跳 AI 全屏（单向——返回走 tap_orb）
    pub fn tap_overlay(&self) {
        self.inner.lock().unwrap().page = Page::AiFullscreen;
    }

    /// 按下/抬起（pressed = 第四视觉态硬切，D8）
    pub fn press_down(&self) {
        self.inner.lock().unwrap().pressed = true;
    }
    pub fn press_up(&self) {
        self.inner.lock().unwrap().pressed = false;
    }

    /// 拖动球到 (x, y)（状态字段，边界钳制：不出屏、让位快捷键行/键盘，D9）
    pub fn drag_to(&self, x: f64, y: f64) {
        let mut g = self.inner.lock().unwrap();
        let (x, y) = clamp_pos(x, y, g.bounds);
        g.x = x;
        g.y = y;
    }

    /// 上滑甩掉浮层：per-run dismissed——本次运行（含 run_end 驻留期）不现，
    /// 下次 run_start 复位
    pub fn dismiss_overlay(&self) {
        self.inner.lock().unwrap().dismissed = true;
    }

    /// debug 钩子（长按球触发；echo-brain 就位后可拆）：假跑一次，
    /// duration_ms 后到期自动转 run_end 等价行为（判定在 snap/tick）
    pub fn fake_run(&self, duration_ms: u64, now_ms: u64) {
        self.run_start(now_ms);
        self.inner.lock().unwrap().fake_end_ms = Some(now_ms + duration_ms);
    }

    /// 命中测试：触摸点与球心距离 ≤ ORB_HIT_RADIUS_PX 即按住球
    pub fn hit_orb(&self, x: f64, y: f64) -> bool {
        let g = self.inner.lock().unwrap();
        let dx = x - g.x;
        let dy = y - g.y;
        (dx * dx + dy * dy).sqrt() <= f64::from(ORB_HIT_RADIUS_PX)
    }

    /// 浮层可见性 = f(ai_running, dismissed, 驻留期)：甩掉 → 隐；
    /// 在跑 → 现；跑完 LINGER_MS 内 → 仍现。时间判定全吃 now_ms
    pub fn overlay_visible(&self, now_ms: u64) -> bool {
        self.snap(now_ms).overlay_visible
    }

    /// 拍快照（唯一读数出口）：先结算 fake_run 到期（到期 = run_end 等价
    /// 行为——灯灭、驻留从到期时刻起算），再算浮层可见性。生产侧
    /// now_ms = report::boot_ms()；考题喂时间戳
    pub fn snap(&self, now_ms: u64) -> PresenceSnap {
        let mut g = self.inner.lock().unwrap();
        // fake_run 到期结算：只在「还在跑」时转——真 run_end 抢先到期的
        // 不许把已结束的 run 复活（fake_end 已被 run_end 清掉，双保险）
        if g.ai_running && g.fake_end_ms.is_some_and(|end| now_ms >= end) {
            g.ai_running = false;
            g.run_ended_ms = g.fake_end_ms.take();
        }
        let overlay_visible = if g.dismissed {
            false
        } else if g.ai_running {
            true
        } else {
            g.run_ended_ms
                .is_some_and(|end| now_ms.saturating_sub(end) < LINGER_MS)
        };
        PresenceSnap {
            page: g.page,
            ai_running: g.ai_running,
            x: g.x,
            y: g.y,
            pressed: g.pressed,
            overlay_visible,
        }
    }
}

impl Default for AiPresenceState {
    fn default() -> Self {
        Self::new()
    }
}

/// 边界钳制（纯函数）：球心不出屏（四边各内缩一个可视半径）；底边在
/// 快捷键行与键盘 inset 之上让位。坏几何（屏比球小/未 set_bounds）钳到
/// 半径点保命，不出负数不出 NaN
fn clamp_pos(x: f64, y: f64, bounds: (u32, u32, u32)) -> (f64, f64) {
    let (w, h, ime_bottom) = bounds;
    let r = f64::from(ORB_RADIUS_PX);
    let max_x = (f64::from(w) - r).max(r);
    let max_y = (f64::from(h) - f64::from(ime_bottom) - f64::from(keybar::HEIGHT_PX) - r).max(r);
    (x.clamp(r, max_x), y.clamp(r, max_y))
}
