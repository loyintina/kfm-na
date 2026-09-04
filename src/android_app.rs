//! android_app.rs — Android 壳（B 档：平台胶水，冒烟钉防退化）
//!
//! 渲染路线定案（2026-08-13，用户拍板）：**软渲染 softbuffer**。
//! 背景：本机 GPU 驱动栈（Mali-G720 Immortalis r44p1 + OriginOS）与 wgpu
//! 双后端（Vulkan/GLES）随机原生暴毙——六次实拍，死亡点在 adapter/surface/
//! configure 间漂移、零 Rust panic，非代码逻辑病；裸 winit 窗对照组稳定。
//! 终端负载（字符网格 + 光标）本就是 CPU 教科书级场景，softbuffer 零驱动
//! 依赖、行为确定。GPU 路线留档后查（git 历史 ENABLE_GFX/wgpu 时代）。
//!
//! 切片「终端渲染」（2026-08-13）：TERMINAL_MODE=true 时启动即进终端——
//! 建窗口 → softbuffer → 加载字体建 TermView → spawn 常驻 ws 会话
//! （command=None 交互 shell）→ Output 喂 Term → render_into 帧缓冲 present。
//! false 时走旧紫屏 + echo 冒烟路径（留作对照组/回退开关）。
//!
//! 已知留白（尖刺期）：
//! - ~~重绘泵忙轮询~~（2026-08-26 降频治理：WaitUntil 4ms 节拍 + 有脏才
//!   redraw，空转 57k 圈/s → ≤250 圈/s；事件到达照常即时唤醒。ws 输出
//!   最坏延迟 4ms，人不可感；proxy 全事件驱动的彻底版留待电耗专题）
//! - 键盘只翻可打印字符 + Enter/Backspace/Tab/Esc；中文 IME 走 Java 皮
//!   （KfmInputConnection.commitText → JNI → ime_queue → drain_ime_inject，
//!   2026-08-13 定案——winit native-activity 后端零 Ime 事件代码，平台层
//!   补不了，只能 Java 层接 InputConnection）

use std::num::NonZeroU32;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

use crate::base::{Base, PluginEntry};
use crate::conn::{ConnConfig, TermCmd, TermFactory};
use crate::session::SessionEvent;
use crate::termview::{self, TermEmu, TermEmuFactory};

/// KFM 紫（softbuffer 像素格式 XRGB）
const KFM_PURPLE: u32 = 0x008B_5CF6;

use crate::report::boot_ms;

/// 首笔 RedrawRequested 是否已到。唤醒锤的收锤信号：blackout 期（首笔
/// Redraw 前）外部线程 50ms 一锤 proxy user event 锤醒循环补画脏帧。
/// （2026-08-22 探针拆除案保留此机制作冗余兜底；当日「系统扣 Redraw 2.5s」
/// 后查明是自家主线程同步探针堵出来的假象，见 bugs.md/启动战役通报）
static FIRST_REDRAW_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// BAR-037 重跑防御：android_main 是否已在本进程跑过。
/// ROM 会把进程冻在 exit(0) 之前（BAR-029 保活又抬高了存活率），旧进程
/// 活着但事件循环已毁；再点图标/am start 会同进程重跑 android_main，
/// 重复起线程 + EventLoop::new 必 panic（RecreationAttempt）。第二次进门
/// 直接体面 exit(0) 让位——系统随后起的是全新进程。
static ANDROID_MAIN_RAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 终端模式开关：true = 启动即进终端画面；false = 紫屏 + echo 冒烟对照组
const TERMINAL_MODE: bool = true;

type SoftContext = softbuffer::Context<Arc<Window>>;
type SoftSurface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

/// 单指按压状态机（长按选择的壳层半：计时与事件路由；选词/扩选/提取的
/// 网格语义全在 termview 选择面）
struct Press {
    at: std::time::Instant,
    x: f64,
    y: f64,
    /// 已越过点按阈值（滚动或扩选接管），长按不再 armed
    moved: bool,
    /// 本次按压已触发长按选词——抬手只保持高亮，复制等下一击
    long_fired: bool,
}

/// 按在光球上的手势（ai-presence 期 0 组件一）：Started 命中球区记下，
/// 位移超拖动阈值 → 拖动（球跟手）；无位移短按抬起 → tap_orb 切页；
/// 长按无位移 → fake_run（debug 钩子）。Some 期间终端手势全家让路
/// （球命中优先级高于终端，D9）
struct OrbTouch {
    at: std::time::Instant,
    x: f64,
    y: f64,
    /// 已越过拖动阈值（ai_presence::DRAG_THRESHOLD_PX）——抬手不算 tap
    dragged: bool,
    /// 本次按压已触发长按 fake_run——抬手不再补 tap
    long_fired: bool,
}

/// 输入栏带手势跟踪（点按 vs 上下拖动滚动文本视口 vs 长按选区仲裁状态）
struct BarTouch {
    at: std::time::Instant,
    start_x: f64,
    start_y: f64,
    last_x: f64,
    last_y: f64,
    dragged: bool,
    /// 已触发长按进入选择模式
    long_fired: bool,
    /// 长按选词落定的词枢轴（BAR-053）：Some 期间滑指 = 词枢轴扩选
    /// （词恒整选 + 扩向指头一侧），不走文本滚动
    sel_pivot: Option<(usize, usize)>,
    /// 当前按在锚点热区上（Some = 拖动锚点；None = 普通栏手势）
    anchor: Option<crate::input_bar::SelAnchor>,
    /// 按在选择菜单浮层某格上（Some = 抬手执行该动作；BAR-046 ⑤号迭代
    /// 配套：菜单可浮出栏带，DOWN 分流时优先登记）
    menu: Option<BarMenuAction>,
}

/// AI 面板手势跟踪（期 0④）：拖动滚行 + 点按收键盘的仲裁
struct AiPageTouch {
    start_y: f64,
    last_y: f64,
    /// 行高余量累积（px）——跨 Moved 事件攒够一行才滚一行（像素级跟手）
    acc_px: f64,
    dragged: bool,
}

/// 输入栏选择操作菜单项（BAR-046）：自绘菜单四键，左→右依次
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarMenuAction {
    SelectAll,
    Copy,
    Cut,
    Paste,
}

/// 会话健康牌（断线重连 2026-08-21，按名字记账——槽位随切换翻面,
/// 死活跟名字走）：dead = Failed/Exited 钉死、Opened 复活;
/// retried = 本次死亡剧集已自动重连过一次（防断网期重连风暴烧钱:
/// 第一次自动,再死就得用户敲键/切换触发）;connecting = 重连在途
/// （Opened/再死才清——在途再触发 = 重孵,在途会话的输入缓存通道被丢）
#[derive(Clone, Copy, Debug, Default)]
struct SessHealth {
    dead: bool,
    retried: bool,
    connecting: bool,
}

struct Gfx {
    _context: SoftContext,
    surface: SoftSurface,
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    // ---- TERMINAL_MODE 状态 ----
    /// 终端实例：插件工厂产出（term-alacritty）。Arc<Mutex<>> 共享——
    /// 除 UI 线程外，闸门值守线程(gate::spawn_gate_watcher)也
    /// 持有一份：事件循环在挂起态叫不醒(proxy 实证叫不动),倒帧只能
    /// 靠值守线程自己锁终端光栅化（2026-08-24 与用户定：后台可截屏）
    term: Option<crate::gate::SharedTerm>,
    /// 出向路由核（L1 双会话：默认本地 PTY 活跃，ws 远程在待机槽）。
    /// 一切击键/IME/闸门注入出向经它发往活跃会话（评审裁决 4 附议：
    /// 输入路由抽纯数据面，考题钉在 host 侧）。Arc<Mutex<>> 共享——
    /// 闸门值守线程（keys-in 注入）也持一份；切换/重连换内脏不换 Arc,
    /// 注册一次永远新鲜
    router: Option<crate::gate::SharedRouter>,
    /// 入向面不在此——全部会话的事件通道归会话泵持有（gate::SessionPump,
    /// 2026-08-24 数据面分家）：泵是唯一消费者，UI 每圈 pump 一次 +
    /// 值守线程 300ms 一轮（挂起态网格照新，闸门眼睛不瞎）；壳只从泵
    /// 取控制事件（记健康账）和待机 replay（切换补屏）。
    /// 最近一次下发的网格尺寸（切换会话时给新活跃方补发 Resize）
    last_grid: (u32, u32),
    /// 有新输出/尺寸变化待渲染
    dirty: bool,
    /// 会话终了（exited/failed）后定格最后一屏，出向不再发
    session_over: bool,
    /// 会话健康牌 ×2（断线重连）：字段语义见 SessHealth
    health_local: SessHealth,
    health_remote: SessHealth,
    /// 真实软键盘底部 inset（px，JNI 轮询得来，BAR-006）。0 = 未弹/未知。
    /// 快捷键行的让位是 Rust 常量（keybar::HEIGHT_PX），不进本字段
    ime_bottom_px: u32,
    /// 上次 JNI 轮询时刻（500ms 节流）
    last_inset_poll: Option<std::time::Instant>,
    /// AndroidApp 句柄（JNI 用；android_main 里 clone 进来）
    android_app: Option<winit::platform::android::activity::AndroidApp>,
    /// 事件循环心跳的上次上报时刻（BAR-012③ 诊断：循环卡死则心跳停，
    /// 与「触摸没派发」区分开）
    last_loop_beat: Option<std::time::Instant>,
    /// 触摸滚动手势状态机（A 档 src/scroll.rs）：Started 建机，Moved 滚
    /// scrollback，Ended 没过阈值才算点按（唤键盘）。None = 没有按着的手指
    touch_scroll: Option<crate::scroll::TouchScroll>,
    /// 终端区活动触摸点（touch.id → 坐标，keybar 带上的不进来）：两个指头
    /// 都在终端区即进捏合（2026-08-21 双指缩放）
    touches: Vec<(u64, f64, f64)>,
    /// 捏合缩放状态：(起手指距, 起手格尺寸)。Some 期间滚动/点按/长按全让路；
    /// 任一指抬起即结束并持久化（kfm-zoom）
    pinch: Option<(f64, (u32, u32))>,
    /// 单指按压状态（长按选择计时）：Started 记录，Moved 过阈值/双指出现
    /// 即 moved 撤 armed；about_to_wait 每圈查时间戳（降频泵 4ms 一圈照准，
    /// 免定时器）——≥500ms 未动即进选择模式
    press: Option<Press>,
    /// 选区边界拖动中：Some(端点) = 手指按住了起/止边界（抬手定型后的
    /// 精调手势；2026-08-21 拖柄废除，改按住边界格直拖）。Some 期间放大镜
    /// 浮窗跟着触点走
    sel_drag: Option<crate::termview::SelEnd>,
    /// 放大镜触点（边界拖动中 Some）：draw_frame 据此在触点上方画浮窗
    magnifier_at: Option<(f64, f64)>,
    /// 按在快捷键行带上的手势（BAR-017）：Started 记下起点，Ended 命中测试
    /// 发键/翻修饰键。Some = 这手势归快捷键行，不滚屏不唤键盘
    bar_touch: Option<(f64, f64)>,
    /// 闸门触摸注入队列（通道八 touch-in）：值守线程入，about_to_wait
    /// 逐条出，sleep 指令挂起到点再取下一条
    touch_pending: std::collections::VecDeque<crate::gate::TouchCmd>,
    touch_wait_until: Option<std::time::Instant>,
    /// 插件基座（连接 provider 设计页）：持有它 = 插件服务活着
    base: Option<Base>,
    /// input.modifiers 服务句柄（input-ime 插件，方案 A：修饰键状态挂服务键）
    modifiers: Option<Arc<crate::keybar::ModifierState>>,
    /// ime.insets 服务句柄（键盘高度/强弹；生产 = JniInsets）
    ime_insets: Option<Arc<dyn crate::insets::ImeInsets>>,
    /// AiPresenceState 服务句柄（ai-presence 插件，期 0 组件一）：
    /// 光球/AI 页状态同源读数（人走触摸、AI 走服务，D9）
    ai_presence: Option<Arc<crate::ai_presence::AiPresenceState>>,
    /// AiChatState 服务句柄（ai-presence 插件，期 0③）：对话消息同源读数——
    /// 发送闭包（脑线程 apply 事件）与 AI 页渲染（snap）共这份
    ai_chat: Option<Arc<crate::ai_chat::AiChatState>>,
    /// 上一圈的对话代际（脑线程流式落格不经触摸，代际变了也要置脏画帧）
    last_chat_gen: Option<u64>,
    /// 按在光球上的手势（Some = 这手势归球，终端手势全家让路）
    orb_touch: Option<OrbTouch>,
    /// 上一帧的 AI 外显快照（about_to_wait 逐圈比对置脏：
    /// 探针注入/fake_run 到期等不经触摸的状态变化也要画出帧）
    last_ai_snap: Option<crate::ai_presence::PresenceSnap>,
    /// 全局输入栏状态核服务句柄（input-bar 插件，期 0 组件三）
    input_bar: Option<Arc<crate::input_bar::InputBarState>>,
    /// 按在输入栏带上的手势（Some = 这手势归栏，终端手势全家让路）。
    /// 拖动超 slop = 滚动文本视口；未超 = 点按（聚焦/定位/发送）
    inputbar_touch: Option<BarTouch>,
    /// 按在 AI 面板上的手势（期 0④，page=AiFullscreen 时终端手势全家
    /// 让路——不穿透）：拖动 = 对话页滚行（追底状态机），未超 slop
    /// 抬手 = 点按（输入栏失焦 + 收键盘，不召唤终端输入法）
    ai_page_touch: Option<AiPageTouch>,
    /// 本地脑（期 0②：echo-brain 夹具先行，direct-api 随 key 配置落地换插）：
    /// 输入栏发送的真 run 来源——run_start/run_end 驱动光球（期 0②收尾）
    brain: Option<Arc<dyn crate::brain_ep::BrainEndpoint>>,
    /// 上一帧的输入栏快照（about_to_wait 逐圈比对置脏）
    last_bar_snap: Option<crate::input_bar::BarSnap>,
    /// 上次量行时的屏宽（宽度变了要重新量行——捏合/旋转后折行数变）
    last_bar_w: Option<u32>,
    /// 上一圈的光标闪烁相位（聚焦时相位翻转置脏，530ms 节拍）
    last_caret_on: bool,
    /// AI 面板过渡帧离屏缓冲（采样缝 blit 用，复用免逐帧分配）：
    /// 仅在动画进行中的帧真用，硬切路径零成本
    panel_scratch: Vec<u32>,
}

/// 默认脑路（2026-09-03 用户拍板）：Kimi 卡 + k2.7 coding highspeed。
/// 备选两路已配 key：智谱 coding 套餐 glm-5.3-flash / DeepSeek 官网
/// deepseek-v4-flash-vision-exp——模型选择器是未来活（期 3 打磨），v1 定死
const DEFAULT_PROVIDER: &str = "Kimi";
const DEFAULT_MODEL: &str = "kimi-for-coding-highspeed";

/// 装配本地脑（期 0③ 换脑，D11）：私有目录 ai/providers.json + ai/.env
/// 齐且可解析 → DirectApiBrain；任一环缺/坏 → echo-brain 夹具兜底 +
/// 上报原因（未配 key 的机子 run 生命周期仍可验，回退粒度纪律）。
/// 配置文件不进 git——由 scripts/deploy-ai-config.sh 经 ssh 推送
fn assemble_brain(
    app: Option<&winit::platform::android::activity::AndroidApp>,
) -> Arc<dyn crate::brain_ep::BrainEndpoint> {
    let echo = |why: String| -> Arc<dyn crate::brain_ep::BrainEndpoint> {
        crate::report::report_sync("ai", &format!("脑装配回退 echo：{why}"));
        Arc::new(crate::brain_ep::EchoBrain::from_upstream_sse(
            include_str!("../tests/fixtures/ai-chat/upstream-kimi-k2.7-highspeed-20260830.sse"),
            std::time::Duration::from_millis(15),
        ))
    };
    let Some(dir) = app.and_then(|a| a.internal_data_path()) else {
        return echo("无私有目录句柄".to_string());
    };
    let cfg = dir.join("ai");
    let (json, env) = match (
        std::fs::read_to_string(cfg.join("providers.json")),
        std::fs::read_to_string(cfg.join(".env")),
    ) {
        (Ok(j), Ok(e)) => (j, e),
        _ => return echo(format!("{} 下 providers.json/.env 未齐", cfg.display())),
    };
    match crate::direct_brain::DirectApiBrain::from_files(&json, &env) {
        Ok(b) => {
            crate::report::report_sync(
                "ai",
                &format!("脑装配：direct-api（{DEFAULT_PROVIDER}/{DEFAULT_MODEL}）"),
            );
            Arc::new(b)
        }
        Err(e) => echo(format!("配置解析失败: {e}")),
    }
}

impl App {
    /// 当前输入栏带高（textarea 随行数长高；栏未装 = 单行默认）
    fn cur_bar_h(&self) -> u32 {
        self.input_bar
            .as_ref()
            .map_or(crate::input_bar::HEIGHT_PX, |b| {
                crate::input_bar::height_for_lines(b.lines())
            })
    }

    /// 闸门触摸注入抽干（通道八）：每圈 about_to_wait 调。Sleep 指令
    /// 挂起节拍（到点再取下一条），其余指令即刻喂 handle_touch——
    /// 与真手指同一入口，判卷尺同一把
    fn drain_touch_in(&mut self) {
        for cmd in crate::gate::touch_take() {
            self.touch_pending.push_back(cmd);
        }
        loop {
            if let Some(until) = self.touch_wait_until {
                if std::time::Instant::now() < until {
                    break; // sleep 节拍未到,剩下的下圈再取
                }
                self.touch_wait_until = None;
            }
            let Some(cmd) = self.touch_pending.pop_front() else {
                break;
            };
            use crate::gate::TouchCmd as TC;
            match cmd {
                TC::Down { id, x, y } => self.handle_touch(id, x, y, TouchPhase::Started),
                TC::Move { id, x, y } => self.handle_touch(id, x, y, TouchPhase::Moved),
                TC::Up { id, x, y } => self.handle_touch(id, x, y, TouchPhase::Ended),
                TC::Tap { x, y } => {
                    self.handle_touch(90, x, y, TouchPhase::Started);
                    self.handle_touch(90, x, y, TouchPhase::Ended);
                }
                TC::Scroll { lines } => self.inject_scroll(lines),
                TC::Sleep { ms } => {
                    self.touch_wait_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                }
            }
            self.dirty = true;
        }
    }

    /// scroll 语法糖展开:n>0 = 看历史 = 手指下扫(scroll.rs 契约:y 增大
    /// = 正行数)。从内容区上 1/4 处起指,分 4 步模拟真手指的 moved 序列,
    /// 终点钳在内容区内。
    /// 几何取 last_grid + cell_size,**不取 window**——挂起态窗口已弃
    /// (BAR-004)但网格活着,注入不许跟着瞎(2026-08-27 实拍:window 早退
    /// 让 scroll 语法糖在挂起态静默空转,裸事件反而通——钉此防回潮)
    fn inject_scroll(&mut self, lines: i32) {
        let (cols, rows) = self.last_grid;
        if cols == 0 || rows == 0 {
            return; // 终端还没建几何,空转不如明退
        }
        let cell_h = self
            .term_handle()
            .map(|t| t.lock().unwrap().cell_size().1)
            .unwrap_or(crate::termview::CELL_H);
        let area_w = f64::from(cols)
            * f64::from(
                self.term_handle()
                    .map(|t| t.lock().unwrap().cell_size().0)
                    .unwrap_or(crate::termview::CELL_W),
            );
        let area_h = f64::from(rows) * f64::from(cell_h);
        let cx = area_w / 2.0;
        let y0 = area_h * 0.25;
        let y1 = (y0 + f64::from(lines) * f64::from(cell_h)).clamp(10.0, area_h * 0.7);
        crate::report::report(
            "gate",
            &format!("scroll 注入 {lines} 行展开: ({cx:.0},{y0:.0})→({cx:.0},{y1:.0})"),
        );
        self.handle_touch(90, cx, y0, TouchPhase::Started);
        for i in 1..=4 {
            let y = y0 + (y1 - y0) * f64::from(i) / 4.0;
            self.handle_touch(90, cx, y, TouchPhase::Moved);
        }
        self.handle_touch(90, cx, y1, TouchPhase::Ended);
    }

    /// 触摸统一入口(2026-08-27 通道八 touch-in):真手指(winit Touch)与
    /// 闸门注入双喂同一函数——判卷尺与真实手势同一把。本体从原
    /// WindowEvent::Touch 臂机械搬家,一行逻辑未动(fmt 收尾)
    fn handle_touch(&mut self, id: u64, x: f64, y: f64, phase: TouchPhase) {
        if !TERMINAL_MODE {
            return;
        }
        match phase {
            TouchPhase::Started => {
                static FIRST_TOUCH: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !FIRST_TOUCH.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::report::report("ime", "首个触摸进 handler（派发活着）");
                }
                // 光球命中优先级高于终端（ai-presence 期 0 组件一，D9）：
                // 按下命中球区 → 这手势归球（pressed 置位 = 第四视觉态硬切；
                // 拖动/点按/长按在 Moved/Ended/check_orb_long_press 分路）
                if let Some(ai) = &self.ai_presence
                    && ai.hit_orb(x, y)
                {
                    ai.press_down();
                    self.orb_touch = Some(OrbTouch {
                        at: std::time::Instant::now(),
                        x,
                        y,
                        dragged: false,
                        long_fired: false,
                    });
                    self.dirty = true;
                    return;
                }
                // 选择菜单浮层命中（BAR-046 2026-09-03 ⑤号迭代配套）：菜单
                // 气泡可浮出栏带盖在终端区上（贴选区），不再被栏带几何包住——
                // 命中检查必须先于栏带/终端分流，否则浮出栏带的菜单格点了
                // 没反应。仅登记，抬手（Ended 臂 bt.menu 分路）才执行
                if let Some(menu) = self.hit_selection_menu(x, y) {
                    self.inputbar_touch = Some(BarTouch {
                        at: std::time::Instant::now(),
                        start_x: x,
                        start_y: y,
                        last_x: x,
                        last_y: y,
                        dragged: false,
                        long_fired: false,
                        sel_pivot: None,
                        anchor: None,
                        menu: Some(menu),
                    });
                    return;
                }
                // 输入栏命中（期 0 组件三）：起点在栏带上 → 这手势归栏
                // （不滚屏不唤键盘——聚焦/发送在 Ended 分路）。
                // 带高随行数走（textarea 长高，眼手同尺）
                let bar_h = self.cur_bar_h();
                let in_input_bar = self.window.as_ref().is_some_and(|w| {
                    crate::input_bar::in_bar(y, w.inner_size().height, self.ime_bottom_px, bar_h)
                });
                if in_input_bar {
                    // 选择态下先检查是否按在锚点热区上（锚点命中优先级最高）
                    let anchor = self.hit_selection_anchor(x, y);
                    self.inputbar_touch = Some(BarTouch {
                        at: std::time::Instant::now(),
                        start_x: x,
                        start_y: y,
                        last_x: x,
                        last_y: y,
                        dragged: false,
                        long_fired: false,
                        sel_pivot: None,
                        anchor,
                        menu: None,
                    });
                    return;
                }
                // AI 面板靠泊中（期 0④）：面板上只有输入栏是活区——其余
                // 位置手势归对话页（拖动滚行/点按收键盘），终端手势全家
                // 让路（快捷键行热区也不许穿透：面板盖着它，点得着看不
                // 见 = 幽灵键）
                let ai_page = self
                    .last_ai_snap
                    .is_some_and(|s| s.page == crate::ai_presence::Page::AiFullscreen);
                if ai_page {
                    self.ai_page_touch = Some(AiPageTouch {
                        start_y: y,
                        last_y: y,
                        acc_px: 0.0,
                        dragged: false,
                    });
                    return;
                }
                // 起点在快捷键行带上 → 这手势归行（不滚屏不唤键盘）
                // BAR-018：判定尺与渲染/hit 一致——减去键盘 inset，
                // 否则键盘弹起时行带浮在 inset 上方，这里却认屏底。
                // 期 0 组件三：行上移一层（输入栏压底），有效 inset + 当前栏高
                let in_bar = self.window.as_ref().is_some_and(|w| {
                    crate::keybar::in_bar(y, w.inner_size().height, self.ime_bottom_px + bar_h)
                });
                if in_bar {
                    self.bar_touch = Some((x, y));
                    return;
                }
                // 终端区指头登记（keybar 带上的不进来）
                self.touches.push((id, x, y));
                // 双指都在终端区 → 捏合缩放：挂起滚动/点按/长按
                // （touch_scroll/press 清掉，残余指头抬手前不接管任何手势）
                if self.touches.len() == 2 && self.bar_touch.is_none() && self.pinch.is_none() {
                    let ((_, x1, y1), (_, x2, y2)) = (self.touches[0], self.touches[1]);
                    let dist0 = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(1.0);
                    let base = self
                        .term_handle()
                        .map(|t| t.lock().unwrap().cell_size())
                        .unwrap_or((crate::termview::CELL_W, crate::termview::CELL_H));
                    self.pinch = Some((dist0, base));
                    self.touch_scroll = None;
                    self.press = None;
                    self.sel_drag = None;
                    self.magnifier_at = None;
                    crate::report::report(
                        "zoom",
                        &format!("捏合开始: dist0={dist0:.0} base={}x{}", base.0, base.1),
                    );
                    return;
                }
                if self.touches.len() > 2 {
                    return; // 第三指起不接管
                }
                let selecting = self
                    .term_handle()
                    .is_some_and(|t| t.lock().unwrap().selection_active());
                // 选择态按住边界格 → 端点精调（放大镜随触点浮起）；
                // 不记 press——边界抬手不触发复制
                if selecting
                    && let Some(end) = self
                        .term_handle()
                        .and_then(|t| t.lock().unwrap().hit_boundary(x, y))
                {
                    self.sel_drag = Some(end);
                    self.magnifier_at = Some((x, y));
                    crate::report::report("ime", &format!("边界按住: {end:?}"));
                    return;
                }
                // 单指：记按压（长按计时，RedrawRequested 里查）；
                // 选择态下不建滚动机——拖动 = 扩选
                self.press = Some(Press {
                    at: std::time::Instant::now(),
                    x,
                    y,
                    moved: false,
                    long_fired: false,
                });
                if !selecting {
                    let cell_h = self
                        .term_handle()
                        .map(|t| t.lock().unwrap().cell_size().1)
                        .unwrap_or(crate::termview::CELL_H);
                    self.touch_scroll = Some(crate::scroll::TouchScroll::new(y, f64::from(cell_h)));
                }
            }
            TouchPhase::Moved => {
                // 输入栏带手势：锚点拖动 > 滚动 > 长按候选
                // field_h 供边缘判定（框界）；view_h 供滚动钳制（BAR-049
                // 文本视口高，与渲染同尺）
                let field_h = self.cur_bar_h().saturating_sub(64);
                let view_h = crate::input_bar::text_view_h(field_h);
                if let Some(bt) = self.inputbar_touch.as_mut() {
                    let _dx = x - bt.last_x;
                    let dy = y - bt.last_y;
                    bt.last_x = x;
                    bt.last_y = y;
                    if let Some(anchor) = bt.anchor {
                        // 拖动锚点：钳制版换算 char 下标（BAR-055 出界不冻结），
                        // 换锚语义 setter 回传指头和前持有的锚（BAR-056 交叉不断）
                        if let Some(idx) = self.bar_field_char_at_clamped(x, y)
                            && let Some(bar) = &self.input_bar
                        {
                            let held = match anchor {
                                crate::input_bar::SelAnchor::Left => bar.set_selection_start(idx),
                                crate::input_bar::SelAnchor::Right => bar.set_selection_end(idx),
                                _ => anchor,
                            };
                            if let Some(bt) = self.inputbar_touch.as_mut() {
                                bt.anchor = Some(held);
                            }
                        }
                        // 拖到 field 上下边缘自动滚屏
                        self.bar_edge_autoscroll(y, field_h, view_h);
                        self.dirty = true;
                    } else if bt.long_fired
                        && let Some(pivot) = bt.sel_pivot
                    {
                        // 长按后滑指 = 词枢轴扩选（BAR-053）：词恒整选，
                        // 扩向指头一侧；与锚点拖动同享边缘自动滚屏。
                        // 双端原子落跨度（BAR-056：拆两发会被换锚截胡）
                        if let Some(idx) = self.bar_field_char_at_clamped(x, y)
                            && let Some(bar) = &self.input_bar
                        {
                            let (s, e) = crate::input_bar::pivot_drag_span(pivot, idx);
                            bar.set_selection_span(s, e);
                        }
                        self.bar_edge_autoscroll(y, field_h, view_h);
                        self.dirty = true;
                    } else if bt.menu.is_some() {
                        // 菜单浮层手势：滑出 slop 记拖（抬手不执行动作），
                        // 不滚文本——菜单不是文本区
                        if !bt.dragged
                            && ((y - bt.start_y).abs() > crate::scroll::TAP_SLOP_PX
                                || (x - bt.start_x).abs() > crate::scroll::TAP_SLOP_PX)
                        {
                            bt.dragged = true;
                        }
                    } else {
                        if !bt.dragged
                            && ((y - bt.start_y).abs() > crate::scroll::TAP_SLOP_PX
                                || (x - bt.start_x).abs() > crate::scroll::TAP_SLOP_PX)
                        {
                            bt.dragged = true;
                        }
                        if bt.dragged {
                            // 像素级 1:1 跟手:手指位移直进视口偏移(下拖=回头部)
                            if let Some(bar) = &self.input_bar {
                                bar.scroll_by_px(-(dy as i32), view_h);
                            }
                            self.dirty = true;
                        }
                    }
                }
                // AI 面板手势：拖动 = 对话页滚行（像素级累积跟手，行高
                // 与渲染同尺 AI_PAGE_LINE_H；上滑 = 看更早 = 偏移+）
                if let Some(apt) = self.ai_page_touch.as_mut() {
                    let dy = y - apt.last_y;
                    apt.last_y = y;
                    if (y - apt.start_y).abs() > crate::scroll::TAP_SLOP_PX {
                        apt.dragged = true;
                    }
                    apt.acc_px -= dy; // 手指上滑 dy<0 → 累积为正 = 看更早
                    let line_h = f64::from(crate::termview::AI_PAGE_LINE_H);
                    let rows = (apt.acc_px / line_h).trunc() as i32;
                    if rows != 0 {
                        apt.acc_px -= f64::from(rows) * line_h;
                        if let Some(chat) = &self.ai_chat {
                            chat.scroll_drag_rows(rows);
                        }
                        self.dirty = true;
                    }
                    return;
                }
                // 指头坐标跟新（捏合测距用）
                for t in &mut self.touches {
                    if t.0 == id {
                        t.1 = x;
                        t.2 = y;
                    }
                }
                // 捏合：dist/dist0 比例 × 起手格尺寸，钳制后整数变化
                // ≥1px 才应用（防抖）——set_cell_size 重算字几何，
                // apply_window_size 触发 resize（alacritty 自带 reflow）
                if let Some((dist0, base)) = self.pinch {
                    if self.touches.len() >= 2 {
                        let ((_, x1, y1), (_, x2, y2)) = (self.touches[0], self.touches[1]);
                        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
                        let (cw, ch) =
                            crate::termview::pinch_cell_size(base.0, base.1, dist / dist0);
                        if self.term_handle().map(|t| t.lock().unwrap().cell_size())
                            != Some((cw, ch))
                        {
                            if let Some(t) = self.term_handle() {
                                t.lock().unwrap().set_cell_size(cw, ch);
                            }
                            if let Some(w) = &self.window {
                                let s = w.inner_size();
                                self.apply_window_size(s.width, s.height);
                            }
                        }
                    }
                    return;
                }
                // 光球手势：越过拖动阈值即 dragged，球跟手（边界钳制在
                // 状态核 drag_to）；未过阈值不动球（等抬手判 tap / 长按）
                if let Some(ot) = &mut self.orb_touch {
                    if (x - ot.x).abs() >= crate::ai_presence::DRAG_THRESHOLD_PX
                        || (y - ot.y).abs() >= crate::ai_presence::DRAG_THRESHOLD_PX
                    {
                        ot.dragged = true;
                    }
                    if ot.dragged
                        && let Some(ai) = &self.ai_presence
                    {
                        ai.drag_to(x, y);
                        self.dirty = true;
                    }
                    return;
                }
                if self.bar_touch.is_some() {
                    return; // 快捷键行手势：不支持拖动
                }
                // 边界拖动：端点跟手指走（跨行/历史区换算在
                // move_selection_end），放大镜跟着触点浮
                if let Some(end) = self.sel_drag {
                    if let Some(t) = self.term_handle() {
                        t.lock().unwrap().move_selection_end(end, x, y);
                    }
                    self.magnifier_at = Some((x, y));
                    self.dirty = true;
                    return;
                }
                // 过阈值撤长按 armed（选择态/滚动态同一把尺）
                if let Some(p) = &mut self.press
                    && ((x - p.x).abs() >= crate::scroll::TAP_SLOP_PX
                        || (y - p.y).abs() >= crate::scroll::TAP_SLOP_PX)
                {
                    p.moved = true;
                }
                // 选择态：拖动 = 扩选（不滚屏，坐标含 display_offset/边距，
                // 换算在 termview grid_point_at）
                if self
                    .term_handle()
                    .is_some_and(|t| t.lock().unwrap().selection_active())
                {
                    if let Some(t) = self.term_handle() {
                        t.lock().unwrap().extend_selection(x, y);
                    }
                    self.dirty = true;
                    return;
                }
                let Some(tracker) = &mut self.touch_scroll else {
                    return;
                };
                let lines = tracker.moved(y);
                if lines == 0 {
                    return;
                }
                let Some(t) = self.term_handle() else { return };
                let mut t = t.lock().unwrap();
                if t.mouse_report_active() {
                    // BAR-016②：对端开了鼠标上报（tmux/kimicode 等全屏
                    // TUI）——alt screen 没有本地历史可滚，翻成 SGR 滚轮
                    // 事件发 PTY，让对方滚自己的视图
                    let (cw, ch) = t.cell_size();
                    let col = (x as u32 / cw + 1).max(1);
                    let row = (y as u32 / ch + 1).max(1);
                    if let Some(r) = self.router_handle() {
                        let r = r.lock().unwrap();
                        // 每次事件按行数发滚轮 tick，封顶防一次猛拖雪崩
                        for _ in 0..lines.unsigned_abs().min(10) {
                            r.send(TermCmd::Input(crate::scroll::wheel_seq(
                                lines > 0,
                                col,
                                row,
                            )));
                        }
                    }
                } else {
                    t.scroll_lines(lines);
                    self.dirty = true;
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.touches.retain(|t| t.0 != id);
                // 捏合收尾：任一指抬起即结束，缩放比写盘 + [zoom] 上报。
                // 残余指头不接管滚动/点按（touch_scroll/press 进捏合时已清）
                if self.pinch.take().is_some() {
                    self.persist_zoom();
                    return;
                }
                // 光球手势收尾：pressed 复位；无位移短按抬起 → tap 切页
                // （Cancelled / 拖过 / 长按已发 fake_run 的抬手不补 tap）
                if let Some(ot) = self.orb_touch.take() {
                    if let Some(ai) = &self.ai_presence {
                        ai.press_up();
                        if phase == TouchPhase::Ended && !ot.dragged && !ot.long_fired {
                            ai.tap_orb();
                        }
                    }
                    self.dirty = true;
                    return;
                }
                // 输入栏手势收尾(Ended|Cancelled 臂)
                if let Some(bt) = self.inputbar_touch.take() {
                    if phase == TouchPhase::Cancelled {
                        return; // 取消:丢弃
                    }
                    // 锚点拖动结束：保持选择，重绘
                    if bt.anchor.is_some() {
                        self.dirty = true;
                        return;
                    }
                    // 长按选词/枢轴扩选结束：保持选择（BAR-053——原案漏这
                    // 一臂，选区落进 Field 点按分路被 set_cursor 顺手清掉，
                    // 刚召唤即销毁）
                    if bt.long_fired {
                        self.dirty = true;
                        return;
                    }
                    // 拖动结束（滚动/扩选/滑出菜单）不当点按
                    if bt.dragged {
                        self.dirty = true;
                        return;
                    }
                    // 菜单浮层点按：执行 DOWN 时登记的动作（浮层可出栏带，
                    // 命中已在 DOWN 分流判过，这里只管执行）
                    if let Some(menu) = bt.menu {
                        self.execute_bar_menu(menu);
                        self.dirty = true;
                        return;
                    }
                    let bar_h = self.cur_bar_h();
                    let action = self.window.as_ref().and_then(|w| {
                        let s = w.inner_size();
                        crate::input_bar::hit(x, y, s.width, s.height, self.ime_bottom_px, bar_h)
                    });
                    let selecting = self.input_bar.as_ref().is_some_and(|b| b.snap().selecting);
                    match action {
                        Some(crate::input_bar::BarHit::Field) => {
                            if let Some(bar) = &self.input_bar {
                                bar.focus();
                                // 选择模式下：先检查菜单命中，再检查选区外点按
                                if selecting && let Some(menu) = self.hit_selection_menu(x, y) {
                                    self.execute_bar_menu(menu);
                                    self.dirty = true;
                                    return;
                                }
                                // 点按定位光标（浏览器控件行为）
                                if let Some(idx) = self.bar_field_char_at(x, y) {
                                    bar.set_cursor(idx);
                                }
                            }
                            if let Some(w) = &self.window {
                                w.set_ime_allowed(true);
                            }
                            if let Some(insets) = &self.ime_insets {
                                insets.force_show();
                            }
                            crate::report::report("ime", "输入栏聚焦（弹键盘）");
                        }
                        Some(crate::input_bar::BarHit::Send) => {
                            if let Some(bar) = &self.input_bar {
                                let sent = bar.submit();
                                crate::report::report("ai", &format!("输入栏发送: {sent:?}"));
                            }
                        }
                        None => {}
                    }
                    self.dirty = true;
                    return;
                }
                // AI 面板手势收尾：点按（未拖过 slop）= 输入栏失焦 +
                // 收键盘——面板不是输入区，绝不穿透召唤终端输入法（期 0④
                // 用户拍板两条：不穿透 + 点非输入区自动收键盘）
                if let Some(apt) = self.ai_page_touch.take() {
                    if phase == TouchPhase::Ended && !apt.dragged {
                        if self.input_bar.as_ref().is_some_and(|b| b.is_focused())
                            && let Some(bar) = &self.input_bar
                        {
                            bar.unfocus();
                        }
                        if let Some(w) = &self.window {
                            w.set_ime_allowed(false);
                        }
                        if let Some(insets) = &self.ime_insets {
                            insets.force_hide();
                        }
                        crate::report::report("ime", "AI 页点按：收键盘不穿透");
                    }
                    self.dirty = true;
                    return;
                }
                // 快捷键行手势：抬手命中发键（Cancelled 不发）
                if self.bar_touch.take().is_some() {
                    // BAR-018 诊断：进得了这个分支 = Started 的 in_bar
                    // 判定活着；hit 落空也会留痕（坐标+inset 三数）
                    crate::report::report(
                        "ime",
                        &format!("快捷键行抬手 ({},{}), inset={}", x, y, self.ime_bottom_px),
                    );
                    if phase != TouchPhase::Ended {
                        return;
                    }
                    let Some(w) = &self.window else { return };
                    let s = w.inner_size();
                    let Some(kd) = crate::keybar::hit(
                        x,
                        y,
                        s.width,
                        s.height,
                        self.ime_bottom_px + self.cur_bar_h(),
                    ) else {
                        crate::report::report(
                            "ime",
                            &format!(
                                "快捷键行命中落空: 窗 {}x{} inset={}",
                                s.width, s.height, self.ime_bottom_px
                            ),
                        );
                        return;
                    };
                    // BAR-018 诊断：点哪个键报哪个键——实拍「PgUp
                    // 表现得像↑」必须分清命中错还是对端不认
                    crate::report::report("ime", &format!("快捷键行点按: {}", kd.label));
                    match kd.key {
                        crate::keybar::Key::Direct(code) => {
                            crate::ime_queue::global().push_key_code(code);
                        }
                        crate::keybar::Key::Modifier(bit) => {
                            let m = self.modifiers.as_ref().map_or(0, |ms| ms.toggle(bit));
                            crate::report::report("ime", &format!("修饰键粘滞位: {m:03b}"));
                        }
                        crate::keybar::Key::None => {}
                    }
                    self.dirty = true; // 修饰键变色/下帧重画
                    return;
                }
                let press = self.press.take();
                // 边界抬手：定型保持高亮，不复制（Cancelled 同样只收尾）
                if self.sel_drag.take().is_some() {
                    self.magnifier_at = None;
                    self.dirty = true;
                    return;
                }
                // 选择态：抬手保持高亮；单击（未拖动扩选、且不是刚触发
                // 长按的那次抬手）→ 复制 + Toast + 清选。点按唤键盘让路
                if self
                    .term_handle()
                    .is_some_and(|t| t.lock().unwrap().selection_active())
                {
                    let tap = press.is_some_and(|p| !p.moved && !p.long_fired);
                    if tap && phase == TouchPhase::Ended {
                        self.copy_selection();
                    }
                    return;
                }
                let was_tap = self.touch_scroll.take().is_some_and(|t| t.was_tap());
                if was_tap && let Some(w) = &self.window {
                    // 焦点二态（§五）：点终端区 = 输入栏失焦（键盘留给终端）
                    if self.input_bar.as_ref().is_some_and(|b| b.is_focused()) {
                        if let Some(bar) = &self.input_bar {
                            bar.unfocus();
                        }
                        crate::report::report("ime", "点终端区：输入栏失焦");
                    }
                    w.set_ime_allowed(true);
                    if let Some(insets) = &self.ime_insets {
                        insets.force_show();
                    }
                    crate::report::report("ime", "点按唤出软键盘");
                }
            }
        }
    }

    /// JNI 轮询真实键盘高度（500ms 节流）：winit 的 Ime::Enabled/Disabled 在
    /// 本机从未触发（全日志零条），事件驱动是死路，轮询才是活路（BAR-006）。
    /// 值变了才 resize + 上报——resize 会抖动服务器 pty，不能跟着轮询抖
    fn poll_ime_inset(&mut self) {
        let now = std::time::Instant::now();
        if let Some(t) = self.last_inset_poll
            && now.duration_since(t) < std::time::Duration::from_millis(500)
        {
            return;
        }
        self.last_inset_poll = Some(now);
        let Some(insets) = &self.ime_insets else {
            return;
        };
        // None = 查询失败：维持旧值不抖动
        let Some(px) = insets.ime_bottom_px() else {
            return;
        };
        if px != self.ime_bottom_px {
            crate::report::report("ime", &format!("键盘 inset 变化: {px}px"));
            self.ime_bottom_px = px;
            if let Some(w) = &self.window {
                let s = w.inner_size();
                self.apply_window_size(s.width, s.height);
            }
        }
    }

    /// 长按计时（about_to_wait 每圈查时间戳，免定时器——2026-08-26 从
    /// RedrawRequested 挪来：降频泵后重绘是条件触发，空圈不再 redraw）：
    /// 单指按压 ≥500ms 未移动 → 进选择模式，选中落点词（termview 选择面）
    fn check_long_press(&mut self) {
        let Some(p) = &mut self.press else { return };
        if p.long_fired || p.moved || self.pinch.is_some() {
            return;
        }
        if p.at.elapsed() < std::time::Duration::from_millis(500) {
            return;
        }
        p.long_fired = true;
        let (x, y) = (p.x, p.y);
        if let Some(t) = self.term_handle() {
            t.lock().unwrap().select_word_at(x, y);
            self.dirty = true;
            crate::report::report("ime", "长按选词——进入选择模式");
        }
    }

    /// 光球长按计时（与 check_long_press 同制，about_to_wait 每圈查）：
    /// 按住球 ≥LONG_PRESS_MS 未拖动 → fake_run(3000)。
    /// **debug 钩子**（规格书 §五：echo-brain 就位后可拆）——假跑一次验证
    /// 灯亮/浮层/stats 全链，不接任何真 AI
    fn check_orb_long_press(&mut self) {
        let Some(ot) = &mut self.orb_touch else {
            return;
        };
        if ot.long_fired || ot.dragged {
            return;
        }
        if ot.at.elapsed() < std::time::Duration::from_millis(crate::ai_presence::LONG_PRESS_MS) {
            return;
        }
        ot.long_fired = true;
        if let Some(ai) = &self.ai_presence {
            ai.fake_run(3000, crate::report::boot_ms() as u64);
        }
        self.dirty = true;
        crate::report::report("ai", "长按光球 → fake_run(3000)（debug 钩子）");
    }

    /// 输入栏长按计时（BAR-046）：按住栏内文本区 ≥SELECT_LONG_PRESS_MS
    /// 未拖动 → 进入选择模式。锚点命中时不走这里。
    /// BAR-053：改长按选词（落点词整段高亮）+ 登记词枢轴（续滑扩选用）；
    /// 空文本/无词可选不点火（保持原滚动/点按行为）。
    fn check_inputbar_long_press(&mut self) {
        let Some(bt) = &mut self.inputbar_touch else {
            return;
        };
        if bt.long_fired || bt.dragged || bt.anchor.is_some() || bt.menu.is_some() {
            return;
        }
        if bt.at.elapsed()
            < std::time::Duration::from_millis(crate::input_bar::SELECT_LONG_PRESS_MS)
        {
            return;
        }
        let (x, y) = (bt.start_x, bt.start_y);
        if let Some(idx) = self.bar_field_char_at(x, y)
            && let Some(bar) = &self.input_bar
            && let Some(span) = bar.enter_selection_word(idx)
        {
            let Some(bt) = &mut self.inputbar_touch else {
                return;
            };
            bt.long_fired = true;
            bt.sel_pivot = Some(span);
            self.dirty = true;
            crate::report::report("ime", &format!("输入栏长按 → 选词 {span:?} 进入选择模式"));
        }
    }

    /// 拖到 field 上下边缘自动滚屏（BAR-046 锚点拖动/BAR-053 枢轴扩选
    /// 共用一把尺：每秒 2 行≈每帧 8px）
    fn bar_edge_autoscroll(&self, y: f64, field_h: u32, view_h: u32) {
        let bar_h = self.cur_bar_h();
        let field_top = self.window.as_ref().map_or(0, |w| {
            w.inner_size()
                .height
                .saturating_sub(self.ime_bottom_px + bar_h)
                + 32
        }) as f64;
        let edge = 12.0;
        if y - field_top < edge
            && let Some(bar) = &self.input_bar
        {
            bar.scroll_by_px(-8, view_h);
        } else if (field_top + f64::from(field_h)) - y < edge
            && let Some(bar) = &self.input_bar
        {
            bar.scroll_by_px(8, view_h);
        }
    }

    /// 屏坐标 → 输入栏文本区 char 下标（BAR-046）。复用 `bar_cursor_at` 几何，
    /// 点按在 field 外返回 None。
    fn bar_field_char_at(&self, x: f64, y: f64) -> Option<usize> {
        let w = self.window.as_ref()?.inner_size().width;
        let h = self.window.as_ref()?.inner_size().height;
        let bar_h = self.cur_bar_h();
        let ime_bottom = self.ime_bottom_px;
        let top = h.checked_sub(ime_bottom)?.checked_sub(bar_h)?;
        let field_top = top + 32;
        let field_h = bar_h.checked_sub(64)?;
        let field_left = crate::input_bar::MARGIN_X_PX;
        let send_left = w
            .checked_sub(crate::input_bar::MARGIN_X_PX)?
            .checked_sub(crate::input_bar::SEND_W_PX)?;
        let field_w = send_left
            .checked_sub(crate::input_bar::GAP_PX)?
            .checked_sub(field_left)?;
        if x < f64::from(field_left)
            || x >= f64::from(field_left + field_w)
            || y < f64::from(field_top)
            || y >= f64::from(field_top + field_h)
        {
            return None;
        }
        let bar = self.input_bar.as_ref()?;
        let term = self.term_handle()?;
        let snap = bar.snap();
        let x_local = x - f64::from(field_left + 40);
        let y_local = y - f64::from(field_top);
        Some(
            term.lock()
                .unwrap()
                .bar_cursor_at(&snap, w, x_local, y_local),
        )
    }

    /// bar_field_char_at 的拖动连续态钳制版（BAR-055）：指头滑出文本框
    /// 上下沿/抓柄拖到框外时按最近边换算（clamp_to_field），不再 None
    /// 冻结——旧尺在拖动中指头一越界就停更，实拍「上下挪一下断触」。
    /// 仅拖锚点/枢轴扩选的 Moved 连续态用；点按/命中判定仍用严格版。
    fn bar_field_char_at_clamped(&self, x: f64, y: f64) -> Option<usize> {
        let w = self.window.as_ref()?.inner_size().width;
        let h = self.window.as_ref()?.inner_size().height;
        let bar_h = self.cur_bar_h();
        let ime_bottom = self.ime_bottom_px;
        let top = h.checked_sub(ime_bottom)?.checked_sub(bar_h)?;
        let field_top = top + 32;
        let field_h = bar_h.checked_sub(64)?;
        let field_left = crate::input_bar::MARGIN_X_PX;
        let send_left = w
            .checked_sub(crate::input_bar::MARGIN_X_PX)?
            .checked_sub(crate::input_bar::SEND_W_PX)?;
        let field_w = send_left
            .checked_sub(crate::input_bar::GAP_PX)?
            .checked_sub(field_left)?;
        let (cx, cy) =
            crate::input_bar::clamp_to_field(x, y, field_left, field_top, field_w, field_h);
        let bar = self.input_bar.as_ref()?;
        let term = self.term_handle()?;
        let snap = bar.snap();
        let x_local = cx - f64::from(field_left + 40);
        let y_local = cy - f64::from(field_top);
        Some(
            term.lock()
                .unwrap()
                .bar_cursor_at(&snap, w, x_local, y_local),
        )
    }

    /// 判断是否按在选择锚点热区上（BAR-046）。热区以锚点柄视觉中心
    /// （几何 left/right_anchor）为心、ANCHOR_HIT_SIZE 为边长的正方形。
    fn hit_selection_anchor(&self, x: f64, y: f64) -> Option<crate::input_bar::SelAnchor> {
        let w = self.window.as_ref()?.inner_size().width;
        let h = self.window.as_ref()?.inner_size().height;
        let bar = self.input_bar.as_ref()?;
        let snap = bar.snap();
        if !snap.selecting {
            return None;
        }
        let term = self.term_handle()?;
        let geo = term
            .lock()
            .unwrap()
            .bar_selection_geometry(&snap, w, h, self.ime_bottom_px)?;
        let half = f64::from(crate::input_bar::ANCHOR_HIT_SIZE) / 2.0;
        let in_hot =
            |px: f64, py: f64| x >= px - half && x < px + half && y >= py - half && y < py + half;
        if in_hot(geo.left_anchor.0, geo.left_anchor.1) {
            Some(crate::input_bar::SelAnchor::Left)
        } else if in_hot(geo.right_anchor.0, geo.right_anchor.1) {
            Some(crate::input_bar::SelAnchor::Right)
        } else {
            None
        }
    }

    /// 判断是否命中选择操作菜单四键之一（BAR-046）。
    /// 顺序左→右：全选 | 复制 | 剪切 | 粘贴。
    fn hit_selection_menu(&self, x: f64, y: f64) -> Option<BarMenuAction> {
        let w = self.window.as_ref()?.inner_size().width;
        let h = self.window.as_ref()?.inner_size().height;
        let bar = self.input_bar.as_ref()?;
        let snap = bar.snap();
        if !snap.selecting {
            return None;
        }
        let term = self.term_handle()?;
        let geo = term
            .lock()
            .unwrap()
            .bar_selection_geometry(&snap, w, h, self.ime_bottom_px)?;
        let fx = f64::from(geo.menu_x);
        let fy = f64::from(geo.menu_y);
        let fw = f64::from(geo.menu_w);
        let fh = f64::from(geo.menu_h);
        if x < fx || x >= fx + fw || y < fy || y >= fy + fh {
            return None;
        }
        let btn_w = fw / 4.0;
        let idx = ((x - fx) / btn_w).floor() as usize;
        match idx {
            0 => Some(BarMenuAction::SelectAll),
            1 => Some(BarMenuAction::Copy),
            2 => Some(BarMenuAction::Cut),
            3 => Some(BarMenuAction::Paste),
            _ => None,
        }
    }

    /// 执行输入栏选择菜单动作（BAR-046）。复制/剪切/粘贴都走系统剪贴板。
    fn execute_bar_menu(&mut self, action: BarMenuAction) {
        let Some(bar) = &self.input_bar else { return };
        match action {
            BarMenuAction::SelectAll => bar.select_all(),
            BarMenuAction::Copy => {
                if let Some(text) = bar.selected_text()
                    && let Some(app) = &self.android_app
                {
                    crate::clipboard::copy_and_toast(app, &text);
                }
            }
            BarMenuAction::Cut => {
                if let Some(text) = bar.selected_text()
                    && let Some(app) = &self.android_app
                {
                    crate::clipboard::copy_and_toast(app, &text);
                }
                bar.delete_selection();
            }
            BarMenuAction::Paste => {
                if let Some(text) = self.paste_from_clipboard() {
                    bar.insert_or_replace(&text);
                }
            }
        }
        self.dirty = true;
    }

    /// 从系统剪贴板读文本（BAR-046）。JNI 任一环节失败只返回 None，不 panic。
    fn paste_from_clipboard(&self) -> Option<String> {
        let app = self.android_app.as_ref()?;
        crate::clipboard::get_clipboard_text(app)
    }

    /// AI 外显快照逐圈比对置脏：探针注入（通道十直调状态核）/fake_run
    /// 到期/run 驻留翻隐等不经壳层触摸的状态变化也要画出帧
    fn poll_ai_presence(&mut self) {
        let Some(ai) = &self.ai_presence else { return };
        let snap = ai.snap(crate::report::boot_ms() as u64);
        if self.last_ai_snap != Some(snap) {
            self.last_ai_snap = Some(snap);
            self.dirty = true;
        }
        // 对话代际比对（期 0③）：脑线程流式落格不经触摸/快照，
        // 代际变了也要画出帧（AI 页尾随的命）
        if let Some(chat) = &self.ai_chat {
            let g = chat.generation();
            if self.last_chat_gen != Some(g) {
                self.last_chat_gen = Some(g);
                self.dirty = true;
            }
        }
    }

    /// 输入栏快照逐圈比对置脏（闸门注入/IME 分流改的状态也要画出帧）。
    /// 量行写回（textarea 眼手同尺单源）：文本/屏宽变了先量行 set_lines
    /// 写回状态核，再 snap——触摸命中/渲染/dump 读的都是同一份行数
    fn poll_input_bar(&mut self) {
        let Some(bar) = self.input_bar.clone() else {
            return;
        };
        let cur = bar.snap();
        let w = self
            .window
            .as_ref()
            .map(|w| w.inner_size().width)
            .unwrap_or(0);
        let stale = self
            .last_bar_snap
            .as_ref()
            .is_none_or(|p| p.text != cur.text)
            || self.last_bar_w != Some(w);
        if stale {
            if let Some(t) = self.term_handle() {
                // 量行原料 = 显示文本(组合态拼入,行数跟所见走)
                let display = crate::input_bar::InputBarState::display_text(&cur);
                let lines = t.lock().unwrap().bar_text_lines(&display, w);
                bar.set_lines(lines);
            }
            self.last_bar_w = Some(w);
        }
        let snap = bar.snap();
        if self.last_bar_snap.as_ref() != Some(&snap) {
            self.last_bar_snap = Some(snap);
            self.dirty = true;
        }
        // 光标闪烁相位逐圈比对置脏（聚焦时 530ms 相位翻转不经触摸也要画帧）
        if bar.is_focused() {
            let on = (crate::report::boot_ms() as u64 / crate::input_bar::CARET_BLINK_MS)
                .is_multiple_of(2);
            if on != self.last_caret_on {
                self.last_caret_on = on;
                self.dirty = true;
            }
        }
    }

    /// 选择态单击复制：提取选中文字 → JNI 系统剪贴板 + Toast，清高亮。
    /// 提取为空（按在空白格）不打扰剪贴板，只清选区
    fn copy_selection(&mut self) {
        let Some(t) = self.term_handle() else { return };
        let mut t = t.lock().unwrap();
        if let Some(text) = t.selected_text() {
            let n = text.chars().count();
            if n > 0
                && let Some(app) = &self.android_app
            {
                crate::clipboard::copy_and_toast(app, &text);
            }
        }
        t.clear_selection();
        self.dirty = true;
    }

    /// 缩放比持久化路径：应用 files 目录下 kfm-zoom（ndk
    /// internal_data_path，与 exec_probe 同一取法，不硬编码）
    fn zoom_path(&self) -> Option<std::path::PathBuf> {
        self.android_app
            .as_ref()
            .and_then(|a| a.internal_data_path())
            .map(|p| p.join("kfm-zoom"))
    }

    /// 捏合收尾写盘：缩放比浮点（相对编译期基准 CELL_W/CELL_H），
    /// 冷启动读回（init_terminal）；写失败只上报——缩放不该炸终端
    fn persist_zoom(&self) {
        let (Some(term), Some(path)) = (self.term_handle(), self.zoom_path()) else {
            return;
        };
        let (cw, ch) = term.lock().unwrap().cell_size();
        let ratio = f64::from(cw) / f64::from(crate::termview::CELL_W);
        match std::fs::write(&path, format!("{ratio:.4}")) {
            Ok(()) => crate::report::report(
                "zoom",
                &format!("缩放持久化: ratio={ratio:.2} cell={cw}x{ch}"),
            ),
            Err(e) => crate::report::report("zoom", &format!("缩放写盘失败: {e}")),
        }
    }

    /// 初始化 softbuffer（上下文 + 表面），按窗口尺寸配置
    fn init_gfx(window: &Arc<Window>) -> Gfx {
        let context = softbuffer::Context::new(window.clone()).expect("创建 softbuffer 上下文失败");
        crate::report::report("boot", "softbuffer 上下文建成");
        let mut surface =
            softbuffer::Surface::new(&context, window.clone()).expect("创建 softbuffer 表面失败");
        crate::report::report("boot", &format!("softbuffer 表面建成 +{}ms", boot_ms()));
        let size = window.inner_size();
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            surface.resize(w, h).expect("surface resize 失败");
        }
        Gfx {
            _context: context,
            surface,
        }
    }

    /// 终端模式初始化：建终端（插件工厂）+ spawn 常驻会话（插件工厂）+ 首发 resize
    fn init_terminal(&mut self, window: &Arc<Window>) {
        // BAR-004 后台往返重开会话的路径：旧会话的死亡标记必须清掉，
        // 否则键盘/IME 输入被 session_over 挡死，新会话成了哑巴
        self.session_over = false;
        // 全量重建 = 生死簿重开（断线重连的健康牌归零；待机缓存在泵里,
        // 下面装配时同名 register 自清）
        self.health_local = SessHealth::default();
        self.health_remote = SessHealth::default();

        // exec 探针(L2/L3 总开关,exec_probe.rs):私有目录 exec 放行与否
        // 决定 busybox/apt 生态路线。冷启动一次,结果走飞鸽传书。
        // 后台线程跑(2026-08-21 实测:同步跑吃 2283ms 占 init 96%,大头是
        // report_sync 阻塞 HTTP;探针结果 v1 只上报不分支,没资格堵启动)
        if let Some(app) = &self.android_app
            && let Some(dir) = app.internal_data_path()
        {
            // 登记 AndroidApp 句柄供闸门剪贴板 JNI 使用（BAR-046）
            crate::gate::register_android_app(app.clone());
            std::thread::spawn(move || {
                crate::exec_probe::run(&dir);
            });
        }

        // 插件基座：终端模拟器 + 连接 provider（边界手术第一/二刀）——
        // 「用哪个终端芯、连哪、怎么连」都不归主循环；工厂是服务，实例归调用方。
        // 瞬时返回契约预算 50ms 是 harness 政策(G5 归层:cordis-na 默认关,
        // 这里显式开启,规格书 §4.3)
        let base = Base::new(vec![
            PluginEntry {
                id: crate::plugins::conn_provider_ws::PLUGIN_NAME,
                disabled: false,
                config: Some(Box::new(|| {
                    Arc::new(ConnConfig::default()) as Arc<dyn std::any::Any + Send + Sync>
                })),
            },
            // 新插件上线纪律：disabled 一键关,默认开(回退第一层)——
            // 翻 true 即整插件不激活,状态核/光球/AI 页全下线
            PluginEntry {
                id: crate::plugins::ai_presence::PLUGIN_NAME,
                disabled: false,
                config: None,
            },
        ])
        .with_apply_budget(std::time::Duration::from_millis(50));
        if let Err(e) = base.load(crate::plugins::term_alacritty::TermAlacritty::new()) {
            crate::report::report_sync("term", &format!("终端插件装载失败: {e:?}"));
        }
        if let Err(e) = base.load(crate::plugins::conn_provider_ws::ConnProviderWs::new()) {
            crate::report::report_sync("term", &format!("连接插件装载失败: {e:?}"));
        }
        // 输入/IME 插件（边界手术第三刀，方案 A）：修饰键状态 + 键盘来源两个
        // 共享实例直挂。JniInsets 持 AndroidApp 句柄（运行时对象，构造注入）
        if let Some(app) = &self.android_app {
            let input = crate::plugins::input_ime::InputIme::new(Arc::new(
                crate::insets::JniInsets::new(app.clone()),
            ));
            if let Err(e) = base.load(input) {
                crate::report::report_sync("ime", &format!("输入插件装载失败: {e:?}"));
            }
            self.modifiers = base.ctx().get::<crate::keybar::ModifierState>().ok();
            self.ime_insets = base.ctx().get::<dyn crate::insets::ImeInsets>().ok();
            // JNI 桥端点：commitText 回调线程拿不到 ctx，装入服务实例句柄
            if let Some(m) = &self.modifiers {
                crate::keybar::install_bridge_mods(m.clone());
            }
        } else {
            crate::report::report_sync("ime", "无 AndroidApp 句柄——输入插件未装");
        }

        // AI 外显插件（期 0 组件一）：状态核共享实例直挂。壳层（光球绘制/
        // 触摸路由）与闸门（stats 字段族/通道十注入）同读这一份（D9 同源）；
        // 装载失败只上报——球没了终端照跑（回退粒度纪律）
        if let Err(e) = base.load(crate::plugins::ai_presence::AiPresence::new()) {
            crate::report::report_sync("ai", &format!("AI 外显插件装载失败: {e:?}"));
        }
        self.ai_presence = base.ctx().get::<crate::ai_presence::AiPresenceState>().ok();
        self.ai_chat = base.ctx().get::<crate::ai_chat::AiChatState>().ok();
        if let Some(ai) = &self.ai_presence {
            crate::gate::register_ai_presence(ai);
        }
        if let Some(chat) = &self.ai_chat {
            crate::gate::register_ai_chat(chat);
        }

        // 全局输入栏插件（期 0 组件三）：状态核共享实例直挂 + 发送口装配。
        // 脑 = 配置驱动（期 0③ 换脑，D11 本地直连是地基）：私有目录
        // ai/providers.json + ai/.env 齐 → DirectApiBrain；缺/坏 →
        // echo-brain 夹具兜底并上报（未配 key 也可验 run 生命周期，
        // 回退粒度纪律）。发送闭包在触摸/值守线程被调，真 run 自开
        // 线程——瞬时返回契约
        if let Err(e) = base.load(crate::plugins::input_bar::InputBar::new()) {
            crate::report::report_sync("ai", &format!("输入栏插件装载失败: {e:?}"));
        }
        // ui-fx 动画插件（ui-base §五，采样缝第一消费者）：占「AI 面板
        // Y 偏移」缝播弹簧落下；装载失败/禁用 = 不占槽 = 全局硬切
        // （功能等价只是变糙——纪律条款「拔动画插件功能等价」）
        if let Err(e) = base.load(crate::plugins::ui_fx::UiFx::new()) {
            crate::report::report_sync("ui", &format!("ui-fx 插件装载失败: {e:?}"));
        }
        self.input_bar = base.ctx().get::<crate::input_bar::InputBarState>().ok();
        if let (Some(bar), Some(ai)) = (&self.input_bar, &self.ai_presence) {
            crate::gate::register_input_bar(bar);
            let brain: Arc<dyn crate::brain_ep::BrainEndpoint> =
                assemble_brain(self.android_app.as_ref());
            self.brain = Some(brain.clone());
            let ai2 = ai.clone();
            let chat = self.ai_chat.clone();
            bar.install_sender(Arc::new(move |text| {
                let Some(chat) = &chat else {
                    crate::report::report("ai", "发送被吞：AI 对话状态核未就位");
                    return;
                };
                // 用户消息入格 + 全量历史投影（OpenAI 无状态，每轮全量上传）
                let history = chat.user_send(&text);
                let brain = brain.clone();
                let ai = ai2.clone();
                let chat = chat.clone();
                std::thread::spawn(move || {
                    ai.run_start(crate::report::boot_ms() as u64);
                    let req = crate::brain_ep::ChatStartReq {
                        session_id: "local".to_string(),
                        messages: history,
                        model: DEFAULT_MODEL.to_string(),
                        provider: DEFAULT_PROVIDER.to_string(),
                        tools: vec![],
                    };
                    let (_h, rx) = brain.start(req);
                    while let Ok(ev) = rx.recv() {
                        let end = matches!(
                            ev,
                            crate::brain::ChatEvent::Done | crate::brain::ChatEvent::Error { .. }
                        );
                        chat.apply(&ev);
                        if end {
                            break;
                        }
                    }
                    ai.run_end(crate::report::boot_ms() as u64);
                });
            }));
        }

        // L3 首启安装(必须在本地会话 spawn 前:装好后 shell_plan 才会
        // 换成 $PREFIX/bin/bash)。幂等——非首启秒过(只查 prefix 非空)
        if let Some(app) = &self.android_app {
            crate::bootstrap::first_boot_install(app);
            // L2:kfm-pkg 每启覆盖铺进 $PREFIX/bin(版本随 APK 滚动)
            crate::bootstrap::ensure_pkg_tool(app);
        }
        // 双会话（L1，多端分层设计页 §3）：本地 PTY 秒开为默认活跃会话——
        // 零网络，冷进程首连 ~2.1s 唤醒成本（BAR-022/023 归因）不在此路径；
        // ws 远程会话后台接为待机，Ctrl-] 切换（并存可切换，不自动接管）。
        // spawn 提前到基座就绪即刻的传统保留（BAR-022：与建终端/字体加载并行）
        if let Err(e) = base.load(crate::plugins::conn_provider_local::ConnProviderLocal::new()) {
            crate::report::report_sync("term", &format!("本地连接插件装载失败: {e:?}"));
        }
        let local = match base.ctx().get::<crate::local_pty::LocalPtyFactory>() {
            Ok(factory) => Some(factory.spawn(&factory.default_config())),
            Err(e) => {
                crate::report::report_sync("term", &format!("本地会话工厂取回失败: {e:?}"));
                None
            }
        };
        let remote = match base.ctx().get::<dyn TermFactory>() {
            Ok(factory) => Some(factory.spawn(&factory.default_config())),
            Err(e) => {
                crate::report::report_sync("term", &format!("远程连接工厂取回失败: {e:?}"));
                None
            }
        };
        match (local, remote) {
            (Some(l), Some(r)) => {
                let mut router = crate::session_router::SessionRouter::new(l.outbound, "local");
                if let Err(e) = router.add_standby(r.outbound, "remote") {
                    crate::report::report_sync("term", &format!("路由装配失败: {e}"));
                }
                crate::gate::pump_register("local", l.events);
                crate::gate::pump_register("remote", r.events);
                self.install_router(router);
            }
            // 兜底：本地挂了远程顶上（单会话退化，行为同 L1 前）
            (None, Some(r)) => {
                crate::report::report_sync("term", "本地会话断裂——退化纯远程模式");
                crate::gate::pump_register("remote", r.events);
                self.install_router(crate::session_router::SessionRouter::new(
                    r.outbound, "remote",
                ));
            }
            (Some(l), None) => {
                crate::report::report_sync("term", "远程连接断裂——纯本地模式");
                crate::gate::pump_register("local", l.events);
                self.install_router(crate::session_router::SessionRouter::new(
                    l.outbound, "local",
                ));
            }
            (None, None) => {
                crate::report::report_sync("term", "双会话全灭——本屏无会话");
            }
        }
        // 建终端：经基座取终端工厂；build 失败 = 字体全灭走 Err（裁决 3，非插件失败）
        let Some((tv, _font_path, cjk_path)) = (match base.ctx().get::<dyn TermEmuFactory>() {
            Ok(factory) => match factory.build() {
                Ok(built) => Some(built),
                Err(e) => {
                    crate::report::report_sync("term", &e);
                    None
                }
            },
            Err(e) => {
                crate::report::report_sync("term", &format!("终端工厂取回失败: {e:?}"));
                None
            }
        }) else {
            return;
        };
        // CJK 备用字体全灭是产品级风险（中文画 tofu），留一行预警；
        // 其余启动计时探针已拆（2026-08-22 探针拆除案，数字见 git 历史）
        if cjk_path.is_none() {
            crate::report::report("term", "CJK 备用字体全灭——中文画 tofu");
        }
        // （BAR-021：诊断脚手架已拆——候选体检/目录普查每个冷启动全量解析
        // 44MB×2+32MB 巨物，是启动慢的最大单块成本；探测链本身也已退役，
        // 生产字体编译期内嵌。需要排查时从 git 历史恢复）
        let term = std::sync::Arc::new(std::sync::Mutex::new(tv));
        crate::gate::register_dump_term(&term); // 后台倒帧值守持有
        self.term = Some(term);
        self.base = Some(base);

        // 捏合缩放持久化读回（kfm-zoom，files 目录）：有记录则按基准×比例
        // 应用（没有就用编译期基准 CELL_W/CELL_H）；在首发 apply_window_size
        // 之前落位，首帧即缩放后的几何
        if let Some(path) = self.zoom_path()
            && let Ok(s) = std::fs::read_to_string(&path)
            && let Ok(ratio) = s.trim().parse::<f64>()
        {
            let (cw, ch) = crate::termview::pinch_cell_size(
                crate::termview::CELL_W,
                crate::termview::CELL_H,
                ratio,
            );
            if let Some(t) = self.term_handle() {
                t.lock().unwrap().set_cell_size(cw, ch);
            }
        }

        // 首发尺寸：Opened 前 outbound 会被 conn 层缓存，绑定后补发
        let size = window.inner_size();
        self.apply_window_size(size.width, size.height);

        // 上机提示(L1 实拍后用户要「至少一个提示」):app 级快捷键 shell
        // 看不见,开局直接印在网格上(只 feed 视图,不进 PTY 不污染会话)。
        // 每次冷启动印一次;滚屏可回看。
        // 同时 tap 进飞行记录仪(按启动时活跃名)——它上了屏就是屏幕事实,
        // 不记则「回放末屏=读屏」判卷每次冷启动都差这 5 行(2026-08-25 实拍)
        // BAR-040:必须在 apply_window_size 之后印——先在 BOOT 80 列印、
        // 再 resize 到真机 61 列,重排折行 +2 会把标题顶出视野
        // (2026-08-27 用户实拍,考题 tests/termview_spec.rs spec_bar040_*)
        if let Some(t) = self.term_handle() {
            t.lock()
                .unwrap()
                .feed(crate::termview::HELP_BANNER.as_bytes());
            let active = self
                .router_handle()
                .map_or("local", |r| r.lock().unwrap().active_name());
            crate::gate::rec_output(active, crate::termview::HELP_BANNER.as_bytes());
        }
        self.dirty = true;
    }

    /// 窗口 px 尺寸 → cols/rows → Term resize + terminal-resize 出向。
    /// 可用区域 = 窗口 - 四周边距（BAR-005）- 真实软键盘 inset（BAR-006，
    /// JNI 轮询，insets.rs）- 快捷键行高（BAR-017，Rust 自绘常驻让位）。
    /// 顶带跟当前格高走（margin_top：捏合缩放后格高可变，2026-08-21）
    fn apply_window_size(&mut self, w: u32, h: u32) {
        // 光球边界钳制原料（首次调用落默认出生位；键盘 inset 变化也走这里）
        if let Some(ai) = &self.ai_presence {
            ai.set_bounds(w, h, self.ime_bottom_px);
        }
        let Some(term) = self.term_handle() else {
            return;
        };
        let (cw, ch) = term.lock().unwrap().cell_size();
        let usable_w = w.saturating_sub(2 * termview::MARGIN_X);
        let usable_h = h.saturating_sub(
            termview::margin_top(ch)
                + termview::MARGIN_Y
                + self.ime_bottom_px
                + crate::keybar::HEIGHT_PX
                + crate::input_bar::HEIGHT_PX, // 期 0 组件三：输入栏常驻让位
                                               // （textarea 覆盖式悬浮：网格只让单行带高，栏长高向上浮盖终端
                                               // 底部行——不触发 resize→SIGWINCH→重绘洪峰链，nz case-002 教训）
        );
        let (cols, rows) = termview::grid_dims(usable_w, usable_h, cw, ch);
        term.lock().unwrap().resize_cells(cols, rows);
        self.last_grid = (cols, rows);
        // 飞行记录仪:尺寸事件落带(回放网格几何的锚点;名字记当时活跃方)
        if let Some(r) = self.router_handle() {
            let name = r.lock().unwrap().active_name();
            crate::gate::rec_resize(name, cols, rows, cw, ch);
        }
        if !self.session_over
            && let Some(r) = self.router_handle()
        {
            r.lock().unwrap().send(TermCmd::Resize { cols, rows });
        }
        self.dirty = true;
    }

    /// 会话切换（L1）：Ctrl-] 触达——router 换出向活跃槽；入向不换槽
    /// （全部 rx 归会话泵持有，路由按活跃名走）。待机期缓存的输出从泵
    /// 取 replay 补屏；给新活跃方补发当前网格尺寸；横幅直接喂进终端
    /// 网格（不走对端）。切入死会话 → 立即重连（用户在场，断线重连
    /// 2026-08-21）
    fn switch_session(&mut self) {
        // 锁即取即还——后面补屏循环要借 self 别处
        let Some((name_a, name_s)) = self
            .router_handle()
            .and_then(|r| r.lock().unwrap().switch())
        else {
            return; // 没待机方：装作没发生(或没路由装配)
        };
        // 待机期缓存的输出补屏：死会话的遗屏也喂——用户看得到「死前最后
        // 画面」,比重连后的白屏亲切;活的会话更必须(输出连续)
        let replay = crate::gate::pump_take_replay(name_s);
        if let Some(t) = self.term_handle() {
            let mut g = t.lock().unwrap();
            for chunk in &replay {
                g.feed(chunk.as_bytes());
            }
        }
        let (cols, rows) = self.last_grid;
        if let Some(router) = self.router_handle() {
            router.lock().unwrap().send(TermCmd::Resize { cols, rows });
        }
        if let Some(t) = self.term_handle() {
            let banner =
                format!("\r\n\x1b[36m[kfm-na → {name_s} 会话（Ctrl-] 切回 {name_a}）]\x1b[0m\r\n");
            t.lock().unwrap().feed(banner.as_bytes());
        }
        self.session_over = self.health(name_s).dead;
        crate::report::report("term", &format!("会话切换: {name_a} → {name_s}"));
        if self.session_over {
            self.kick_reconnect(); // 切入死会话 = 立即重连
        }
        self.dirty = true;
    }

    /// 健康牌按名查（槽位随切换翻面，死活跟名字走）
    fn health(&self, name: &str) -> SessHealth {
        if name == "local" {
            self.health_local
        } else {
            self.health_remote
        }
    }

    fn health_mut(&mut self, name: &str) -> &mut SessHealth {
        if name == "local" {
            &mut self.health_local
        } else {
            &mut self.health_remote
        }
    }

    /// 死会话上敲键/切入 = 重连触发器（用户在场的明示）。在途不重孵
    /// （重孵会丢在途会话的输入缓存通道）
    fn kick_reconnect(&mut self) {
        let Some(name) = self
            .router_handle()
            .map(|r| r.lock().unwrap().active_name())
        else {
            return;
        };
        let h = self.health(name);
        if h.dead && !h.connecting {
            self.respawn_session(name);
        }
    }

    /// 断线重连（2026-08-21 实拍：WS 退后台被掐 → 会话线程死 → 僵尸通道
    /// 静默吞输入）：给死会话 spawn 新实例，router 换心脏（出向）+ 泵同名
    /// 登记换入向通道。服务器侧 PTY 随 WS 断即杀（kfmv4 ws-server killAll），
    /// 重连必然是新 shell——横幅明示，旧现场引导 tmux attach。本地 PTY
    /// 死亡（shell exit）同路重孵
    fn respawn_session(&mut self, name: &'static str) {
        let handle = match name {
            "local" => self
                .base
                .as_ref()
                .and_then(|b| b.ctx().get::<crate::local_pty::LocalPtyFactory>().ok())
                .map(|f| f.spawn(&f.default_config())),
            _ => self
                .base
                .as_ref()
                .and_then(|b| b.ctx().get::<dyn TermFactory>().ok())
                .map(|f| f.spawn(&f.default_config())),
        };
        let Some(h) = handle else {
            crate::report::report_sync("term", &format!("重连失败: {name} 工厂取回不到"));
            return;
        };
        {
            let health = self.health_mut(name);
            health.retried = true;
            health.connecting = true;
        }
        if self
            .router_handle()
            .is_some_and(|r| r.lock().unwrap().active_name() == name)
        {
            if let Some(r) = self.router_handle() {
                r.lock().unwrap().replace_active(h.outbound);
            }
            // 泵换心脏:同名 register 顶掉旧通道、清该名 replay(遗物不喂)
            crate::gate::pump_register(name, h.events);
            // 新会话 Input 缓存到 Opened（conn pending_input）——输出面先解开
            self.session_over = false;
            let (cols, rows) = self.last_grid;
            if let Some(r) = self.router_handle() {
                r.lock().unwrap().send(TermCmd::Resize { cols, rows });
            }
            if let Some(t) = self.term_handle() {
                let banner = format!(
                    "\r\n\x1b[36m[kfm-na: {name} 会话断线，已重连 = 新 shell（旧现场 tmux attach 接回）]\x1b[0m\r\n"
                );
                t.lock().unwrap().feed(banner.as_bytes());
            }
        } else {
            if let Some(r) = self.router_handle()
                && let Err(e) = r.lock().unwrap().replace_standby(h.outbound)
            {
                crate::report::report_sync("term", &format!("待机换心脏失败: {e}"));
            }
            crate::gate::pump_register(name, h.events);
        }
        crate::report::report("term", &format!("会话重连: {name} 重孵"));
        self.dirty = true;
    }

    /// 抽干会话事件（about_to_wait 每圈调）：pump 一轮——活跃方 Output
    /// 直接喂共享终端（值守线程 300ms 也在 pump，挂起态网格照新，
    /// 2026-08-24 数据面分家）；待机 Output 泵自存 replay；控制事件
    /// 出队记健康账（死讯即时登记——不抽的话压到切换才爆，重连晚一整拍；
    /// 2026-08-21 实拍 WS 退后台被掐的坑）
    fn drain_terminal_events(&mut self) {
        let active = self
            .router_handle()
            .map_or("", |r| r.lock().unwrap().active_name());
        // 终端还没建好就不 pump:Output 堆 mpsc 不丢(同旧制),控制事件
        // 等得起(首轮 about_to_wait 前终端必就位——init_terminal 先跑)
        if let Some(t) = self.term_handle()
            && crate::gate::pump_once(active, &mut |b| t.lock().unwrap().feed(b))
        {
            self.dirty = true;
        }
        for (name, ev) in crate::gate::pump_take_control() {
            self.on_session_event(name, ev, name == active);
        }
    }

    /// 单控制事件分派（Output 不经过此——泵已直接喂终端）：
    /// name = 来源会话名（泵按名带进），is_active = 是否当前可见方
    fn on_session_event(&mut self, name: &'static str, ev: SessionEvent, is_active: bool) {
        match ev {
            SessionEvent::Opened { session_id } => {
                {
                    let h = self.health_mut(name);
                    h.dead = false;
                    h.retried = false;
                    h.connecting = false;
                }
                if is_active {
                    self.session_over = false; // 重连复活：输出面解开
                }
                crate::report::report(
                    "term",
                    &format!("会话 opened: {session_id} +{}ms", boot_ms()),
                );
            }
            SessionEvent::Output { .. } => {
                crate::report::report_sync("term", "Output 窜进控制队列——泵分派 bug");
            }
            SessionEvent::Exited { code } => {
                self.on_slot_dead(name, is_active, &format!("exited: code={code}"));
            }
            SessionEvent::Failed { message } => {
                self.on_slot_dead(name, is_active, &format!("failed: {message}"));
            }
        }
    }

    /// 会话死亡登记：钉健康牌;活跃方死亡且本剧集未自动重连过 → 立即重孵
    /// 一次（用户在盯着，网多半是好的）;待机方死亡只记账不吵（切换那一刻
    /// 再重连——断网期给待机自动重连是烧钱风暴）
    fn on_slot_dead(&mut self, name: &'static str, is_active: bool, why: &str) {
        crate::gate::note_session_death(); // 会话死亡计数(资源画像)
        // 异步 report:此处在主线程抽干路径上,sync 直报会在断线瞬间
        // 冻 UI(2026-08-21 同步探针堵主线程同案);进程没死,不需要 sync
        crate::report::report(
            "term",
            &format!(
                "会话 {why}: {name}{}",
                if is_active {
                    "（活跃）"
                } else {
                    "（待机）"
                }
            ),
        );
        if is_active {
            self.session_over = true;
        }
        {
            let h = self.health_mut(name);
            h.dead = true;
            h.connecting = false;
        }
        if is_active && !self.health(name).retried {
            self.respawn_session(name);
        }
    }

    /// 排干 Java 皮（KfmInputConnection/快捷键行）经 JNI 注入的输入——
    /// 中文落字从这里进终端（NativeActivity 无 InputConnection 的补丁，
    /// 链路见 ime_queue.rs 文件头）。键码在排干侧按当下光标模式翻序列
    /// （模式位只有这里的 Term 知道，keymap.rs 吃 app_cursor 参数）
    fn drain_ime_inject(&mut self) {
        let items = crate::ime_queue::global().drain();
        if items.is_empty() {
            return;
        }
        // 死会话上敲键 = 重连触发器（不 return：重连后的新会话会缓存输入
        // 到 Opened；没重连上 = 僵尸通道吞掉，无害）
        if self.session_over {
            self.kick_reconnect();
        }
        static FIRST_INJECT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !FIRST_INJECT.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::report::report("ime", "首个 JNI IME 文字注入");
        }
        // 输入栏聚焦分流（期 0 组件三，§五 焦点二态）：键盘按键全归栏，
        // 不下终端——Enter=发送、退格删字、Esc 失焦、文本追加，其余特殊键
        // v1 不管（方向键/Tab 等）
        if self.input_bar.as_ref().is_some_and(|b| b.is_focused()) {
            let mut want_submit = false;
            if let Some(bar) = &self.input_bar {
                for item in items {
                    match item {
                        crate::ime_queue::Inject::Text(s) => {
                            crate::report::report("ime-input", &format!("commit: {s:?}"));
                            bar.insert_text(&s)
                        }
                        crate::ime_queue::Inject::Key(66) => want_submit = true, // KC_ENTER
                        crate::ime_queue::Inject::Key(67) => {
                            crate::report::report("ime-input", "backspace");
                            bar.backspace()
                        }
                        crate::ime_queue::Inject::Key(111) => bar.unfocus(), // KC_ESC
                        crate::ime_queue::Inject::Composing(s) => {
                            crate::report::report("ime-input", &format!("composing: {s:?}"));
                            bar.set_composing(&s)
                        }
                        crate::ime_queue::Inject::ComposingEnd => {
                            crate::report::report("ime-input", "composing-end");
                            bar.finish_composing()
                        }
                        crate::ime_queue::Inject::CommitEmpty => {
                            // BAR-054：空 commit = IME 契约删选区（剪切删除半真身）
                            let deleted = bar.delete_selection();
                            crate::report::report(
                                "ime-input",
                                &format!("commit-empty: 删选区={deleted}"),
                            );
                        }
                        crate::ime_queue::Inject::ContextMenuAction(action) => {
                            crate::report::report("ime-input", &format!("context-menu: {action}"));
                            match action.as_str() {
                                "selectAll" => bar.select_all(),
                                "copy" => {
                                    if let Some(text) = bar.selected_text() {
                                        crate::report::report(
                                            "ime-input",
                                            &format!("copy: {} chars", text.chars().count()),
                                        );
                                        if let Some(app) = &self.android_app {
                                            crate::clipboard::copy_and_toast(app, &text);
                                        }
                                    }
                                }
                                "cut" => {
                                    if let Some(text) = bar.selected_text() {
                                        crate::report::report(
                                            "ime-input",
                                            &format!("cut: {} chars", text.chars().count()),
                                        );
                                        if let Some(app) = &self.android_app {
                                            crate::clipboard::copy_and_toast(app, &text);
                                        }
                                        bar.delete_selection();
                                    }
                                }
                                "paste" => {
                                    // BAR-054 探针：粘贴全链路末环——系统剪贴板
                                    // 里此刻到底有什么（IME 的剪切复制落没落进来）
                                    let clip = self
                                        .android_app
                                        .as_ref()
                                        .and_then(crate::clipboard::get_clipboard_text);
                                    match clip {
                                        Some(text) => {
                                            crate::report::report(
                                                "ime-input",
                                                &format!(
                                                    "paste: {} chars（系统剪贴板命中）",
                                                    text.chars().count()
                                                ),
                                            );
                                            bar.insert_or_replace(&text);
                                        }
                                        None => {
                                            crate::report::report(
                                                "ime-input",
                                                "paste: 系统剪贴板空/读不到",
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                if want_submit {
                    let sent = bar.submit();
                    crate::report::report("ai", &format!("输入栏 Enter 发送: {sent:?}"));
                }
            }
            self.dirty = true;
            return;
        }
        let app_cursor = self
            .term_handle()
            .is_some_and(|t| t.lock().unwrap().app_cursor_mode());
        // 先落成字节串列表（借 self 算 app_cursor/记诊断），再逐条下发——
        // 下发段要 &mut self（Ctrl-] 会话切换），与 outbound 借用拆开
        let mut pending: Vec<String> = Vec::with_capacity(items.len());
        for item in items {
            let bytes = match item {
                crate::ime_queue::Inject::Text(s) => Some(s),
                crate::ime_queue::Inject::Key(code) => {
                    let seq = crate::keymap::key_seq(code, app_cursor);
                    // BAR-018 诊断：快捷键行的键到底发了什么序列
                    if let Some(seq) = seq {
                        let esc: String = seq.chars().flat_map(|c| c.escape_default()).collect();
                        crate::report::report(
                            "ime",
                            &format!("落键 {code} → {esc}（app_cursor={app_cursor}）"),
                        );
                    }
                    seq.map(str::to_string)
                }
                crate::ime_queue::Inject::Composing(_) | crate::ime_queue::Inject::ComposingEnd => {
                    None // 终端不画组合态(BAR-012 沿革);消费掉防空转
                }
                crate::ime_queue::Inject::CommitEmpty => {
                    // 终端无选区语义，空 commit 消费掉（上报留诊断）
                    crate::report::report("ime-input", "term commit-empty swallowed");
                    None
                }
                crate::ime_queue::Inject::ContextMenuAction(action) => {
                    // 终端无选择/剪贴板语义，消费掉防空转（上报保留诊断）
                    crate::report::report(
                        "ime-input",
                        &format!("term context-menu swallowed: {action}"),
                    );
                    None
                }
            };
            if let Some(bytes) = bytes {
                pending.push(bytes);
            }
        }
        let mut sent = false;
        for bytes in pending {
            if bytes.is_empty() {
                continue;
            }
            // L1 会话切换闸：Ctrl-]（keymap 把 Ctrl+] 落成 \x1d）不发对端，
            // 活跃/待机槽互换（telnet 转义符惯例）
            if bytes == "\u{1d}" {
                self.switch_session();
                continue;
            }
            if let Some(r) = self.router_handle() {
                r.lock().unwrap().send(TermCmd::Input(bytes));
                sent = true;
            }
        }
        if sent {
            // IME 落字 = 用户输入：滚回底部贴最新输出
            if let Some(t) = self.term_handle() {
                t.lock().unwrap().scroll_to_bottom();
            }
        }
    }

    /// 键盘事件 → 终端输入字节（尖刺极简映射，IME 见文件头留白）
    fn handle_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        // 死会话上敲键 = 重连触发器（同 drain_ime_inject 口径）
        if self.session_over {
            self.kick_reconnect();
        }
        // 输入栏聚焦分流（物理键盘与 IME 同尺）
        if self.input_bar.as_ref().is_some_and(|b| b.is_focused()) {
            let mut want_submit = false;
            if let Some(bar) = &self.input_bar {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => want_submit = true,
                    Key::Named(NamedKey::Backspace) => bar.backspace(),
                    Key::Named(NamedKey::Escape) => bar.unfocus(),
                    _ => {
                        if let Some(t) = &event.text {
                            bar.insert_text(t);
                        }
                    }
                }
                if want_submit {
                    bar.submit();
                }
            }
            self.dirty = true;
            return;
        }
        let bytes: Option<String> = match &event.logical_key {
            Key::Named(NamedKey::Enter) => Some("\r".into()),
            Key::Named(NamedKey::Backspace) => Some("\x7f".into()),
            Key::Named(NamedKey::Tab) => Some("\t".into()),
            Key::Named(NamedKey::Escape) => Some("\x1b".into()),
            _ => event.text.as_ref().map(|t| t.to_string()),
        };
        if let (Some(bytes), Some(r)) = (bytes, self.router_handle())
            && !bytes.is_empty()
        {
            r.lock().unwrap().send(TermCmd::Input(bytes));
            // 打字了就是要看现在——滚回底部贴最新输出
            if let Some(t) = self.term_handle() {
                t.lock().unwrap().scroll_to_bottom();
            }
        }
    }

    /// 光栅化一帧的内容（终端网格 + 快捷键行 + 光球/AI 页 + 放大镜 +
    /// tofu 上报）进任意像素缓冲。关联函数按字段传参，避开 buf 借用
    /// self.gfx 时动不了 self 的问题。后台离屏倒帧不走这里——值守线程
    /// (screendump) 只画终端网格本体，快捷键行/光球/放大镜是 UI 装帧，
    /// 不在后台视野里
    #[allow(clippy::too_many_arguments)]
    fn rasterize(
        term: Option<&mut Box<dyn TermEmu>>,
        mods: u8,
        magnifier_at: Option<(f64, f64)>,
        ime_bottom_px: u32,
        ai_snap: Option<crate::ai_presence::PresenceSnap>,
        chat_msgs: &[(bool, String, String)],
        chat_scroll: u32,
        bar_snap: Option<&crate::input_bar::BarSnap>,
        caret_on: bool,
        buf: &mut [u32],
        w: u32,
        h: u32,
        panel_off: i32,
        panel_scratch: &mut Vec<u32>,
    ) -> Option<(u32, u32)> {
        let mut ai_layout = None; // AI 页渲染过 = Some(总行数, 一屏行数)
        if let Some(term) = term {
            // 当前栏带高（与 render_inputbar 同源实测折行——不读 poll 写回
            // 的 lines，前后台都不得两张皮）——keybar inset 与渲染同尺
            // （眼手同尺，2026-08-31 排障实锤的延伸）
            let bar_h = bar_snap.map_or(crate::input_bar::HEIGHT_PX, |bs| {
                crate::input_bar::height_for_lines(term.bar_text_lines(&bs.text, w))
            });
            // AI 面板三分支（采样缝 2026-09-04）：panel_off 是缝采样值——
            // 无 ui-fx 占槽时恒等于目标值（0 或 -h），退化为原硬切两分支；
            // 中间值 = 弹簧过渡帧：终端在下，面板移位压上
            if panel_off <= -(h as i32) {
                // 面板完全屏外（终端页稳态）：画终端网格 + 快捷键行
                // （快捷键行 = BAR-017 Rust 自绘覆盖层；inset 必须叠输入栏
                // 当前带高——栏带压在行下沿，漏叠 = 眼手错位，2026-08-31
                // 排障实锤，触摸几何早就是叠后的）
                term.render_into(buf, w, h);
                term.render_keybar(buf, w, h, ime_bottom_px + bar_h, mods);
            } else if panel_off == 0 {
                // 面板靠泊（AI 全屏页稳态，期 0③ 真对话页）：不画终端
                // 网格与快捷键行，深紫暗底 + 消息行视口（期 0④ 滚动）
                ai_layout = Some(term.render_ai_page(buf, w, h, chat_msgs, chat_scroll));
            } else {
                // 过渡帧：终端 + 快捷键行在下（照画——快捷键行层级低于
                // 面板，被落下来的面板盖住是自然结果，用户 2026-09-04
                // 拍板；BAR-063：过渡帧不画它 = 动画两端硬切 = 闪烁），
                // AI 面板整页离屏渲染后按偏移压盖——与直接渲染像素等价
                term.render_into(buf, w, h);
                term.render_keybar(buf, w, h, ime_bottom_px + bar_h, mods);
                panel_scratch.clear();
                panel_scratch.resize((w as usize) * (h as usize), 0);
                ai_layout = Some(term.render_ai_page(panel_scratch, w, h, chat_msgs, chat_scroll));
                crate::termview::blit_panel_shifted(buf, panel_scratch, w, h, panel_off);
            }
            // 全局输入栏（常驻 chrome：任何会话下都在，§二——AI 页也画）。
            // 压底紧贴键盘（栏带 = 屏底 - inset - 栏高）；sending 图标态
            // 跟 AI 运行态硬切（kfmv4 .ai-send-btn.sending ▶ ↔ ⏸）；
            // caret_on = 光标闪烁相位（draw_frame 按 CARET_BLINK_MS 算好传入）
            if let Some(bs) = bar_snap {
                let sending = ai_snap.is_some_and(|s| s.ai_running);
                term.render_inputbar(buf, w, h, ime_bottom_px, bs, sending, caret_on);
            }
            // 光球（常驻 chrome：画在终端网格/AI 页之后，两页都在）。
            // 四态增益硬切读 ai_presence::orb_gain（闲/运行/pressed/AI页）
            if let Some(s) = ai_snap {
                let (gain, halo_gain) =
                    crate::ai_presence::orb_gain(s.ai_running, s.pressed, s.page);
                term.render_orb(buf, w, h, s.x, s.y, gain, halo_gain);
            }
            // 选区边界拖动中的放大镜浮窗（画在所有内容之上——
            // 帧缓冲源区在主渲染里已就位，这里纯位图放大）
            if let Some((mx, my)) = magnifier_at {
                term.render_magnifier(buf, w, h, mx, my);
            }
            // tofu 目击上报：双字体都缺的字符（方框的真身），新字才报
            let tofu = term.take_tofu_chars();
            if !tofu.is_empty() {
                let list = tofu
                    .iter()
                    .map(|c| format!("U+{:04X}({c})", *c as u32))
                    .collect::<Vec<_>>()
                    .join(" ");
                crate::report::report("term", &format!("tofu 目击: {list}"));
            }
        } else {
            buf.fill(KFM_PURPLE); // 字体全灭的降级画面：紫屏 + 已有上报
            return None;
        }
        ai_layout
    }

    /// 装配路由：Arc 化 + 登记闸门注册表（keys-in 注入的唯一入口）
    fn install_router(&mut self, router: crate::session_router::SessionRouter) {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(router));
        crate::gate::register_gate_router(&shared);
        self.router = Some(shared);
    }

    /// 取路由句柄（owned Arc，借用即还——同 term_handle 套路）
    fn router_handle(&self) -> Option<crate::gate::SharedRouter> {
        self.router.clone()
    }

    /// 取终端句柄（Arc 克隆）：UI 线程与后台倒帧值守线程共用一把锁。
    /// 返回 owned Arc 而非 guard——guard 会拖着 &self 借用,挡住块内
    /// 写 self.dirty 等其他字段;句柄落地后 lock 出的 guard 只借本地
    fn term_handle(&self) -> Option<crate::gate::SharedTerm> {
        self.term.clone()
    }

    /// 渲染一帧：终端模式画网格，非终端模式清紫屏
    fn draw_frame(&mut self) {
        let t0 = std::time::Instant::now(); // 帧耗时画像(自观测第三块)
        // 先拿终端句柄(owned Arc,借用即还),再借 gfx——顺序反了 E0502
        let th = self.term_handle();
        let Some(g) = &mut self.gfx else { return };
        let mut buf = g.surface.buffer_mut().expect("取帧缓冲失败");
        let (w, h) = (buf.width().get(), buf.height().get());
        if TERMINAL_MODE {
            crate::gate::note_frame_size(w, h); // 给后台倒帧值守记账
            let mods = self.modifiers.as_ref().map_or(0, |m| m.peek());
            // 光标闪烁相位（聚焦时每半周期翻转要重画——置脏在 poll_input_bar）
            let caret_on = (crate::report::boot_ms() as u64 / crate::input_bar::CARET_BLINK_MS)
                .is_multiple_of(2);
            let chat_msgs = self.ai_chat.as_ref().map(|c| c.snap()).unwrap_or_default();
            let mut tg = th.as_ref().map(|a| a.lock().unwrap());
            // AI 面板 Y 偏移过缝（ui-base §三）：目标值 = AI 页 0 靠泊 /
            // 终端页 -屏高屏外（目标值语义在基础层，缝只许插值不许改）。
            // 无 ui-fx 占槽 = 直通目标值（硬切，与改前像素等价）
            let ai_page = self
                .last_ai_snap
                .is_some_and(|s| s.page == crate::ai_presence::Page::AiFullscreen);
            let panel_target = if ai_page { 0.0 } else { -(h as f32) };
            let panel_off = crate::ui::seam::sample_ai_panel_offset_y(
                panel_target,
                crate::report::boot_ms() as u64,
            ) as i32;
            let chat_scroll = self.ai_chat.as_ref().map_or(0, |c| c.scroll_offset());
            let ai_layout = Self::rasterize(
                tg.as_deref_mut(),
                mods,
                self.magnifier_at,
                self.ime_bottom_px,
                self.last_ai_snap,
                &chat_msgs,
                chat_scroll,
                self.last_bar_snap.as_ref(),
                caret_on,
                &mut buf,
                w,
                h,
                panel_off,
                &mut self.panel_scratch,
            );
            // 布局写回视口状态机（眼手同尺：手势钳制与渲染同一份布局）
            if let (Some(chat), Some((total, fit))) = (&self.ai_chat, ai_layout) {
                chat.scroll_sync_layout(total, fit);
            }
        } else {
            buf.fill(KFM_PURPLE);
            // 首帧呈现里程碑：紫屏真亮了才算雷 1 排除
            static FIRST_PRESENT: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !FIRST_PRESENT.swap(true, std::sync::atomic::Ordering::Relaxed) {
                crate::report::report("boot", "首帧 present 完成——紫屏应已亮");
            }
        }
        // 画面回传由值守线程统一消费(gate::spawn_gate_watcher)——
        // 挂起态事件循环叫不醒,前台顺帧消费那套在后台是死路,单一消费者
        buf.present().expect("帧呈现失败");
        crate::gate::note_draw(t0.elapsed()); // 含 present 的全帧耗时
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        crate::gate::note_foreground(true); // 看门狗出假(BAR-036)
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("KFM-NA");
        let window = Arc::new(el.create_window(attrs).expect("创建窗口失败"));
        let gfx = Self::init_gfx(&window);
        self.gfx = Some(gfx);
        self.window = Some(window.clone());
        if TERMINAL_MODE {
            // BAR-004 后台往返：Term/会话还活着就只重建窗口表面，别重开会话
            // （scrollback 和 shell 状态保住）；会话死了才重开
            if self.term.is_none() || self.session_over {
                self.init_terminal(&window);
            } else {
                crate::report::report("boot", "后台往返：会话还在，只重建表面");
            }
            // 字体全灭走紫屏降级也要有首帧：dirty 兜底置位
            self.dirty = true;
        }
        log::info!("KFM-NA 壳启动完成");
        // 首帧快路(2026-08-21 落地):表面建成+终端就绪即主动画第一帧,
        // 不等系统发首笔 RedrawRequested——用户实测「秒进」的一刀
        if TERMINAL_MODE {
            self.draw_frame();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                crate::report::report("death", "CloseRequested——窗口被要求关闭");
                el.exit();
            }
            WindowEvent::Resized(sz) => {
                if let Some(g) = &mut self.gfx
                    && let (Some(w), Some(h)) =
                        (NonZeroU32::new(sz.width), NonZeroU32::new(sz.height))
                {
                    g.surface.resize(w, h).expect("surface resize 失败");
                }
                if TERMINAL_MODE {
                    self.apply_window_size(sz.width, sz.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if TERMINAL_MODE {
                    self.handle_key(&event);
                }
            }
            // 触摸：拖动 = 滚 scrollback（A 档手势状态机 src/scroll.rs），
            // 没过阈值的点按才唤软键盘。winit 的 set_ime_allowed 走
            // SHOW_IMPLICIT，用户收过键盘后 IMM 拒弹（BAR-012）——JNI
            // SHOW_FORCED 强弹兜底
            WindowEvent::Touch(touch) => {
                if TERMINAL_MODE {
                    self.handle_touch(touch.id, touch.location.x, touch.location.y, touch.phase);
                }
            }
            // IME 事件链：Commit = 上屏文本（中文候选词落字也走这），直接注入终端
            WindowEvent::Ime(ime) => {
                if TERMINAL_MODE {
                    match ime {
                        // Ime::Enabled/Disabled 只留痕——本机从未触发（BAR-006），
                        // 键盘避让由 JNI 轮询驱动（poll_ime_inset）
                        Ime::Enabled => crate::report::report("ime", "IME Enabled"),
                        Ime::Disabled => crate::report::report("ime", "IME Disabled"),
                        // Preedit（拼音候选中）尖刺期不上屏
                        Ime::Preedit(_, _) => {}
                        Ime::Commit(text) => {
                            // 输入栏聚焦分流（winit IME 链与 JNI 链同尺）
                            if self.input_bar.as_ref().is_some_and(|b| b.is_focused()) {
                                if let Some(bar) = &self.input_bar {
                                    bar.insert_text(&text);
                                }
                                self.dirty = true;
                                return;
                            }
                            // 死会话上落字 = 重连触发器（同 drain_ime_inject 口径）
                            if self.session_over {
                                self.kick_reconnect();
                            }
                            if let Some(r) = self.router_handle() {
                                r.lock().unwrap().send(TermCmd::Input(text));
                                // IME 落字 = 用户输入：滚回底部贴最新输出
                                if let Some(t) = self.term_handle() {
                                    t.lock().unwrap().scroll_to_bottom();
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                FIRST_REDRAW_SEEN.store(true, std::sync::atomic::Ordering::Relaxed);
                if TERMINAL_MODE && !self.dirty {
                    return; // 空圈不重绘(降频泵后 redraw 已是条件触发,双保险)
                }
                self.dirty = false;
                self.draw_frame();
            }
            _ => {}
        }
    }
    fn suspended(&mut self, _el: &ActiveEventLoop) {
        crate::gate::note_foreground(false); // 看门狗休假(BAR-036):挂起停跳合法
        crate::report::report("death", "suspended——Activity 被挂起（退后台/被销毁前奏）");
        // BAR-004：Android 退后台即销毁 native 表面，softbuffer 握着的
        // ANativeWindow 变成死柄——不弃窗则回前台对着死表面画，页面消失
        // （12:10 实拍）。弃窗弃表面，resumed 走重建；Term/会话保留
        self.gfx = None;
        self.window = None;
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        crate::report::report_sync("death", "exiting——事件循环即将退出");
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        crate::gate::note_loop_beat(); // 看门狗心跳:每圈盖戳(降频后 ≥250/s,3s 阈值不变)
        if TERMINAL_MODE {
            self.drain_terminal_events();
            self.drain_ime_inject();
            self.drain_touch_in(); // 通道八:闸门触摸注入(与真手指同入口)
            if crate::gate::switch_take() {
                self.switch_session(); // 通道九:switch-req 遥控切换(与 Ctrl-] 同入口)
            }
            self.poll_ime_inset();
            // 长按计时(从 RedrawRequested 挪来,2026-08-26 降频泵:重绘
            // 现在是条件触发,空圈不 redraw;每圈 4ms 查一次 500ms 阈值照准,
            // 触发即置 dirty → 本圈末尾条件重绘接住)
            self.check_long_press();
            self.check_orb_long_press(); // 光球长按 → fake_run(debug 钩子)
            self.check_inputbar_long_press(); // 输入栏长按 → 选择模式(BAR-046)
            self.poll_ai_presence(); // AI 外显快照比对(注入/到期也要画帧)
            self.poll_input_bar(); // 输入栏快照比对(注入/分流也要画帧)
            // 采样缝动画帧时钟(ui-base §四 按需启停):缝上有活跃动画
            // 且距上帧 ≥16ms 才置脏——无动画零额外帧,有动画 ≤60fps
            if crate::ui::fx_spring::panel_frame_due(crate::report::boot_ms() as u64) {
                self.dirty = true;
            }
            // blackout 期补画(冗余兜底,2026-08-22 探针拆除案保留):
            // 首笔 RedrawRequested 到达前的脏帧由唤醒锤锤醒的本方法直画;
            // 首笔 Redraw 到达后唤醒锤收锤,此路自动关闭归回正道
            if !FIRST_REDRAW_SEEN.load(std::sync::atomic::Ordering::Relaxed)
                && self.dirty
                && self.gfx.is_some()
            {
                self.dirty = false;
                self.draw_frame();
            }
        }
        // 事件循环心跳（10s 节流）：它在跳 = 循环活着，
        // 它停 = 循环卡死在某个 handler 里（BAR-012③ 诊断分界线）
        let beat_due = match self.last_loop_beat {
            Some(t) => t.elapsed() >= std::time::Duration::from_secs(10),
            None => true,
        };
        if beat_due {
            self.last_loop_beat = Some(std::time::Instant::now());
            // BAR-012③ 三轮：捎带 JNI 入口计数——commit=入口/入队，全 0 即
            // Java→JNI 绑定全灭（符号在但被 ART 拒），>0 而 pushed=0 死在转换
            let (ce, cp, sk, il) = crate::ime_bridge::jni_counters();
            crate::report::report(
                "loop",
                &format!("事件循环心跳 jni(commit={ce}/{cp} key={sk} log={il})"),
            );
        }
        // 降频泵(2026-08-26,挂单①治理):Poll 全速空转实测 ~57k 圈/s,
        // 白烧 CPU/电。双闸——①有脏才请求重绘(空圈不 redraw);②节拍改
        // WaitUntil 4ms:击键/IME/会话事件到达照常即时唤醒不受限,纯轮询
        // 部分(会话输出抽干)最坏延迟 4ms,人不可感。空转降两个量级。
        if self.dirty
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
        el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(4),
        ));
    }
}

/// NativeActivity 入口（android-activity 约定符号名）
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    // 飞鸽传书：先起后台冲洗线程（必须在任何上报之前——入队要有人接，
    // 否则第一格静默丢，06:42 实拍已踩），再挂 panic 钩子。
    // 第一格异步入队（BAR-022 归因实锤：此处曾用 report_sync 同步直报，
    // connect 2s+读应答 3s+重试 3 次的同步 HTTP 卡在启动关键路径上，
    // 冷隧道时单这一条就堵 3.3s——「启动慢的窃贼是日志通道自己」。
    // 冲洗线程毫秒级即发出这行，「进门即死零日志」的防护仍在）。
    // 能收到这行 = 死在 android_main 内部；收不到 = 死在更前（加载/manifest）。
    // 构建戳（BAR-013）：设备跑的 .so 是哪个构建一读便知——dex/so 错配
    // 实拍案里「探针全体沉默」曾让我们绕了一整圈才想到 .so 是旧的
    crate::report::start_flusher();
    crate::report::set_boot_t0();
    crate::report::report(
        "boot",
        &format!(
            "android_main 进入 (构建 {} · vc{})",
            option_env!("KFM_NA_BUILD").unwrap_or("dev"),
            option_env!("KFM_NA_VC").unwrap_or("dev")
        ),
    );
    // panic 钩子(2026-08-25 升级):旧版仅 report 异步直报——进程死了
    // 冲洗队列同归于尽,收不到;且 logcat 链被顶掉。新版落盘闸门目录
    // panic.log 为主、report 为辅、链默认钩子,线程 panic 也收
    crate::gate::install_panic_hook(crate::gate::DUMP_DIR);
    // 信号级坠机记录(自观测第四块①):panic 钩子管 Rust 层,SIGSEGV 等
    // native 崩溃绕过它——last-gasp handler 写一行 SIGNAL 后进 panic.log
    // 再 re-raise 交还系统。SIGURG 是装机判卷探针(写行后继续活)
    crate::crash::install_signal_hook(crate::gate::DUMP_DIR);
    // BAR-037 重跑防御：必须卡在任何线程 spawn 与 EventLoop::build 之前。
    // 旧进程被 ROM 冻结保住（exit(0) 没跑完），循环已毁；同进程二进
    // android_main 若往下走 = 心跳/值守线程重复起 + EventLoop::new
    // panic(RecreationAttempt,panic.log 2026-08-26 已捕获)。遗言用
    // report_sync 同步直报——紧随的 exit(0) 不会给它异步入队的机会。
    if ANDROID_MAIN_RAN.swap(true, std::sync::atomic::Ordering::SeqCst) {
        crate::report::report_sync(
            "death",
            &format!(
                "android_main 重跑让位 (构建 {} · vc{})",
                option_env!("KFM_NA_BUILD").unwrap_or("dev"),
                option_env!("KFM_NA_VC").unwrap_or("dev")
            ),
        );
        // BAR-038(2026-08-26 实拍):此处直接 exit(0) 会在主线程跑 TLS
        // 析构 → jni 0.22 sys_detach_current_thread 断言 guard_level==0
        // 炸(left:2——冻结现场里上一次 android_main 的附着从没归还,
        // 二次进门又叠一层)。换无线程史的新线程退:它的 TLS 干净,
        // 断言无从触发;主线程 join 在原地等死(进程先没,join 不返回)。
        // restart-req 路径天然免疫——它从值守线程退(装机实证干净)。
        std::thread::spawn(|| std::process::exit(0)).join().ok();
    }
    // ws 冒烟（尖刺切片 3 对照组）：连服务器 terminal-pty 跑 echo 闭环，
    // 判卷 = field-reports.log 的 [ws] 四格。TERMINAL_MODE=true 时让位给
    // 常驻会话（resumed 里 spawn），冒烟路径保留作回退开关
    if !TERMINAL_MODE {
        crate::conn::spawn_smoke("ws://127.0.0.1:8021/ws", "echo KFM-NA-WS-OK");
    }
    // 心跳：进程存活的客观判决——心跳停 = 进程真死（精确到秒）；
    // 心跳在跳但用户看到「闪退」= Activity 被系统杀、进程活着（病根完全不同）
    // 3s 间隔 + 独立同步直报：不给冲洗队列灌洪水，也不受队首阻塞牵连
    std::thread::spawn(|| {
        let mut n = 0u32;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            n += 1;
            crate::report::report_sync("alive", &format!("心跳 {}", n));
        }
    });
    log::info!("KFM-NA android_main 进入");
    let event_loop = EventLoop::builder()
        .with_android_app(app.clone())
        .build()
        .expect("创建事件循环失败");
    let mut app_handler = App {
        android_app: Some(app),
        ..Default::default()
    };
    // blackout 唤醒锤(冗余兜底,2026-08-22 探针拆除案保留):proxy user
    // event 50ms 一锤,把循环锤醒跑 about_to_wait(抽事件/补画脏帧);
    // 首笔 Redraw 到达即收锤。注意:proxy 只在循环跑着时叫得醒,
    // Activity 挂起态叫不醒(2026-08-24 实拍)——后台倒帧不靠它,
    // 走 screendump 值守线程
    let wake_proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        while !FIRST_REDRAW_SEEN.load(std::sync::atomic::Ordering::Relaxed) {
            if wake_proxy.send_event(()).is_err() {
                break; // 循环已死,收锤
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });
    // 后台画面回传值守(2026-08-24 与用户定:截图不要求应用在前台)
    crate::gate::spawn_gate_watcher();
    let result = event_loop.run_app(&mut app_handler);
    // 同步直报：async 入队后立刻 exit(0) 会吃掉这行（此前历次「静默消失」
    // 的嫌疑——死亡现场被自己的 exit(0) 毁尸灭迹）
    crate::report::report_sync("death", &format!("run_app 返回: {:?}", result));
    // 事件循环一生只能建一次（winit RecreationAttempt）。NativeActivity 销毁后
    // 进程常被 ROM 保留，不自杀则下次点开 android_main 重跑必 panic
    // （2026-08-13 实拍「白退」次生病灶）。活动结束 = 进程跟着死，重来即全新。
    std::process::exit(0);
}

// BAR-046: 选择系统+复制粘贴菜单已在手机端 chain 验证通过 (2026-09-02)
