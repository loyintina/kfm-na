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
//! 面板栈（§五B/D12，2026-09-10 用户拍板）：终端恒底，其上至多两格
//! （末位=顶，其余=被覆盖）。召唤 = 摘出重放顶（重复召唤幂等），栈满
//! 挤出栈底（静默坍缩为收起）；收起只许顶（被覆盖者不可见，无手势能
//! 收它）；遮盖者退场被覆盖者直接露出（placement 从未离靠泊位，零动画）。
//! 抽屉对称手势（四公民 2026-09-12 三缘语义）：swipe_left：顶=文件树推回 /
//! 顶=配置空操作 / 顶=解析空操作 / 否则召唤解析页；swipe_right：顶=配置
//! 推回 / 顶=解析推回 / 顶=文件树空操作 / 否则召唤文件树——任意栈态每个
//! 滑向有唯一归宿。
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
/// 默认出生位（D6；2026-08-30 用户截屏指定点）：屏宽 0.859 × 屏高 0.556
///（用户真机截屏 Screenshot_20260830_214836 实测球心 (1082,1557)@1260×2800，
/// 取代原「右缘×60%」——拇指热区内收，避开右缘手势区）
pub const DEFAULT_X_RATIO: f64 = 0.859;
pub const DEFAULT_Y_RATIO: f64 = 0.556;
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

/// 页：终端 / AI 全屏（两布尔之一；栈模型下 = 「AI 面板在栈」的派生读数，
/// 保留给旧消费方——新逻辑请读 snap.top/covered）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Terminal,
    AiFullscreen,
}

/// 面板公民（§五B/D12，2026-09-12 四公民化·三缘语义）：AI（光球垂直落，
/// 顶缘家=全局 AI）/ 配置（右缘家——设置页=独立第四页，齿轮钮唯一召唤口
/// +右滑唯一关闭口，不占任何滑槽）/ 文件树（左缘家=路由，右滑召唤、左滑
/// 推回）/ 解析（右缘家=解析器家族，左滑召唤、右滑推回，占位页先行——
/// 手势语义闭环：每个滑向在任意栈态都有唯一归宿）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Ai,
    Config,
    FileTree,
    Parser,
}

/// 水平滑向（抽屉手势识别结果，§五B 四公民）：左 = 解析家（顶是文件树则
/// 推回它——滑向来向=推回；顶是配置/解析则空操作；否则召唤解析页）；
/// 右 = 文件树家（顶是配置/解析则推回它，顶是文件树则空操作，否则召唤
/// 文件树）。顶是本家面板时反向外滑 = 空操作（一滑一义）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDir {
    Left,
    Right,
}

/// 抽屉手势最小水平位移（px，物理像素——1260 宽屏上 ≈7% 屏宽，小于
/// 屏宽 1/4 的「半页翻动」惯例：这是召唤/推回开关不是翻页）
pub const SWIPE_MIN_PX: f64 = 90.0;

/// 抽屉手势识别（纯函数，A 档钉）：水平位移够幅且方向锁（|dx| > 1.8|dy|
/// ——纵向滚屏/对话页滚行不冲突，斜滑按主轴归类）；否则 None = 不是抽屉
/// 手势，走原分路（滚屏/点按）。Started 落在哪个手势上下文由调用方仲裁
/// （球/菜单/输入栏/锚点优先），本函数只管位移几何
pub fn decide_swipe(dx: f64, dy: f64) -> Option<SwipeDir> {
    if dx.abs() >= SWIPE_MIN_PX && dx.abs() > 1.8 * dy.abs() {
        Some(if dx < 0.0 {
            SwipeDir::Left
        } else {
            SwipeDir::Right
        })
    } else {
        None
    }
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
    /// 栈顶面板（None = 终端页裸奔）
    pub top: Option<Panel>,
    /// 被覆盖面板（至多一层；None = 无）
    pub covered: Option<Panel>,
    /// AI 面板的入场代（BAR-079）：被覆盖再召唤/被挤出时 bump——同一栈态
    /// 有两种历史（新鲜召唤/覆盖再召唤），目标值纯函数算不出「该动」，代
    /// 就是这一比特历史；壳层见代变把缝重定基到屏外位 → 重播入场
    pub ai_epoch: u64,
    /// 配置面板的入场代（语义同 ai_epoch）
    pub cfg_epoch: u64,
    /// 文件树面板的入场代（语义同 ai_epoch）
    pub ft_epoch: u64,
    /// 解析面板的入场代（语义同 ai_epoch）
    pub pt_epoch: u64,
    /// 三公民页面随机 accent（宪法 §2.2）：随快照同行——壳层静态装配函数
    /// （无 self 直读状态核）与烘焙 sig 的唯一来源；AI 页不纳入（主题色恒定）
    pub accent_cfg: crate::ui::accent::AccentPair,
    pub accent_ft: crate::ui::accent::AccentPair,
    pub accent_pt: crate::ui::accent::AccentPair,
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
    /// 面板栈（§五B）：末位 = 顶，其余 = 被覆盖；至多两格，空 = 终端页
    /// （四公民不升四格：2026-09-11 用户场景序「文件树→AI→配置」关两层
    /// 直接露终端——各家完全隔离，新公民入场照样挤栈底）
    stack: Vec<Panel>,
    /// 入场代（BAR-079）：AI/配置/文件树/解析各一枚。只在「被覆盖者离被覆盖位」时
    /// bump——再召唤（坍缩②重播入场）与被挤出（坍缩③静默瞬移）；新鲜
    /// 召唤/退场/露出都不 bump（目标值翻转自足/打断不跳变/零动画露出）
    epoch_ai: u64,
    epoch_cfg: u64,
    epoch_ft: u64,
    epoch_pt: u64,
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
    /// 随机 accent（宪法 §2.2）：Config/FileTree/Parser 三页各一对，
    /// 每次召唤重新生成（AI 页不纳入，主题色蓝紫恒定）
    accent_rng: crate::ui::accent::AccentRng,
    accent_cfg: crate::ui::accent::AccentPair,
    accent_ft: crate::ui::accent::AccentPair,
    accent_pt: crate::ui::accent::AccentPair,
}

/// AI 外显状态核。Sync 内部可变（Mutex），形态判别同 ModifierState：
/// 共享实例直挂服务键（插件 src/plugins/ai_presence.rs），无 trait 擦除。
pub struct AiPresenceState {
    inner: Mutex<Inner>,
}

impl AiPresenceState {
    pub fn new() -> Self {
        // 种子 = 系统时间纳秒（壳层不注——核自给自足；零种子模块内兜底）
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        let mut accent_rng = crate::ui::accent::AccentRng::new(seed);
        AiPresenceState {
            inner: Mutex::new(Inner {
                ai_running: false,
                stack: Vec::new(),
                epoch_ai: 0,
                epoch_cfg: 0,
                epoch_ft: 0,
                epoch_pt: 0,
                x: 0.0,
                y: 0.0,
                positioned: false,
                pressed: false,
                dismissed: false,
                run_ended_ms: None,
                fake_end_ms: None,
                bounds: (0, 0, 0),
                accent_cfg: accent_rng.generate(),
                accent_ft: accent_rng.generate(),
                accent_pt: accent_rng.generate(),
                accent_rng,
            }),
        }
    }

    /// 读某面板的 accent（宪法 §2.2；Ai 页不纳入，返回 None——
    /// 壳层 AI 页走自己的主题蓝紫 token）
    pub fn accent_of(&self, p: Panel) -> Option<crate::ui::accent::AccentPair> {
        let g = self.inner.lock().unwrap();
        match p {
            Panel::Ai => None,
            Panel::Config => Some(g.accent_cfg),
            Panel::FileTree => Some(g.accent_ft),
            Panel::Parser => Some(g.accent_pt),
        }
    }

    /// 喂屏幕边界（壳层：建窗/resize/键盘 inset 变化时）。首次调用落默认
    /// 出生位（用户指定点 0.859×0.556，钳制后）；之后只把现位钳进新边界
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

    /// 点球：AI 面板的召唤/收起开关（光球唯一职责之一，D7）——栈语义
    /// （§五B）：AI 在顶 → 收起（被覆盖者露出或露终端）；否则召唤
    /// （无论它收起还是被覆盖，一律重播入场动画盖到顶）
    pub fn tap_orb(&self) {
        let mut g = self.inner.lock().unwrap();
        if g.stack.last() == Some(&Panel::Ai) {
            g.stack.pop();
        } else {
            summon_locked(&mut g, Panel::Ai);
        }
    }

    /// 点浮层：跳 AI 全屏（单向——返回走 tap_orb）；栈语义 = 召唤 AI 到顶
    pub fn tap_overlay(&self) {
        summon_locked(&mut self.inner.lock().unwrap(), Panel::Ai);
    }

    /// 召唤面板到顶（§五B）：摘出重放（幂等且保序），栈满两格先静默
    /// 挤出栈底（叠加态坍缩为收起——不可见者被丢弃无感）
    pub fn summon_panel(&self, p: Panel) {
        summon_locked(&mut self.inner.lock().unwrap(), p);
    }

    /// 收起顶面板（抽屉对称：只许收顶——被覆盖者不可见，无手势够得着）；
    /// 顶不是它 = 空操作。返回是否真收
    pub fn dismiss_top(&self, p: Panel) -> bool {
        let mut g = self.inner.lock().unwrap();
        if g.stack.last() == Some(&p) {
            g.stack.pop();
            true
        } else {
            false
        }
    }

    /// 左滑（四公民 §五B 三缘语义，2026-09-12）：顶是文件树 = 推回它的来向
    /// （左缘）；顶是配置/解析 = 空操作（右缘本家已在顶，一滑一义——设置页
    /// 手势全退位只留关闭，解析页是本方向的新家）；其余 = 召唤解析页
    pub fn swipe_left(&self) {
        let mut g = self.inner.lock().unwrap();
        match g.stack.last() {
            Some(&Panel::FileTree) => {
                g.stack.pop();
            }
            Some(&Panel::Config) | Some(&Panel::Parser) => {}
            _ => summon_locked(&mut g, Panel::Parser),
        }
    }

    /// 右滑（四公民 §五B 三缘语义，2026-09-12）：顶是配置/解析 = 推回它们的
    /// 来向（右缘——设置页唯一关闭路径）；顶是文件树 = 空操作（本家已在顶）；
    /// 其余 = 召唤文件树
    pub fn swipe_right(&self) {
        let mut g = self.inner.lock().unwrap();
        match g.stack.last() {
            Some(&Panel::Config) | Some(&Panel::Parser) => {
                g.stack.pop();
            }
            Some(&Panel::FileTree) => {}
            _ => summon_locked(&mut g, Panel::FileTree),
        }
    }

    /// 面板在栈（顶或被覆盖）：渲染目标值语义——在栈 = 靠泊位，
    /// 不在 = 屏外位（被覆盖者 placement 不动，遮盖撤走零动画露出）
    pub fn panel_present(&self, p: Panel) -> bool {
        self.inner.lock().unwrap().stack.contains(&p)
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
            // page 是栈的派生读数（AI 在栈=顶或覆盖皆 AiFullscreen——placement
            // 目标值语义）：旧消费方零改动；新逻辑读 top/covered
            page: if g.stack.contains(&Panel::Ai) {
                Page::AiFullscreen
            } else {
                Page::Terminal
            },
            ai_running: g.ai_running,
            x: g.x,
            y: g.y,
            pressed: g.pressed,
            overlay_visible,
            top: g.stack.last().copied(),
            covered: if g.stack.len() == 2 {
                Some(g.stack[0])
            } else {
                None
            },
            ai_epoch: g.epoch_ai,
            cfg_epoch: g.epoch_cfg,
            ft_epoch: g.epoch_ft,
            pt_epoch: g.epoch_pt,
            accent_cfg: g.accent_cfg,
            accent_ft: g.accent_ft,
            accent_pt: g.accent_pt,
        }
    }
}

impl Default for AiPresenceState {
    fn default() -> Self {
        Self::new()
    }
}

/// 召唤核心（栈规的唯一实体）：摘出重放顶——重复召唤幂等、被覆盖者再
/// 召唤 = 摘下重放（叠加态坍缩为收起后再入场，BAR-079：入场代 bump 让
/// 缝重播动画）；栈满两格先静默挤出栈底（第三面板入场，叠加态坍缩为
/// 收起——入场代 bump 让缝瞬移屏外，零帧空烧）
///
/// accent 语义（宪法 §2.2 召唤即随机）：被召唤者**重新生成** accent
/// （新鲜召唤与被覆盖再召唤都算召唤）；被挤出的栈底不是召唤不重生。
/// 反向重开（拖拽取消/关到一半拉开）不经过本函数 = 沿用本次配色
/// （BAR-CARD-ACCENT-01 条款的结构性兑现）
fn summon_locked(g: &mut Inner, p: Panel) {
    let was_covered = g.stack.len() == 2 && g.stack[0] == p;
    g.stack.retain(|&x| x != p);
    if g.stack.len() == 2 {
        let ejected = g.stack.remove(0); // 挤出栈底被覆盖者
        bump_epoch(g, ejected);
    }
    if was_covered {
        bump_epoch(g, p);
    }
    regen_accent(g, p);
    g.stack.push(p);
}

/// 召唤即重随 accent（AI 页不纳入卡片体系，恒主题色）
fn regen_accent(g: &mut Inner, p: Panel) {
    match p {
        Panel::Ai => {}
        Panel::Config => g.accent_cfg = g.accent_rng.generate(),
        Panel::FileTree => g.accent_ft = g.accent_rng.generate(),
        Panel::Parser => g.accent_pt = g.accent_rng.generate(),
    }
}

/// 入场代 bump（BAR-079）：壳层见代变 = 该面板缝重定基到屏外位
fn bump_epoch(g: &mut Inner, p: Panel) {
    match p {
        Panel::Ai => g.epoch_ai += 1,
        Panel::Config => g.epoch_cfg += 1,
        Panel::FileTree => g.epoch_ft += 1,
        Panel::Parser => g.epoch_pt += 1,
    }
}

/// 边界钳制（纯函数）：球心不出屏（四边各内缩一个可视半径）；底边在
/// 快捷键行 + 全局输入栏（期 0 组件三，chrome 叠高一层）与键盘 inset
/// 之上让位。坏几何（屏比球小/未 set_bounds）钳到半径点保命，不出负数不出 NaN
fn clamp_pos(x: f64, y: f64, bounds: (u32, u32, u32)) -> (f64, f64) {
    let (w, h, ime_bottom) = bounds;
    let r = f64::from(ORB_RADIUS_PX);
    let max_x = (f64::from(w) - r).max(r);
    let max_y = (f64::from(h)
        - f64::from(ime_bottom)
        - f64::from(keybar::HEIGHT_PX)
        - f64::from(crate::input_bar::HEIGHT_PX)
        - r)
        .max(r);
    (x.clamp(r, max_x), y.clamp(r, max_y))
}
