//! android_app.rs — Android 壳（B 档：平台胶水，冒烟钉防退化）
//!
//! 渲染路线（2026-09-04 修订）：**GLES present 优先，softbuffer 兜底**。
//! 原 2026-08-13 定案（纯 softbuffer）的背景：本机 GPU 驱动栈
//! （Mali-G720 Immortalis r44p1 + OriginOS）与 wgpu 双后端随机原生暴毙。
//! 期 0③ 尖刺判活（gpu-render.md §九）：病灶坐实在 wgpu-hal 而非驱动，
//! 裸 EGL/GLES 全链路通关——壳内 GLES 基建（期 1 第 1 层）据此落地，
//! init 失败自动回退 softbuffer（立项书红线：永久保留）。
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

/// 面板页手势跟踪（期 0④ AI 页起手，2026-09-10 泛化成面板栈顶§五B）：
/// 在顶面板（AI/配置）的手势仲裁——拖动滚行（仅 AI 页有内容可滚）+
/// 点按收键盘 + 水平抽屉滑（召唤/推回）
struct PanelTouch {
    start_x: f64,
    start_y: f64,
    last_y: f64,
    /// 行高余量累积（px）——跨 Moved 事件攒够一行才滚一行（像素级跟手）
    acc_px: f64,
    dragged: bool,
}

/// 配置卡标签栏手势（主题宪法 §四，2026-09-12）：按在标签行带上 =
/// 手势归标签栏（仲裁条款——横向滑动滚标签不触发面板拖拽/页面滑向）。
/// 拖过 slop = 横滚标签（pan 像素级跟手）；未超抬手 = 点按选池
struct TabTouch {
    start_x: f64,
    start_y: f64,
    last_x: f64,
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

/// tmux 执行种类（解析页插件在途账：Ok 后的善后按种类分流）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserExec {
    /// 列会话（结果填表）
    List,
    /// 新建会话（结果 = 新会话名 → attach 过去）
    New,
    /// 关闭会话（结果无视 → 刷新列表）
    Kill,
    /// 重排窗口尺寸（结果无视 → 刷新列表）
    Reflow,
}

/// 解析页卡区拖动分流（2026-09-20 视口化，用户拍板「卡弹小+上下能
/// 滑动」）：起手落会话框表带 = 表内滚动；其余卡区 = 页面滚动（整链
/// 随视口平移）。仲裁在拖过 slop 且垂直主导时一次定终身
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserDrag {
    /// 会话框表内滚动（>6 框的内部滚窗）
    Session,
    /// 页面级滚动（卡链整体随视口平移）
    Page,
}

/// 会话健康牌（断线重连 2026-08-21，按名字记账——槽位随切换翻面,
/// 死活跟名字走）：dead = Failed/Exited 钉死、Opened 复活;
/// connecting = 重连在途
/// （Opened/再死才清——在途再触发 = 重孵,在途会话的输入缓存通道被丢）。
/// 自动重孵的放行判据 = crate::session::auto_respawn_due 时间闸
///（2026-09-11 redroid 瞬死案后升级；原「每剧集一次」retried 语义
/// 被 Opened 清牌击穿，已退役）
#[derive(Clone, Copy, Debug, Default)]
struct SessHealth {
    dead: bool,
    connecting: bool,
}

/// 渲染后端（期 1 第 1 层）：GLES present 优先，init 失败回退 softbuffer
/// （GLES_FIRST 开关；立项书红线——softbuffer 永久保留做兜底与老设备保险）
enum Gfx {
    Soft {
        _context: SoftContext,
        surface: SoftSurface,
    },
    Gles(Box<crate::gles_present::GlesPresent>),
}

/// GLES 优先开关：true = init_gfx 先试 GLES present 后端（失败自动回退
/// softbuffer，照常可用）；false = 纯 softbuffer（对照组/排障开关）
const GLES_FIRST: bool = true;

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
    /// 上次自动重孵时刻（boot_ms 口径；None = 本进程还没自动重孵过）。
    /// 自动重孵的时间闸依据，见 crate::session::auto_respawn_due；
    /// 一切重孵（含手动）都在 respawn_session 里刷新本字段——
    /// 闸量的是「真实重孵密度」
    last_auto_respawn_ms: Option<u64>,
    /// 真实软键盘底部 inset（px，JNI 轮询得来，BAR-006）。0 = 未弹/未知。
    /// 快捷键行的让位是 Rust 常量（keybar::HEIGHT_PX），不进本字段。
    /// 本字段是「目标值」：终端 resize / pty 永远吃它（resize 抖动红线，
    /// 不过缝）；chrome（栏带渲染与触摸命中）吃过缝后的 chrome_inset_px
    ime_bottom_px: u32,
    /// chrome 跟随 inset（px，2026-09-04 键盘 inset 缝的采样值）：
    /// 输入栏/快捷键行渲染与触摸命中、AI 页视口下沿吃这份——眼手同尺。
    /// 无 ui-fx 占槽时 == ime_bottom_px（硬切基座）；有占槽 = 弹簧平滑
    chrome_inset_px: u32,
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
    /// 方向键长按连发状态（2026-09-19 用户拍板）：Started 命中方向键即
    /// 武装，主循环 check_bar_repeat 每圈 poll；抬手/Cancelled 收走
    bar_repeat: Option<crate::keybar::KeyRepeat>,
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
    /// 按在面板页上的手势（期 0④ 起手，2026-09-10 泛化面板栈顶§五B：
    /// 任一面板在顶时终端手势全家让路——不穿透）：AI 页拖动 = 对话页
    /// 滚行（追底状态机）；水平快滑 = 抽屉召唤/推回；未超 slop 抬手 =
    /// 点按（输入栏失焦 + 收键盘，不召唤终端输入法）
    panel_touch: Option<PanelTouch>,
    /// 面板跟手拖拽会话（2026-09-11 §五B 手势升级，panel_drag.rs）：
    /// Started 在两上下文（终端裸奔/面板在顶）创建旁观者；Moved 喂轨迹，
    /// 方向锁一锁即接管（召唤锁=立即入栈，渲染偏移旁路缝采样直读）；
    /// Ended 裁决完成/取消 + replay 踢收尾续播；Cancelled 强制取消
    panel_drag: Option<crate::ui::panel_drag::PanelDrag>,
    /// 按在设置钮上的手势（2026-09-12 配置池卡按钮入口，ui/gear.rs）：
    /// (指 id, 起点 x, 起点 y, 拖过 slop)。只在裸终端页可达——Started
    /// 分流里面板在顶先于它 return，互斥从路由自然推出（栈零新规则）。
    /// 点按抬手 = summon_panel(Config)；拖过 slop = 不触发
    gear_touch: Option<(u64, f64, f64, bool)>,
    /// 按在断线状态卡钮上的手势（A 断线治理，ui/down_card.rs）：
    /// (指 id, 起点 x, 起点 y, 武装命中, 拖过 slop)。只在 session_over
    /// 且裸终端页可达（卡在屏才命中）；抬手同钮 = 触发，拖过 = 不触发
    down_touch: Option<(u64, f64, f64, crate::ui::down_card::DownHit, bool)>,
    /// 配置卡标签栏状态（主题宪法 §四）：池名表 + 选中 + 横滚 + 光标
    /// 弹簧。共享句柄注册给 gate 值守倒帧（D9 同源——后台截图/倒帧
    /// 与前台帧同一份标签栏读数）
    tab_bar: Option<crate::ui::tab_bar::SharedTabBar>,
    /// 配置卡双池状态（主题宪法 §五）：上池内容高 + 可用区 → 动态高度
    /// 布局。共享句柄注册给 gate 值守倒帧（D9 同源，与标签栏同规）
    dual_pool: Option<crate::ui::dual_pool::SharedDualPool>,
    /// 配置页三层目录状态核（宪法 §五 目录语义，2026-09-13）：下池
    /// 子目录行表/聚焦 + 上池字段行/联动下拉。共享句柄注册给 gate
    /// 值守倒帧（D9 同源）；行表/字段由 rebuild_cfg_rows 重建
    cfg_page: Option<crate::ui::cfg_page::SharedCfgPage>,
    /// 按在配置页池区上的手势（2026-09-13 三层目录）：(起点 x, 起点 y,
    /// 拖过 slop)。只在配置页靠泊在顶且起点在池区时建——点按抬手 =
    /// 下池行聚焦/触发器开合；拖过 slop = 手势归面板页全家（不聚焦）
    cfg_pool_touch: Option<(f64, f64, bool)>,
    /// 解析页 tmux 插件状态核（2026-09-19 用户立项）：会话表/附着名/
    /// 命名态/确认态。共享句柄注册给 gate 值守倒帧（与 cfg_page 同规）
    parser_page: Option<crate::ui::parser_page::SharedParserPage>,
    /// 解析页卡区触摸槽：(起手 x, 起手 y, 拖动分流, 上次 y)——
    /// 拖过 slop 垂直主导才分流（2026-09-20 视口化）：起手落会话框表
    /// 带且表可滚 → Session 表内滚动；其余卡区且页面可滚 → Page 页面
    /// 滚动；都不可滚/横向主导 → 让回面板页全家
    parser_touch: Option<(f64, f64, Option<ParserDrag>, f64)>,
    /// tmux 执行在途（动作类 + 结果通道）：about_to_wait 排水——Ok 后
    /// List=填表 / New=attach 新会话 / Kill/Reflow=刷新列表
    parser_exec: Option<(
        ParserExec,
        std::sync::mpsc::Receiver<Result<String, String>>,
    )>,
    /// 远程连接配置缓存（启动时装配 conn_provider 那份的 clone）——
    /// tmux 执行通道的 ws url 与 attach 重开连接的命令来源
    remote_conn_cfg: Option<crate::conn::ConnConfig>,
    /// 本端当前附着的 tmux 会话名（启动命令提取/attach 后更新）
    remote_attached: Option<String>,
    /// 本地相附着名（两轴第 6 步②对称：本地 PTY 重孵 tmux attach 后
    /// 更新；本地会话死亡/手动重孵 = 裸 shell——default_config 无命令，
    /// 重孵点把本账勾销）
    local_attached: Option<String>,
    /// L3 内置 ssh 正连隧道快照（2026-09-19 用户拍板：通道收归 na 自持，
    /// 取代 Termux 外挂隧道）——Some = 看门狗已起，UI/报表只读这份
    tunnel_snap: Option<std::sync::Arc<std::sync::Mutex<crate::tunnel::TunnelSnap>>>,
    /// 解析页靠泊上一圈状态（BAR-115：靠泊上升沿 = 重开页 → 重列会话。
    /// Idle 一次性闸只补首查，Ready 后重开页不刷 = 服务器侧 tmux 会话
    /// 增删永远看不见——「开页自动刷」设计口径的实际破洞）
    parser_docked_prev: bool,
    /// 隧道快照上一圈代际戳+可用相（BAR-117：不可用→可用的上升沿踢
    /// 活跃死会话重孵——重孵链是死亡事件驱动的，末次重孵撞 TCP
    /// refused 被 5s 闸压住后链断，隧道 Up 必须回头踢壳层）
    last_tunnel_epoch: u64,
    last_tunnel_usable: bool,
    /// 跳框模态手势槽（宪法 §六 跳框条款，九修）：
    /// (起手x, 起手y, 已拖过slop)——模态开着时配置页手势全归它
    modal_touch: Option<(f64, f64, bool)>,
    /// 挂起态屏尺寸缓存（BAR-108）：退后台窗口即弃（BAR-004 suspended
    /// 置 None），触摸命中几何不能跟着瞎——apply_window_size 每次记账，
    /// window=None 时由 screen_px() 兜底回退
    last_win_px: (u32, u32),
    /// 按在标签行带上的手势（宪法 §四 仲裁条款：行上横向滑动不触发
    /// 面板拖拽/页面滑向）——配置页靠泊且起点在行带才建
    tab_touch: Option<TabTouch>,
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
    /// 图层烘焙判定（ui-base §八 渲染成本模型，2026-09-07）：三槽 sig
    /// 记账——动画帧（panel_off 变）不进任何槽的重烘焙
    layer_sigs: LayerSigs,
    /// 设置（设置页 v1，docs/active/设置页.md）：servers.json 条目表 +
    /// terminal.json 全局项。冷启动由壳读盘解析（缺/坏 → 现状默认 +
    /// 上报，配置文件不许炸终端）；切换键拦截/默认会话/ConnConfig/
    /// 配置页 UI 共用这份
    settings_servers: Vec<crate::settings::ServerEntry>,
    terminal_cfg: crate::settings::TerminalConfig,
    /// 全局切换键的拦截字节（= terminal_cfg.switch_hotkey 经 keymap
    /// 同一把尺产出的缓存，逐键比对免换算；空 = 不拦截）
    switch_hotkey_bytes: Vec<u8>,
}

/// 图层烘焙 sig 记账（ui-base §八）。判据纪律：**sig 必须列全该槽
/// paint 读过的每一个输入**——漏一个 = 陈旧像素（鬼影），比慢更严重。
///   键行槽：render_keybar(mods, ime_bottom=ime+bar_h, w, h)
///   面板槽：paint_ai_page_chrome(w, h, bottom_inset=ime+bar_h)——
///           panel_off/panel_fade 是合成期 placement/alpha，不进 sig
///           （烘焙画布恒靠泊位恒全实）
///   配置槽：paint_cfg_page_chrome(w, h, bottom_inset=ime+bar_h)——
///           同上，cfg_off/cfg_fade 是合成期 placement/alpha 不进 sig
///   上层槽：render_inputbar(bar_snap,sending,caret_on,ime,w,h) +
///           render_orb(ai_snap) + render_magnifier——orb_alpha_out 在
///           GLES 路径恒 true 不进 sig；放大镜内容跟终端网格活（网格
///           变化不进 sig），拖选期调用方强制重烘焙
/// 配置槽 sig（2026-09-12 双池骨架后 10 维：w/h/ime/bar_h + accent
/// c1/c2 + 标签栏选中/横滚/光标 x + 双池上池高；2026-09-13 三层目录
/// +1 维：池内容代际 epoch——聚焦/下拉/行表重建都必触发重烘焙，
/// 漏维 = 旧行表新聚焦鬼影）。
/// 13 维超元组 trait 上限（PartialEq/Default 只到 12 元），改具名
/// 结构——字段即注释，手机 1.97 的 E0277/E0599 就是踩在这上面
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct ConfigSig {
    w: u32,
    h: u32,
    ime: u32,
    bar_h: u32,
    /// accent c1
    c1: u32,
    /// accent c2
    c2: u32,
    /// 双池上池高
    pool_upper_h: u32,
    /// 池内容代际（cfg_page epoch，§五 目录语义）
    cfg_epoch: u64,
    // BAR-096 拆层：标签栏三维（sel/scroll/cx）与下池光标一维
    // （cursor_row_q）已移出本槽——它们各自的层（TabBar/LowerCursor）
    // 持小画布享自己的 sig。本槽从此**不再逐帧重烘**（动画期零光栅
    // 零上传）：游标滑行只脏 0.65MB 标签栏层、光标滑行只脏 0.69MB
    // 光标层（原三路逐帧 14MB 全页重烘 = draw_avg 47ms/21fps 真凶）
    /// 下拉进度 ×1000 量化（十五修 §六：开合动画逐帧新值逐帧重烘焙）
    dd_progress_q: u32,
    /// 触发器值框宽伸缩账瞬时值 px（二十四修 §六②：换选后值框宽
    /// 伸缩动画逐帧新值逐帧重烘行 0——与 dd_progress_q 同窗同拍，
    /// 收敛/无账恒 0 零空烧）
    trig_w_q: u32,
    /// 平移 hold 模式（十九修 D8/BAR-092 补丁）：pan 进行中=true——
    /// 模式开关只翻转两次（起步/贴死各一烘），不是动画进度，不违
    /// 合成期优先律；漏维 = 贴死后 hold 烘焙滞留（上池行消失到下次
    /// 交互，redroid rd427 f0008 实咬）
    pan_hold: bool,
    /// 动效预览时间桶（十四修 §六：动画展品开着 = boot_ms/33 逐帧
    /// 新值逐帧重烘焙；关着恒 0 不挤烘焙闸）
    anim_bucket: u64,
}

/// 池框几何层 sig（BAR-097）：(画布w, 画布h, 底inset, 上池高瞬时值, c1, c2)
type PoolFxSig = (u32, u32, u32, u32, u32, u32);
/// 下池行层 sig（BAR-097）：(画布w, 画布h, 底inset, 终点上池高, c1, c2,
/// 内容代际, 行表哈希)
type LowerRowsSig = (u32, u32, u32, u32, u32, u32, u64, u64);
/// 解析槽签名（与 LowerRowsSig 同形状不同语义——别名分开，谁改维度不殃及对方）
type ParserSig = (u32, u32, u32, u32, u32, u32, u64, u64);

#[derive(Default)]
struct LayerSigs {
    keybar: crate::ui::stage::DirtyGuard<(u8, u32, u32, u32, u32)>,
    panel: crate::ui::stage::DirtyGuard<(u32, u32, u32, u32)>,
    over: crate::ui::stage::DirtyGuard<OverSig>,
    config: crate::ui::stage::DirtyGuard<ConfigSig>,
    filetree: crate::ui::stage::DirtyGuard<(u32, u32, u32, u32, u32, u32)>,
    /// 解析槽：末两维 = tmux 插件 epoch（2026-09-19 插件卡内容随槽同烘焙）
    /// + 隧道 epoch（2026-09-20 连接/服务卡——状态翻转必重烘，漏维 = 鬼影）
    parser: crate::ui::stage::DirtyGuard<ParserSig>,
    termcard: crate::ui::stage::DirtyGuard<(u32, u32, u32, u32)>,
    /// 断线状态卡层（A 断线治理）：(w, h, session_over)——死活翻转
    /// 才重烘，稳态零成本
    downcard: crate::ui::stage::DirtyGuard<(u32, u32, u8)>,
    /// 标签栏层（BAR-096 拆槽）
    tabbar: crate::ui::stage::DirtyGuard<TabBarSig>,
    /// 下池光标层（BAR-096 拆槽）
    cursor: crate::ui::stage::DirtyGuard<CursorSig>,
    /// 池框几何层（BAR-097：Upper 平移期逐帧烘——upper.h 是 glide 瞬时
    /// 值逐帧变 = 逐帧烘；其余维不变不重烘）
    poolfx: crate::ui::stage::DirtyGuard<PoolFxSig>,
    /// 下池行层（BAR-097：起步一烘——内容哈希+epoch+终点几何锚定后
    /// 平移期恒定零重烘）
    lower_rows: crate::ui::stage::DirtyGuard<LowerRowsSig>,
    /// 下拉面板层（二十四修拆槽）：抽屉开合/细框滑行/宽度伸缩逐帧
    /// 只脏这块小画布；贴死（progress=0）即隐零重烘
    dropdown: crate::ui::stage::DirtyGuard<DropdownSig>,
    /// 平移旧代捕获账（十九修 D8）：Some((epoch, scope, dir)) = 当前
    /// PanOld 纹理属于哪笔平移账——新账才重新拷贝画布
    pan_cap: Option<(u64, u8, i8)>,
}

impl LayerSigs {
    /// GL 上下文重建（resumed 换 Gfx）后烘焙物全死——判定器全失效，
    /// 下一帧必然全量重烘焙（漏了这步 = 槽不画，画面缺层）
    fn invalidate_all(&mut self) {
        self.keybar.invalidate();
        self.panel.invalidate();
        self.over.invalidate();
        self.config.invalidate();
        self.filetree.invalidate();
        self.parser.invalidate();
        self.termcard.invalidate();
        self.downcard.invalidate();
        self.tabbar.invalidate();
        self.cursor.invalidate();
        self.poolfx.invalidate();
        self.lower_rows.invalidate();
        self.dropdown.invalidate();
    }
}

/// 标签栏层 sig（BAR-096 拆槽）：层画布 = 屏宽 × TAB_LAYER_H——游标
/// 滑行（cx_q 逐帧变）只脏这一层（0.65MB vs 配置槽 14MB）
#[derive(PartialEq)]
struct TabBarSig {
    w: u32,
    layer_h: u32,
    sel: u32,
    scroll: i32,
    cx_q: i32,
    c1: u32,
    c2: u32,
    line_span: (i32, i32),
    /// 标签文本 + 每标签随机双色的哈希（内容变必重烘，漏 = 旧标签鬼影）
    tabs_hash: u64,
}

/// 下池光标层 sig（BAR-096 拆槽）：层画布 = 池内容宽 × 下池行高。
/// 位置（px/py）进 sig 而非合成期：渐变参照吃「框在页上原位」的页坐标
/// （BAR-096 保真条——颜色不随层画布尺漂移），故位置变即重烘；小画布
/// 0.69MB ≈1-2ms，替代配置槽每次 14MB（帧饥饿根治）
#[derive(PartialEq)]
struct CursorSig {
    w: u32,
    h: u32,
    c1: u32,
    c2: u32,
    px: i32,
    py: i32,
    denom: i64,
    /// BAR-096 修：层内画选中行文字（复刻 BAR-089「先框后字」）——
    /// 行表代际/聚焦行变即重烘（小画布便宜；漏维 = 文字不跟选中行走）
    focus: u32,
    epoch: u64,
}

/// 下拉面板层 sig（二十四修拆槽）：层画布 = 面板最大宽 × 全高。
/// 位置（x/y）进 sig 而非合成期：抽屉刚体钉面板矩形（宽度账在播时
/// 左缘逐帧动 = 内容重烘保右缘锚定）；渐变 denom 画布尺（面板只活
/// 在层里，无稳态配置槽版本要对齐——无 BAR-096 保真条需求）
#[derive(PartialEq)]
struct DropdownSig {
    /// 画布宽（上池内容最大宽，右缘锚定容器）
    w: u32,
    /// 面板全高（progress=1 刚体尺）
    full_h: u32,
    /// 面板矩形页坐标（宽度账/池几何变即重烘）
    x: i32,
    y: i32,
    /// 抽屉开合进度 ×1000（开合/并发收起逐帧新值）
    progress_q: u32,
    /// 选中细框行号浮点 ×64（换选滑行逐帧新值）
    sel_q: i32,
    c1: u32,
    c2: u32,
    /// 选项表哈希（内容变必重烘，漏 = 旧选项鬼影）
    opts_hash: u64,
    /// 代际（保险维：换选/重建必翻）
    epoch: u64,
}

/// 上层槽 sig（derive PartialEq 深比较——BarSnap/PresenceSnap 均已
/// derive，逐帧比对成本可忽略）
#[derive(PartialEq)]
struct OverSig(
    bool,                                     // caret_on
    bool,                                     // sending
    Option<crate::input_bar::BarSnap>,        // bar_snap
    Option<crate::ai_presence::PresenceSnap>, // ai_snap（光球位置/增益）
    Option<(f64, f64)>,                       // magnifier_at
    u32,                                      // ime（chrome inset）
    u32,                                      // w
    u32,                                      // h
);

/// 默认脑路（2026-09-04 用户拍板改路）：智谱 coding 套餐 glm-5.3-flash。
/// 备选两路已配 key：Kimi 卡 kimi-for-coding-highspeed / DeepSeek 官网
/// deepseek-v4-flash-vision-exp——模型选择器是未来活（期 3 打磨），v1 定死
const DEFAULT_PROVIDER: &str = "智谱";
const DEFAULT_MODEL: &str = "glm-5.3-flash";

/// 读设置文件（设置页 v1，docs/active/设置页.md §2.5）：私有目录
/// settings/servers.json + settings/terminal.json。缺文件 = 现状默认
/// （空服务器表 → ConnConfig 走 8021 锚；terminal.json 缺 → 本地起步
/// + Ctrl-]）；坏文件 = 上报 + 回退默认——配置文件不许炸终端。
///
/// 配置文件不进 git——由脚本经隧道推送（ai/providers.json 同款纪律）
fn load_settings(
    app: Option<&winit::platform::android::activity::AndroidApp>,
) -> (
    Vec<crate::settings::ServerEntry>,
    crate::settings::TerminalConfig,
) {
    let mut servers = Vec::new();
    let mut term_cfg = crate::settings::TerminalConfig::default();
    let Some(dir) = app.and_then(|a| a.internal_data_path()) else {
        return (servers, term_cfg);
    };
    let cfg = dir.join("settings");
    if let Ok(j) = std::fs::read_to_string(cfg.join("servers.json")) {
        match crate::settings::parse_servers(&j) {
            Ok(v) => servers = v,
            Err(e) => crate::report::report_sync("term", &format!("servers.json 解析失败: {e}")),
        }
    }
    if let Ok(j) = std::fs::read_to_string(cfg.join("terminal.json")) {
        match crate::settings::parse_terminal(&j) {
            Ok(t) => term_cfg = t,
            Err(e) => crate::report::report_sync("term", &format!("terminal.json 解析失败: {e}")),
        }
    }
    (servers, term_cfg)
}

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

    /// chrome 跟随 inset（眼手同尺：触摸命中与渲染吃同一份采样值）。
    /// 采样在 draw_frame 每帧写回；无 ui-fx 占槽时 == 真实 inset（硬切）
    fn chrome_inset(&self) -> u32 {
        self.chrome_inset_px
    }

    /// 屏尺寸几何源（BAR-108 挂起态兜底）：窗口活着吃实时尺寸；退后台
    /// 窗口已弃（BAR-004）吃 last_win_px 缓存——触摸命中臂（跳框关闭/齿轮/
    /// 键栏/输入栏/面板拖拽）在后台注入时不许静默瞎掉。取舍律钉在
    /// ui/modal.rs pick_screen_px
    fn screen_px(&self) -> Option<(u32, u32)> {
        crate::ui::modal::pick_screen_px(
            self.window.as_ref().map(|w| {
                let s = w.inner_size();
                (s.width, s.height)
            }),
            self.last_win_px,
        )
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
    /// 面板跟手拖拽喂轨迹（§五B 手势升级）：锁定瞬把目标面板召入栈
    /// （目标值翻 0，渲染偏移由拖拽旁路接管不播自动动画）。返回 true =
    /// 本事件被拖拽消费（调用方直接 return）
    fn feed_panel_drag(&mut self, x: f64, y: f64) -> bool {
        use crate::ai_presence::Panel;
        use crate::ui::panel_drag::DragTop;
        let Some(d) = &mut self.panel_drag else {
            return false;
        };
        let top = match self.last_ai_snap.and_then(|s| s.top) {
            Some(Panel::Config) => DragTop::Config,
            Some(Panel::FileTree) => DragTop::FileTree,
            Some(Panel::Parser) => DragTop::Parser,
            _ => DragTop::Other,
        };
        // 字段级直取（self.screen_px() 借全 self 会撞 &mut panel_drag，
        // 借检只认字段级 disjoint——E0502 手机端实踩）
        let w = crate::ui::modal::pick_screen_px(
            self.window.as_ref().map(|w| {
                let s = w.inner_size();
                (s.width, s.height)
            }),
            self.last_win_px,
        )
        .map(|(sw, _)| sw)
        .unwrap_or(0) as f32;
        if w <= 0.0 {
            return false;
        }
        let now = crate::report::boot_ms() as u64;
        let was = d.locked();
        if let Some((role, _off)) = d.on_move(x, y, now, w, top) {
            if !was {
                // 手势追踪（「配置卡无法收回」案侦查）：锁定瞬间留痕
                crate::report::report(
                    "gest",
                    &format!(
                        "拖拽锁定: {role:?} 于({x:.0},{y:.0}) top={:?}",
                        self.last_ai_snap.and_then(|s| s.top)
                    ),
                );
                // 召唤拖拽锁定 = 立即入栈（拖拽期渲染靠栈存在性；
                // 新鲜召唤不 bump 入场代 → 无 replay 踢，不竞态）
                let summon = match role {
                    crate::ui::panel_drag::DragRole::SummonFileTree => Some(Panel::FileTree),
                    crate::ui::panel_drag::DragRole::SummonParser => Some(Panel::Parser),
                    _ => None,
                };
                if let (Some(ai), Some(p)) = (&self.ai_presence, summon) {
                    ai.summon_panel(p);
                }
            }
            self.dirty = true;
            return true;
        }
        false
    }

    /// 面板拖拽收尾（Ended/Cancelled）：裁决完成/取消 → 栈操作 +
    /// 缝 replay 踢从当前跟手偏移重定基续播（BAR-079 原语复用）。
    /// Cancelled 强制取消（系统抢手势不可信末段速度）
    fn finish_panel_drag(&mut self, cancelled: bool) {
        use crate::ai_presence::Panel;
        use crate::ui::panel_drag::{DragRole, ReleaseDecision};
        let now = crate::report::boot_ms() as u64;
        let Some(d) = self.panel_drag.take() else {
            return;
        };
        if !d.locked() {
            // 静默死点留痕（2026-09-11「配置卡无法收回」案侦查：本臂只从
            // 捏合第二指强取消路径可达——Ended 臂已先查 locked。旁观者被
            // 第二指收走 = 真手指持机干扰拖拽的头号嫌疑路径）
            crate::report::report("gest", "拖拽旁观者被第二指收走（捏合抢占）");
            return;
        }
        let w = self.screen_px().map(|(sw, _)| sw).unwrap_or(0) as f32;
        let cur = d.current_offset().unwrap_or(w);
        let decision = if cancelled {
            ReleaseDecision::Cancel
        } else {
            d.on_release(now, w)
        };
        // 角色 → 目标面板（四公民：右缘家=配置/解析、左缘家=文件树，
        // 栈操作同规）
        let role_panel = match d.role() {
            Some(DragRole::DismissConfig) => Some(Panel::Config),
            Some(DragRole::SummonFileTree) | Some(DragRole::DismissFileTree) => {
                Some(Panel::FileTree)
            }
            Some(DragRole::SummonParser) | Some(DragRole::DismissParser) => Some(Panel::Parser),
            None => None,
        };
        match (d.role(), decision) {
            // 召唤锁定时已入栈：完成 = 保持（目标 0）；取消 = 推回出栈
            (Some(DragRole::SummonFileTree), ReleaseDecision::Complete)
            | (Some(DragRole::SummonParser), ReleaseDecision::Complete) => {}
            (Some(DragRole::SummonFileTree), ReleaseDecision::Cancel)
            | (Some(DragRole::SummonParser), ReleaseDecision::Cancel) => {
                if let (Some(ai), Some(p)) = (&self.ai_presence, role_panel) {
                    ai.dismiss_top(p);
                }
            }
            // 推回：完成 = 出栈；取消 = 保持（目标回 0）
            (Some(DragRole::DismissConfig), ReleaseDecision::Complete)
            | (Some(DragRole::DismissFileTree), ReleaseDecision::Complete)
            | (Some(DragRole::DismissParser), ReleaseDecision::Complete) => {
                if let (Some(ai), Some(p)) = (&self.ai_presence, role_panel) {
                    ai.dismiss_top(p);
                }
            }
            (Some(DragRole::DismissConfig), ReleaseDecision::Cancel)
            | (Some(DragRole::DismissFileTree), ReleaseDecision::Cancel)
            | (Some(DragRole::DismissParser), ReleaseDecision::Cancel) => {}
            (None, _) => {}
        }
        // 收尾续播：从跟手偏移重定基到翻转后的目标值（方向分档曲线
        // 自动选臂——靠泊方向 250ms / 屏外方向 350ms，均减速到位）。
        // 按家踢各自的缝（右缘家 +cur（配置/解析）/ 文件树左缘 -cur，
        // cur 是距靠泊的距离，折符号后进 replay）
        match role_panel {
            Some(Panel::Config) => crate::ui::seam::replay_config_panel_offset_x(cur, now),
            Some(Panel::FileTree) => crate::ui::seam::replay_filetree_panel_offset_x(-cur, now),
            Some(Panel::Parser) => crate::ui::seam::replay_parser_panel_offset_x(cur, now),
            _ => {}
        }
        let after = self.ai_presence.as_ref().and_then(|ai| ai.snap(now).top);
        crate::report::report(
            "ui",
            &format!("拖拽收尾: {decision:?} 从偏移 {cur:.0} → 栈顶 {after:?}"),
        );
        self.dirty = true;
    }

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
                let in_input_bar = self.screen_px().is_some_and(|(_, sh)| {
                    crate::input_bar::in_bar(y, sh, self.chrome_inset(), bar_h)
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
                // 面板在顶（期 0④ 起手，2026-09-10 泛化面板栈 §五B）：
                // 在顶面板上只有输入栏是活区——其余位置手势归面板页
                // （AI 页拖动滚行/两页点按收键盘/水平快滑抽屉召唤推回），
                // 终端手势全家让路（快捷键行热区也不许穿透：面板盖着它，
                // 点得着看不见 = 幽灵键；被覆盖面板同样吃不到手势——
                // 路由按逻辑栈顶，不按 placement 过渡帧）
                let panel_top = self.last_ai_snap.and_then(|s| s.top);
                if panel_top.is_some() {
                    // 跳框模态仲裁（宪法 §六 跳框条款，九修）：配置页靠泊
                    // 且跳框开着 → 本手势全归跳框——压暗层吃下层全部触摸
                    // （标签栏/池区/面板拖拽全家让路）；抬手命中判定在
                    // Completed（几何吃 ui/modal.rs，眼手同尺）
                    if panel_top == Some(crate::ai_presence::Panel::Config)
                        && crate::ui::seam::sample_config_panel_offset_x(
                            0.0,
                            crate::report::boot_ms() as u64,
                        ) as i32
                            == 0
                        && let Some(page) = crate::ui::cfg_page::cfg_page_handle()
                        && page.lock().unwrap().modal().is_some()
                    {
                        crate::report::report("gest", &format!("起手→跳框模态 ({x:.0},{y:.0})"));
                        self.modal_touch = Some((x, y, false));
                        return;
                    }
                    // 标签栏仲裁（主题宪法 §四，2026-09-12）：配置页靠泊
                    // （cfg_off==0——过渡帧中不仲裁，手势归面板全家）且
                    // 起点在标签行带 → 手势归标签栏：横滑滚标签不触发
                    // 面板拖拽/页面滑向，点按选池
                    if panel_top == Some(crate::ai_presence::Panel::Config)
                        && crate::ui::tab_bar::in_row(y)
                        && crate::ui::seam::sample_config_panel_offset_x(
                            0.0,
                            crate::report::boot_ms() as u64,
                        ) as i32
                            == 0
                    {
                        crate::report::report("gest", &format!("起手→标签栏 ({x:.0},{y:.0})"));
                        self.tab_touch = Some(TabTouch {
                            start_x: x,
                            start_y: y,
                            last_x: x,
                            dragged: false,
                        });
                        return;
                    }
                    // 池区仲裁（宪法 §五 目录语义，2026-09-13）：配置页
                    // 靠泊（同标签栏的缝采样判据）时——
                    // ① 下拉开着：命中 panel 项 = 点选（上→下联动），
                    //    其他位置 = 收 panel；都吃掉本手势
                    // ② 起点在下拉触发器/下池区 → 建 cfg_pool_touch 槽：
                    //    点按抬手 = 触发器开合 / 下池行聚焦；拖过 slop
                    //    = 手势让回面板页（不聚焦不开合）
                    if panel_top == Some(crate::ai_presence::Panel::Config)
                        && crate::ui::seam::sample_config_panel_offset_x(
                            0.0,
                            crate::report::boot_ms() as u64,
                        ) as i32
                            == 0
                        && let (Some(pool), Some(page)) = (
                            crate::ui::dual_pool::dual_pool_handle(),
                            crate::ui::cfg_page::cfg_page_handle(),
                        )
                    {
                        let now = crate::report::boot_ms() as u64;
                        let (ps_snap, lower, upper) = {
                            let mut p = pool.lock().unwrap();
                            let ps = p.layout(now);
                            (ps.clone(), ps.lower.clone(), ps.upper.clone())
                        };
                        let (yi, xi) = (y as i64, x as i64);
                        // 十四修：下拉命中/触发器几何吃首行实量宽
                        // （先量后锁——锁序 term→cfg_page 不倒持）
                        let (lw, vw) = self.cfg_row0_text_widths();
                        let cw = self.cfg_dropdown_content_w_min();
                        let mut pg = page.lock().unwrap();
                        if pg.dropdown_open() {
                            // 十五修 §六：展开中选项不命中——点按 = 收
                            // （从当前展开度续收，不瞬消）
                            if pg.dropdown_progress(now) < 1.0 {
                                pg.dismiss_dropdown(now);
                                crate::report::report("gest", "下拉展开中点按→收 panel");
                                self.dirty = true;
                                return;
                            }
                            let max_h = 10_000; // 命中只问行号，钳高由涂装侧管
                            if let Some(i) = pg.dropdown_item_at_y(yi, &upper, max_h, lw, vw, cw) {
                                // 二十修 §六②：换选挂并发账 + UpperBody
                                // 平移账——旧代冻结吃当前池几何与页色
                                // （select 同规）
                                let acc = self
                                    .ai_presence
                                    .as_ref()
                                    .and_then(|ai| ai.accent_of(crate::ai_presence::Panel::Config))
                                    .unwrap_or(crate::ui::accent::FALLBACK);
                                pg.dropdown_pick(i, now, ps_snap, acc);
                                drop(pg);
                                // 下拉换选 = 默认服务器变更（二版：写盘+重建归壳）
                                self.apply_default_server_pick();
                                // 二十四修 §六②：值框宽度伸缩账——旧宽 =
                                // 点选前实量（上方 vw），新宽 = 重建后实量；
                                // 等长换选账不起（feed 内部 <1px 闸）
                                let (_, vw_new) = self.cfg_row0_text_widths();
                                if let Some(page) = crate::ui::cfg_page::cfg_page_handle() {
                                    page.lock().unwrap().feed_trigger_width(
                                        vw as f32,
                                        vw_new as f32,
                                        now,
                                    );
                                }
                                crate::report::report("gest", &format!("下拉点选→项 {i}"));
                            } else {
                                pg.dismiss_dropdown(now);
                                crate::report::report("gest", "下拉外点按→收 panel");
                            }
                            self.dirty = true;
                            return;
                        }
                        // 十五修 §六：收起中余影不穿透触摸——点按即时清零
                        if pg.dropdown_progress(now) > 0.0 {
                            pg.dropdown_dismiss_now();
                            crate::report::report("gest", "下拉余影点按→即时清零");
                            self.dirty = true;
                            return;
                        }
                        let tr = pg.trigger_rect(&upper, lw, vw);
                        let tab1 = pg.tab() == 1;
                        // 触发器是系统管理页家具——组件池页首行不是下拉行，
                        // 坐标重合也不许误判（九修：tab 维分流）
                        let in_trigger = !tab1
                            && xi >= tr.x
                            && xi < tr.x + tr.w as i64
                            && yi >= tr.y
                            && yi < tr.y + tr.h as i64;
                        let in_lower = xi >= lower.x
                            && xi < lower.x + lower.w as i64
                            && yi >= lower.y
                            && yi < lower.y + lower.h as i64;
                        // 组件池页：上池行可点开跳框（§五 目录语义 7）——
                        // 上池区也建池区手势槽
                        let in_upper = tab1
                            && xi >= upper.x
                            && xi < upper.x + upper.w as i64
                            && yi >= upper.y
                            && yi < upper.y + upper.h as i64;
                        if in_trigger || in_lower || in_upper {
                            crate::report::report(
                                "gest",
                                &format!(
                                    "起手→池区 ({x:.0},{y:.0}) {}",
                                    if in_trigger {
                                        "触发器"
                                    } else if in_upper {
                                        "上池"
                                    } else {
                                        "下池"
                                    }
                                ),
                            );
                            self.cfg_pool_touch = Some((x, y, false));
                            return;
                        }
                    }
                    // 解析页 tmux 插件卡仲裁（2026-09-19）：解析页靠泊
                    // （缝采样判据与配置页同规——过渡帧中不仲裁，手势归
                    // 面板全家）且起点在卡区 → 手势归插件：点按抬手 =
                    // 行切换/按钮/×；拖过 slop = 让回面板页（Moved 段）
                    if panel_top == Some(crate::ai_presence::Panel::Parser)
                        && crate::ui::seam::sample_parser_panel_offset_x(
                            0.0,
                            crate::report::boot_ms() as u64,
                        ) as i32
                            == 0
                        && let (Some(page), Some((sw, sh))) = (&self.parser_page, self.screen_px())
                    {
                        let in_card = {
                            let pg = page.lock().unwrap();
                            let snap = pg.snap();
                            let mode = if snap.naming.is_some() {
                                crate::ui::parser_page::Mode::Naming
                            } else if snap.confirming.is_some() {
                                crate::ui::parser_page::Mode::Confirming
                            } else {
                                crate::ui::parser_page::Mode::Normal
                            };
                            // 视口化：命中臂与涂装同一份 layout_vp（页面
                            // 滚动后卡链已平移——眼手同尺）
                            let n_svc = crate::ui::svc_card::current().lines.len();
                            let lay = crate::ui::parser_page::layout_vp(
                                sw,
                                sh,
                                self.cur_bar_h()
                                    + crate::ui::parser_chain::reserved_below_tmux(n_svc),
                                snap.sessions.len(),
                                n_svc,
                                mode,
                                snap.scroll,
                                snap.page_scroll,
                                crate::ui::parser_page::visible_bottom(
                                    sh,
                                    self.chrome_inset() + self.cur_bar_h(),
                                ),
                            );
                            let c = &lay.card;
                            let chain_h = crate::ui::parser_chain::heights(c.h, n_svc);
                            // 连接服务合并卡（第二张，两竖列——点按归
                            // 插件手势，不许漏到面板页当滑页起手）
                            let llay = crate::ui::link_card::layout_in(
                                crate::ui::parser_chain::slot_rect(
                                    crate::ui::parser_chain::ChainCardId::Link,
                                    c,
                                    &chain_h,
                                ),
                                n_svc,
                            );
                            let lc = &llay.card;
                            // 环境卡（第三张，纯展示——同归插件手势）
                            let xlay =
                                crate::ui::sys_card::layout_in(crate::ui::parser_chain::slot_rect(
                                    crate::ui::parser_chain::ChainCardId::Sys,
                                    c,
                                    &chain_h,
                                ));
                            let xc = &xlay.card;
                            let (xi, yi) = (x as i64, y as i64);
                            let in_rect = |r: &crate::ui::dual_pool::PoolRect| {
                                xi >= r.x
                                    && xi < r.x + i64::from(r.w)
                                    && yi >= r.y
                                    && yi < r.y + i64::from(r.h)
                            };
                            // 确认跳框（模态）在时全页吞触摸——跳框卡可能
                            // 居中在 tmux 卡外，且框外点按 = 取消
                            mode == crate::ui::parser_page::Mode::Confirming
                                || in_rect(c)
                                || in_rect(lc) // 连接服务合并卡同归插件手势（重连钮）
                                || in_rect(xc) // 环境卡同归（纯展示也吞）
                        };
                        if in_card {
                            crate::report::report(
                                "gest",
                                &format!("起手→解析页卡区 ({x:.0},{y:.0})"),
                            );
                            self.parser_touch = Some((x, y, None, y));
                            return;
                        }
                    }
                    // 手势追踪：第二指落在面板页会盖掉第一指的手势状态
                    // （panel_touch/panel_drag 单槽）——留痕取证
                    if self.panel_touch.is_some() {
                        crate::report::report("gest", "面板页第二指落下：第一指手势状态被盖");
                    }
                    crate::report::report(
                        "gest",
                        &format!("起手→面板页 ({x:.0},{y:.0}) top={panel_top:?}"),
                    );
                    self.panel_touch = Some(PanelTouch {
                        start_x: x,
                        start_y: y,
                        last_y: y,
                        acc_px: 0.0,
                        dragged: false,
                    });
                    // 跟手拖拽旁观者（§五B 升级）：锁定前零行为，
                    // 横向一锁即接管本手势
                    self.panel_drag = Some(crate::ui::panel_drag::PanelDrag::new(
                        x,
                        y,
                        crate::report::boot_ms() as u64,
                    ));
                    return;
                }
                // 断线状态卡命中（A 断线治理，ui/down_card.rs）：活跃会话
                // 死了卡才在屏——登记即归钮，抬手同钮才触发（拖过 slop 不
                // 触发）。在齿轮/快捷键行之前拦：死会话上终端手势本就
                // 只剩滚动，卡的两个钮是此时唯一的图形出路
                if self.session_over
                    && let Some((sw, _)) = self.screen_px()
                    && let Some(h) = crate::ui::down_card::hit(x, y, sw)
                {
                    self.down_touch = Some((id, x, y, h, false));
                    return;
                }
                // 设置钮命中（2026-09-12 配置池卡按钮入口，ui/gear.rs）：
                // 只在裸终端页走到这（面板在顶上面已 return）——「文件树/
                // 浏览器在顶时设置不出现」从路由自然推出，栈零新规则。
                // 登记即归钮，点按抬手才召唤（拖过 slop 不触发）
                if let Some((sw, _)) = self.screen_px()
                    && crate::ui::gear::hit(x, y, sw)
                {
                    self.gear_touch = Some((id, x, y, false));
                    return;
                }
                // 起点在快捷键行带上 → 这手势归行（不滚屏不唤键盘）
                // BAR-018：判定尺与渲染/hit 一致——减去键盘 inset，
                // 否则键盘弹起时行带浮在 inset 上方，这里却认屏底。
                // 期 0 组件三：行上移一层（输入栏压底），有效 inset + 当前栏高
                let in_bar = self.screen_px().is_some_and(|(_, sh)| {
                    crate::keybar::in_bar(y, sh, self.chrome_inset() + bar_h)
                });
                if in_bar {
                    self.bar_touch = Some((x, y));
                    // 方向键武装长按连发（主循环 check_bar_repeat 每圈 poll）；
                    // 武装命中与抬手发键同一把尺（keybar::hit + 同款 inset）
                    if let Some((sw, sh)) = self.screen_px()
                        && let Some(kd) =
                            crate::keybar::hit(x, y, sw, sh, self.chrome_inset() + bar_h)
                        && let crate::keybar::Key::Direct(code) = kd.key
                    {
                        self.bar_repeat =
                            crate::keybar::KeyRepeat::arm(code, std::time::Instant::now());
                    }
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
                    // 拖拽中第二指落下 = 系统级手势变更：强制取消跟手
                    // 拖拽（收尾续播回起点），捏合接管
                    self.finish_panel_drag(true);
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
                // 跟手拖拽旁观者（§五B 升级：终端裸奔左滑拉配置页）
                self.panel_drag = Some(crate::ui::panel_drag::PanelDrag::new(
                    x,
                    y,
                    crate::report::boot_ms() as u64,
                ));
                if !selecting {
                    let cell_h = self
                        .term_handle()
                        .map(|t| t.lock().unwrap().cell_size().1)
                        .unwrap_or(crate::termview::CELL_H);
                    self.touch_scroll = Some(crate::scroll::TouchScroll::new(y, f64::from(cell_h)));
                }
            }
            TouchPhase::Moved => {
                // 断线卡钮手势：只认本指——超 slop 记拖过（抬手不触发）
                if let Some(dt) = &mut self.down_touch
                    && dt.0 == id
                {
                    if (x - dt.1).abs() > crate::scroll::TAP_SLOP_PX
                        || (y - dt.2).abs() > crate::scroll::TAP_SLOP_PX
                    {
                        dt.4 = true;
                    }
                    return;
                }
                // 设置钮手势（2026-09-12）：只认本指——超 slop 记拖过
                // （抬手不触发召唤），它指事件放行走原分路
                if let Some(gt) = &mut self.gear_touch
                    && gt.0 == id
                {
                    if (x - gt.1).abs() > crate::scroll::TAP_SLOP_PX
                        || (y - gt.2).abs() > crate::scroll::TAP_SLOP_PX
                    {
                        gt.3 = true;
                    }
                    return;
                }
                // 面板跟手拖拽优先（§五B 升级）：已锁定/本事件锁定的
                // 手势归拖拽——面板偏移直跟手指，原分路（滚屏/滚行/
                // 点按候选）全部让路。未锁定 = 旁观者，零影响
                if self.panel_drag.is_some() && self.feed_panel_drag(x, y) {
                    // 拖拽一旦接管，滚动机/按压计时作废（防拖拽中滚屏
                    // 或长按走火）
                    self.touch_scroll = None;
                    self.press = None;
                    return;
                }
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
                // 跳框模态手势：拖过 slop 只记账（抬手不归点按不收框）；
                // 模态期间手势不出本槽（压暗层吃下层，宪法 §六）
                if let Some(mt) = self.modal_touch.as_mut() {
                    if !mt.2
                        && ((x - mt.0).abs() > crate::scroll::TAP_SLOP_PX
                            || (y - mt.1).abs() > crate::scroll::TAP_SLOP_PX)
                    {
                        mt.2 = true;
                    }
                    return;
                }
                // 标签栏手势：拖过 slop = 横滚标签（pan 像素级跟手，
                // clamp 在核心 tab_bar）；横向位移不喂面板拖拽（仲裁条款）
                if let Some(tt) = self.tab_touch.as_mut() {
                    if !tt.dragged
                        && ((x - tt.start_x).abs() > crate::scroll::TAP_SLOP_PX
                            || (y - tt.start_y).abs() > crate::scroll::TAP_SLOP_PX)
                    {
                        tt.dragged = true;
                    }
                    if tt.dragged {
                        if let Some(bar) = &self.tab_bar {
                            bar.lock().unwrap().pan(x - tt.last_x);
                        }
                        self.dirty = true;
                    }
                    tt.last_x = x;
                    return;
                }
                // 池区手势（宪法 §五 目录语义）：起手槽只是点按候选的
                // 扣留席——拖过 slop 即**让回面板页全家**（2026-09-13 用户
                // 实机案：「下池右滑回不去页面」——病灶 = 槽只记账不把
                // 手势交出去，横向锁/快滑裁决永远轮不到它）。让回 = 补建
                // panel_drag 旁观者（从起手点建，立即补喂当前点——横向
                // 一锁就跟手推回）+ panel_touch（dragged=true 点按作废，
                // 纵向续上池滚动、抬手 decide_swipe 抽屉裁决）
                if let Some(ct) = self.cfg_pool_touch.take() {
                    if !ct.2
                        && ((x - ct.0).abs() > crate::scroll::TAP_SLOP_PX
                            || (y - ct.1).abs() > crate::scroll::TAP_SLOP_PX)
                    {
                        crate::report::report(
                            "gest",
                            &format!("池区手势让回面板页 ({x:.0},{y:.0})"),
                        );
                        self.panel_touch = Some(PanelTouch {
                            start_x: ct.0,
                            start_y: ct.1,
                            last_y: y,
                            acc_px: 0.0,
                            dragged: true,
                        });
                        self.panel_drag = Some(crate::ui::panel_drag::PanelDrag::new(
                            ct.0,
                            ct.1,
                            crate::report::boot_ms() as u64,
                        ));
                        if self.feed_panel_drag(x, y) {
                            return;
                        }
                        // 本事件不补滚动（防 slop 跳变），下事件起续
                        self.dirty = true;
                        return;
                    }
                    self.cfg_pool_touch = Some(ct); // 未过 slop：槽放回去继续扣留
                    return;
                }
                // 解析页卡区手势：起手槽是点按候选扣留席。拖过 slop 分流——
                // 常态 + 垂直主导：起手落会话框表带且表可滚（>6 框）→
                // Session 表内滚动（2026-09-19 用户拍板）；其余卡区且页面
                // 可滚 → Page 页面滚动（2026-09-20 视口化用户拍板「卡弹小+
                // 上下能滑动」）；否则让回面板页全家（横向锁/抽屉裁决才轮
                // 得到它）
                if let Some(pt) = self.parser_touch.take() {
                    if let Some(kind) = pt.2 {
                        // 滚动态：垂直增量跟手（手指上推 dy<0 = 看后部
                        // = scroll 增）。max 每次吃当下几何（状态核不揣屏寸）
                        let dy = y - pt.3;
                        if dy != 0.0
                            && let (Some(page), Some((sw, sh))) =
                                (&self.parser_page, self.screen_px())
                        {
                            let (smax, pmax) = {
                                let pg = page.lock().unwrap();
                                let snap = pg.snap();
                                let lay = crate::ui::parser_page::layout_vp(
                                    sw,
                                    sh,
                                    self.cur_bar_h()
                                        + crate::ui::parser_chain::reserved_below_tmux(
                                            crate::ui::svc_card::current().lines.len(),
                                        ),
                                    snap.sessions.len(),
                                    crate::ui::svc_card::current().lines.len(),
                                    crate::ui::parser_page::Mode::Normal,
                                    snap.scroll,
                                    snap.page_scroll,
                                    crate::ui::parser_page::visible_bottom(
                                        sh,
                                        self.chrome_inset() + self.cur_bar_h(),
                                    ),
                                );
                                (lay.scroll_max, lay.page_scroll_max)
                            };
                            let mut pg = page.lock().unwrap();
                            match kind {
                                ParserDrag::Session => pg.scroll_by(-(dy as i64), smax),
                                ParserDrag::Page => pg.page_scroll_by(-(dy as i64), pmax),
                            }
                        }
                        self.parser_touch = Some((pt.0, pt.1, Some(kind), y));
                        self.dirty = true;
                        return;
                    }
                    if (x - pt.0).abs() > crate::scroll::TAP_SLOP_PX
                        || (y - pt.1).abs() > crate::scroll::TAP_SLOP_PX
                    {
                        // 滚动仲裁：常态（命名/确认态不滚）+ 垂直主导；
                        // 分流 = 起手落点定（框表带→表滚，其余卡区→页滚）
                        let vertical = (y - pt.1).abs() > (x - pt.0).abs();
                        let drag = if vertical
                            && let (Some(page), Some((sw, sh))) =
                                (&self.parser_page, self.screen_px())
                        {
                            let pg = page.lock().unwrap();
                            let snap = pg.snap();
                            if snap.naming.is_some() || snap.confirming.is_some() {
                                None
                            } else {
                                let lay = crate::ui::parser_page::layout_vp(
                                    sw,
                                    sh,
                                    self.cur_bar_h()
                                        + crate::ui::parser_chain::reserved_below_tmux(
                                            crate::ui::svc_card::current().lines.len(),
                                        ),
                                    snap.sessions.len(),
                                    crate::ui::svc_card::current().lines.len(),
                                    crate::ui::parser_page::Mode::Normal,
                                    snap.scroll,
                                    snap.page_scroll,
                                    crate::ui::parser_page::visible_bottom(
                                        sh,
                                        self.chrome_inset() + self.cur_bar_h(),
                                    ),
                                );
                                let in_list = pt.0 as i64 >= lay.card.x
                                    && (pt.0 as i64) < lay.card.x + i64::from(lay.card.w)
                                    && pt.1 as i64 >= lay.list_clip.0
                                    && (pt.1 as i64) < lay.list_clip.1;
                                if in_list && lay.scroll_max > 0 {
                                    Some(ParserDrag::Session)
                                } else if lay.page_scroll_max > 0 {
                                    Some(ParserDrag::Page)
                                } else {
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        if let Some(kind) = drag {
                            crate::report::report(
                                "gest",
                                &format!("解析页卡区手势→{kind:?}滚动 ({x:.0},{y:.0})"),
                            );
                            self.parser_touch = Some((pt.0, pt.1, Some(kind), y));
                            self.dirty = true;
                            return;
                        }
                        crate::report::report(
                            "gest",
                            &format!("解析页卡区手势让回面板页 ({x:.0},{y:.0})"),
                        );
                        self.panel_touch = Some(PanelTouch {
                            start_x: pt.0,
                            start_y: pt.1,
                            last_y: y,
                            acc_px: 0.0,
                            dragged: true,
                        });
                        self.panel_drag = Some(crate::ui::panel_drag::PanelDrag::new(
                            pt.0,
                            pt.1,
                            crate::report::boot_ms() as u64,
                        ));
                        if self.feed_panel_drag(x, y) {
                            return;
                        }
                        self.dirty = true;
                        return;
                    }
                    self.parser_touch = Some(pt); // 未过 slop：槽放回去继续扣留
                    return;
                }
                // 面板页手势：AI 页拖动 = 对话页滚行（像素级累积跟手，
                // 行高与渲染同尺 AI_PAGE_LINE_H；方向契约在 ui/ai_page.rs
                // drag_accum_rows——下滑 = 看更早，BAR-064）；配置页拖动
                // = 上池像素滚动（§五 四版，起手落上池才滚，下分支）；
                // 水平位移只攒着，抽屉识别在抬手（decide_swipe）
                if let Some(apt) = self.panel_touch.as_mut() {
                    let dy = y - apt.last_y;
                    apt.last_y = y;
                    if (y - apt.start_y).abs() > crate::scroll::TAP_SLOP_PX
                        || (x - apt.start_x).abs() > crate::scroll::TAP_SLOP_PX
                    {
                        apt.dragged = true;
                    }
                    let top_is_ai = self
                        .last_ai_snap
                        .is_some_and(|s| s.top == Some(crate::ai_presence::Panel::Ai));
                    if top_is_ai {
                        let (acc, rows) = crate::ui::ai_page::drag_accum_rows(
                            apt.acc_px,
                            dy,
                            f64::from(crate::termview::AI_PAGE_LINE_H),
                        );
                        apt.acc_px = acc;
                        if rows != 0 {
                            if let Some(chat) = &self.ai_chat {
                                chat.scroll_drag_rows(rows);
                            }
                            self.dirty = true;
                        }
                    } else {
                        // 配置页在顶 + 起手落上池 + 纵向 = 上池像素滚动
                        // （§五 四版滚动条款：1:1 跟手，手指上推 = 内容
                        // 上移）。横向不冲突——panel_drag 横向一锁即让路
                        // 页面跟手（四公民一滑一义 §五B 仲裁不变）；
                        // 触发器/下池起手走 cfg_pool_touch 槽不到这
                        let top_is_cfg = self
                            .last_ai_snap
                            .is_some_and(|s| s.top == Some(crate::ai_presence::Panel::Config));
                        if top_is_cfg
                            && dy != 0.0
                            && !self.panel_drag.as_ref().is_some_and(|d| d.locked())
                            && let (Some(pool), Some(page)) = (
                                crate::ui::dual_pool::dual_pool_handle(),
                                crate::ui::cfg_page::cfg_page_handle(),
                            )
                        {
                            let upper = pool
                                .lock()
                                .unwrap()
                                .layout(crate::report::boot_ms() as u64)
                                .upper;
                            let in_upper = apt.start_x >= upper.x as f64
                                && apt.start_x < (upper.x + i64::from(upper.w)) as f64
                                && apt.start_y >= upper.y as f64
                                && apt.start_y < (upper.y + i64::from(upper.h)) as f64;
                            if in_upper && page.lock().unwrap().scroll_upper_by(-dy as i64, upper.h)
                            {
                                self.dirty = true;
                            }
                        }
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
                // 面板跟手拖拽收尾（§五B 升级）：已锁定 = 裁决完成/取消
                // + replay 踢续播，其后分路（点按/抽屉快滑/选择）全让路；
                // 未锁定的旁观者清掉（Cancelled 强制取消——系统抢手势
                // 不可信末段速度）
                if self.panel_drag.as_ref().is_some_and(|d| d.locked()) {
                    self.finish_panel_drag(matches!(phase, TouchPhase::Cancelled));
                    // 同一指的面板页手势状态同生同灭——残留态会被下一指
                    // 的 Started 误判成「第二指落下」（08:18 实机误报实踩），
                    // 旧起点坐标还有骗出幽灵抽屉手势的风险
                    self.panel_touch = None;
                    return;
                }
                self.panel_drag = None;
                // 捏合收尾：任一指抬起即结束，缩放比写盘 + [zoom] 上报。
                // 残余指头不接管滚动/点按（touch_scroll/press 进捏合时已清）
                if self.pinch.take().is_some() {
                    self.persist_zoom();
                    return;
                }
                // 断线卡钮手势收尾（A 断线治理）：本指抬起、未拖过 slop、
                // 落点仍同一钮 = 触发。重试 = kick_reconnect（敲键重连
                // 同路）；切本地 = switch_session（Ctrl-] 同路）。栈动作
                // 必须留痕（「点了没反应」静默死点治理同规）
                if self.down_touch.as_ref().is_some_and(|d| d.0 == id) {
                    let dt = self.down_touch.take().unwrap();
                    if phase == TouchPhase::Ended
                        && !dt.4
                        && let Some((sw, _)) = self.screen_px()
                        && crate::ui::down_card::hit(x, y, sw) == Some(dt.3)
                    {
                        match dt.3 {
                            crate::ui::down_card::DownHit::Retry => {
                                crate::report::report("term", "断线卡点重试 → kick_reconnect");
                                self.kick_reconnect();
                            }
                            crate::ui::down_card::DownHit::Local => {
                                crate::report::report("term", "断线卡点切本地 → switch_session");
                                self.switch_session();
                            }
                        }
                    }
                    self.dirty = true;
                    return;
                }
                // 设置钮手势收尾（2026-09-12 配置池卡按钮入口）：本指
                // 抬起且未拖过 slop = 点按 → 召唤配置页（栈操作留痕，
                // 「配置卡无法收回」案教训：栈动作必须日志可见）
                if self.gear_touch.as_ref().is_some_and(|g| g.0 == id) {
                    let gt = self.gear_touch.take().unwrap();
                    if phase == TouchPhase::Ended
                        && !gt.3
                        && let Some(ai) = &self.ai_presence
                    {
                        let before = self.last_ai_snap.and_then(|s| s.top);
                        ai.summon_panel(crate::ai_presence::Panel::Config);
                        let after = ai.snap(crate::report::boot_ms() as u64).top;
                        crate::report::report(
                            "gest",
                            &format!("设置钮点按: 栈顶 {before:?}→{after:?}"),
                        );
                    }
                    self.dirty = true;
                    return;
                }
                // 光球手势收尾：pressed 复位；无位移短按抬起 → tap 切页
                // （Cancelled / 拖过 / 长按已发 fake_run 的抬手不补 tap）
                if let Some(ot) = self.orb_touch.take() {
                    if let Some(ai) = &self.ai_presence {
                        ai.press_up();
                        if phase == TouchPhase::Ended && !ot.dragged && !ot.long_fired {
                            // 手势追踪：光球点按是栈操作（召唤/收起 AI 面板）
                            // 却一直零日志——「配置卡无法收回」案的盲区实锤
                            // （用户两球门间右滑全空操作，就因为顶被点翻了）
                            let before = self.last_ai_snap.and_then(|s| s.top);
                            ai.tap_orb();
                            let after = ai.snap(crate::report::boot_ms() as u64).top;
                            crate::report::report(
                                "gest",
                                &format!("光球点按: 栈顶 {before:?}→{after:?}"),
                            );
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
                    let action = self.screen_px().and_then(|(sw, sh)| {
                        crate::input_bar::hit(x, y, sw, sh, self.chrome_inset(), bar_h)
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
                // 跳框模态收尾（宪法 §六，九修）：未拖抬手 = 命中判定
                // （几何吃 ui/modal.rs 同一份——眼手同尺）：关闭钮/框外 =
                // 收起；框内其他 = 无操作吃手势。Cancelled/拖过 slop 零动作
                // BAR-108：几何源走 screen_px（挂起态吃缓存）——曾因直查
                // self.window 在退后台后静默跳过，跳框关不死
                if let Some(mt) = self.modal_touch.take() {
                    if phase == TouchPhase::Ended
                        && !mt.2
                        && let (Some(page), Some((sw, sh))) =
                            (crate::ui::cfg_page::cfg_page_handle(), self.screen_px())
                    {
                        let mut pg = page.lock().unwrap();
                        if let Some(mi) = pg.modal() {
                            use crate::ui::modal as md;
                            let comps = crate::ui::comp_registry::COMPONENTS;
                            let entry = &comps[mi.min(comps.len() - 1)];
                            let fields = md::fields_of(entry, md::content_cells(sw));
                            let card = md::card_rect(sw, sh, &fields);
                            match md::hit(mt.0 as i64, mt.1 as i64, &card) {
                                md::ModalHit::Close | md::ModalHit::Outside => {
                                    pg.close_modal();
                                    crate::report::report("ui", "跳框收起（关闭钮/框外）");
                                }
                                md::ModalHit::Card => {
                                    crate::report::report("ui", "跳框卡内点按（无操作，吃手势）");
                                }
                            }
                        }
                    }
                    self.dirty = true;
                    return;
                }
                // 标签栏手势收尾：未拖抬手 = 点按选池（hit/select/弹簧
                // 重定基全在核心 tab_bar）；收键盘同面板点按（标签栏不是
                // 输入区）；Cancelled = 系统抢手势零动作留痕
                if let Some(tt) = self.tab_touch.take() {
                    if phase == TouchPhase::Cancelled {
                        crate::report::report(
                            "gest",
                            &format!(
                                "标签栏手势取消: dx={:.0} dragged={}（系统抢手势，零动作）",
                                x - tt.start_x,
                                tt.dragged
                            ),
                        );
                        self.dirty = true;
                        return;
                    }
                    if phase == TouchPhase::Ended && !tt.dragged {
                        let mut picked = None;
                        let mut picked_pair = None;
                        if let Some(bar) = &self.tab_bar {
                            let mut g = bar.lock().unwrap();
                            if let Some(i) = g.hit(x, y) {
                                g.select(i, crate::report::boot_ms() as u64);
                                picked_pair = Some(g.selected_pair());
                                crate::report::report(
                                    "ui",
                                    &format!("标签栏点按: 选中池 {}（{}）", i, g.tabs()[i]),
                                );
                                picked = Some(i);
                            }
                        }
                        // 十七修 §六：页面级平移旧代冻结要「换色前」的
                        // 旧 accent——retint 后 accent_of 已是新色，先取
                        let old_cfg_accent = self
                            .ai_presence
                            .as_ref()
                            .and_then(|ai| ai.accent_of(crate::ai_presence::Panel::Config))
                            .or(picked_pair)
                            .unwrap_or(crate::ui::accent::FALLBACK);
                        // 十一修（§四）：点选 = 整页瞬时换成该标签双色
                        // （retint 只换 accent 不重随；弹簧移动在核心自走）
                        if let (Some(pair), Some(ai)) = (picked_pair, &self.ai_presence) {
                            ai.retint_cfg(pair);
                        }
                        // 切标签 = 切池页（九修：组件池入列）：核心记 tab 维
                        // + 切页清零，内容重建归壳（rebuild 按 tab 分流）
                        if let Some(i) = picked {
                            if let Some(page) = &self.cfg_page {
                                let now = crate::report::boot_ms() as u64;
                                if let Some(pool) = crate::ui::dual_pool::dual_pool_handle() {
                                    let ps = pool.lock().unwrap().layout(now);
                                    page.lock().unwrap().set_tab(i, now, ps, old_cfg_accent);
                                }
                            }
                            self.rebuild_cfg_rows();
                            self.dirty = true;
                        }
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
                    }
                    self.dirty = true;
                    return;
                }
                // 池区手势收尾：未拖抬手 = 点按——起点在触发器 = 开合
                // 下拉；起点在下池 = 行命中聚焦（下→上联动，字段行重建）；
                // 拖过 slop / Cancelled = 零动作
                if let Some(ct) = self.cfg_pool_touch.take() {
                    if phase == TouchPhase::Ended && !ct.2 {
                        let (xi, yi) = (ct.0 as i64, ct.1 as i64);
                        if let (Some(pool), Some(page)) = (
                            crate::ui::dual_pool::dual_pool_handle(),
                            crate::ui::cfg_page::cfg_page_handle(),
                        ) {
                            let (ps, lower, upper) = {
                                let now = crate::report::boot_ms() as u64;
                                let ps = pool.lock().unwrap().layout(now);
                                (ps.clone(), ps.lower.clone(), ps.upper.clone())
                            };
                            // 十四修：触发器命中吃首行实量宽（先量后锁）
                            let (lw, vw) = self.cfg_row0_text_widths();
                            let mut pg = page.lock().unwrap();
                            let tr = pg.trigger_rect(&upper, lw, vw);
                            let tab1 = pg.tab() == 1;
                            let in_trigger = !tab1
                                && xi >= tr.x
                                && xi < tr.x + tr.w as i64
                                && yi >= tr.y
                                && yi < tr.y + tr.h as i64;
                            let now = crate::report::boot_ms() as u64;
                            if in_trigger {
                                pg.toggle_dropdown(now);
                                crate::report::report("gest", "下拉触发器点按→开合");
                                self.dirty = true;
                            } else if let Some(i) = pg.lower_row_at_y(yi, &lower) {
                                // 十七修 §六：上池级平移旧代冻结吃当前池几何
                                // 与页色（选行不换色，accent 仍要入快照——
                                // 旧代上池内容带当时页色出）
                                let acc = self
                                    .ai_presence
                                    .as_ref()
                                    .and_then(|ai| ai.accent_of(crate::ai_presence::Panel::Config))
                                    .unwrap_or(crate::ui::accent::FALLBACK);
                                pg.select(i, now, ps, acc);
                                drop(pg);
                                self.rebuild_cfg_rows();
                                crate::report::report("ui", &format!("下池点按: 聚焦行 {i}"));
                                self.dirty = true;
                            } else if tab1 {
                                // 组件池页：上池行点按 = 开跳框（宪法 §六
                                // 跳框条款；行号 → COMPONENTS 下标走
                                // entries_of 同一份表——眼手同尺）
                                let scroll = pg.upper_scroll();
                                if let Some(i) = pg.upper_row_at_y(yi, &upper, scroll) {
                                    use crate::ui::comp_registry as cr;
                                    let cat = cr::CATEGORIES
                                        .get(pg.focus())
                                        .copied()
                                        .unwrap_or(cr::CATEGORIES[0]);
                                    if let Some(&ci) = cr::entries_of(cat).get(i) {
                                        pg.open_modal(ci);
                                        crate::report::report(
                                            "ui",
                                            &format!("组件池点按: 开跳框 #{ci}"),
                                        );
                                        self.dirty = true;
                                    }
                                }
                            }
                        }
                    }
                    return;
                }
                // 解析页卡区手势收尾（2026-09-19 tmux 插件）：未拖抬手 =
                // 命中判定（几何吃 ui/parser_page::layout_vp 同一份——眼手
                // 同尺）：行 = 切换 attach / × = 开确认 / 按钮 = 动作分发；
                // 拖过 slop / Cancelled = 零动作（Moved 段已让回面板页）
                if let Some(pt) = self.parser_touch.take() {
                    if phase == TouchPhase::Ended
                        && pt.2.is_none()
                        && let (Some(page), Some((sw, sh))) = (&self.parser_page, self.screen_px())
                    {
                        let (snap, hit_result, conn_hit) = {
                            let pg = page.lock().unwrap();
                            let snap = pg.snap();
                            let mode = if snap.naming.is_some() {
                                crate::ui::parser_page::Mode::Naming
                            } else if snap.confirming.is_some() {
                                crate::ui::parser_page::Mode::Confirming
                            } else {
                                crate::ui::parser_page::Mode::Normal
                            };
                            let lay = crate::ui::parser_page::layout_vp(
                                sw,
                                sh,
                                self.cur_bar_h()
                                    + crate::ui::parser_chain::reserved_below_tmux(
                                        crate::ui::svc_card::current().lines.len(),
                                    ),
                                snap.sessions.len(),
                                crate::ui::svc_card::current().lines.len(),
                                mode,
                                snap.scroll,
                                snap.page_scroll,
                                crate::ui::parser_page::visible_bottom(
                                    sh,
                                    self.chrome_inset() + self.cur_bar_h(),
                                ),
                            );
                            let h = crate::ui::parser_page::hit(
                                &lay,
                                pt.0 as i64,
                                pt.1 as i64,
                                sw,
                                sh,
                                mode,
                            );
                            // tmux 卡未命中且常态 → 连接服务合并卡
                            // （几何同一份 lay.card 推出——眼手同尺）；
                            // 模态/命名态屏蔽（模态跳框期间卡区命中全
                            // 屏蔽惯例）
                            let ch = if h.is_none() && mode == crate::ui::parser_page::Mode::Normal
                            {
                                let n_svc = crate::ui::svc_card::current().lines.len();
                                let llay = crate::ui::link_card::layout_in(
                                    crate::ui::parser_chain::slot_rect(
                                        crate::ui::parser_chain::ChainCardId::Link,
                                        &lay.card,
                                        &crate::ui::parser_chain::heights(lay.card.h, n_svc),
                                    ),
                                    n_svc,
                                );
                                crate::ui::link_card::hit(&llay, pt.0 as i64, pt.1 as i64)
                            } else {
                                None
                            };
                            (snap, h.map(|hh| (hh, mode)), ch)
                        };
                        if let Some((hh, mode)) = hit_result {
                            self.parser_dispatch(snap, hh, mode);
                        }
                        if let Some(crate::ui::link_card::LinkHit::Reconnect) = conn_hit {
                            let ok = crate::tunnel::request_reconnect();
                            crate::report::report(
                                "tunnel",
                                &format!("连接服务卡点重连 → 下达{ok}"),
                            );
                        }
                    }
                    self.dirty = true;
                    return;
                }
                // 过 slop）= 输入栏失焦 + 收键盘——面板不是输入区，绝不穿透
                // 召唤终端输入法（期 0④ 用户拍板两条：不穿透 + 点非输入区
                // 自动收键盘）
                if let Some(apt) = self.panel_touch.take() {
                    // 账洞留痕（2026-09-11）：Cancelled 此前全静默——起手行
                    // 「起手→面板页」无收尾，日志流上就是一次无疾而终的手势，
                    // 与「手势落空」无从区分。强制取消同样记一行（系统抢手势，
                    // 不做任何栈/焦点动作）
                    if phase == TouchPhase::Cancelled {
                        crate::report::report(
                            "gest",
                            &format!(
                                "面板页手势取消: dx={:.0} dy={:.0} dragged={}（系统抢手势，零动作）",
                                x - apt.start_x,
                                y - apt.start_y,
                                apt.dragged
                            ),
                        );
                        self.dirty = true;
                        return;
                    }
                    if phase == TouchPhase::Ended
                        && let Some(dir) =
                            crate::ai_presence::decide_swipe(x - apt.start_x, y - apt.start_y)
                    {
                        if let Some(ai) = &self.ai_presence {
                            match dir {
                                crate::ai_presence::SwipeDir::Left => ai.swipe_left(),
                                crate::ai_presence::SwipeDir::Right => ai.swipe_right(),
                            }
                        }
                        let after = self
                            .ai_presence
                            .as_ref()
                            .and_then(|ai| ai.snap(crate::report::boot_ms() as u64).top);
                        crate::report::report("ui", &format!("抽屉手势: {dir:?} → 栈顶 {after:?}"));
                        self.dirty = true;
                        return;
                    }
                    // 静默死点留痕（「配置卡无法收回」案侦查）：面板页位移
                    // 手势既没锁拖拽（24px/1.8 斜率）也没过抽屉阈（90px）
                    // = 动作落空零响应——两阈之间的死区全在这一行现形
                    if phase == TouchPhase::Ended && apt.dragged {
                        crate::report::report(
                            "gest",
                            &format!(
                                "面板页手势落空: dx={:.0} dy={:.0}（拖拽锁24px/抽屉90px 之间死区）",
                                x - apt.start_x,
                                y - apt.start_y
                            ),
                        );
                    }
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
                        crate::report::report("ime", "面板页点按：收键盘不穿透");
                    }
                    self.dirty = true;
                    return;
                }
                // 快捷键行手势：抬手命中发键（Cancelled 不发）
                if self.bar_touch.take().is_some() {
                    // 连发状态随手势收走（Cancelled 同路——后台/抢手势不残留）
                    let repeat = self.bar_repeat.take();
                    // BAR-018 诊断：进得了这个分支 = Started 的 in_bar
                    // 判定活着；hit 落空也会留痕（坐标+inset 三数）
                    crate::report::report(
                        "ime",
                        &format!("快捷键行抬手 ({},{}), inset={}", x, y, self.chrome_inset()),
                    );
                    if phase != TouchPhase::Ended {
                        return;
                    }
                    // 长按连发已交卷（fired>0）= 抬手不再补发——硬键盘同语义
                    // （按住 N 连发，松手不多跳一格）；短点（fired=0/非方向键
                    // 未武装）走下方原命中发键路径
                    if repeat.as_ref().is_some_and(|r| r.fired > 0) {
                        self.dirty = true;
                        return;
                    }
                    let Some(w) = &self.window else { return };
                    let s = w.inner_size();
                    let Some(kd) = crate::keybar::hit(x, y, s.width, s.height, self.cur_bar_h())
                    else {
                        crate::report::report(
                            "ime",
                            &format!(
                                "快捷键行命中落空: 窗 {}x{} inset={}",
                                s.width,
                                s.height,
                                self.chrome_inset()
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
                // 抽屉手势（§五B）：终端区水平快滑 = 面板召唤/推回（方向锁
                // 1.8，纵向滚屏不冲突——decide_swipe 纯函数单源；手势起点
                // 在 press 里，滚屏轨迹在 touch_scroll 里，同一指）
                if phase == TouchPhase::Ended
                    && let Some(p) = press.as_ref()
                    && let Some(dir) = crate::ai_presence::decide_swipe(x - p.x, y - p.y)
                {
                    if let Some(ai) = &self.ai_presence {
                        match dir {
                            crate::ai_presence::SwipeDir::Left => ai.swipe_left(),
                            crate::ai_presence::SwipeDir::Right => ai.swipe_right(),
                        }
                    }
                    let after = self
                        .ai_presence
                        .as_ref()
                        .and_then(|ai| ai.snap(crate::report::boot_ms() as u64).top);
                    crate::report::report("ui", &format!("抽屉手势: {dir:?} → 栈顶 {after:?}"));
                    self.touch_scroll.take();
                    self.dirty = true;
                    return;
                }
                // 静默死点留痕：终端区横向位移手势（≥24px 且横过纵）既没
                // 锁拖拽也没过 90px 抽屉阈——被滚屏/点按分路吞掉的横向
                // 意图在此现形（「配置卡无法收回」案侦查）
                if phase == TouchPhase::Ended
                    && let Some(p) = press.as_ref()
                {
                    let (dx, dy) = (x - p.x, y - p.y);
                    if dx.abs() >= 24.0 && dx.abs() > dy.abs() {
                        crate::report::report(
                            "gest",
                            &format!("终端横向手势无人认领: dx={dx:.0} dy={dy:.0}"),
                        );
                    }
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

    /// JNI 轮询真实键盘高度（100ms 节流）：winit 的 Ime::Enabled/Disabled 在
    /// 本机从未触发（全日志零条），事件驱动是死路，轮询才是活路（BAR-006）。
    /// 值变了才 resize + 上报——resize 会抖动服务器 pty，不能跟着轮询抖。
    /// 节流从 500ms 降到 100ms（BAR-065：输入栏/快捷键行跟键盘开合慢半拍——
    /// 轮询间隔就是感知延迟本身；轮询只在事件循环醒着时跑，100ms 成本可忽略）
    fn poll_ime_inset(&mut self) {
        let now = std::time::Instant::now();
        if let Some(t) = self.last_inset_poll
            && now.duration_since(t) < std::time::Duration::from_millis(crate::insets::IME_POLL_MS)
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
        // BAR-112：窗口死了不记账——挂起态记账不重算 = kb_shift 卡死半屏；
        // 留旧值待回前台同差再判（变更臂必带 apply_window_size 重算）
        let Some(px) = crate::insets::on_inset_poll(px, self.ime_bottom_px, self.window.is_some())
        else {
            return;
        };
        crate::report::report("ime", &format!("键盘 inset 变化: {px}px"));
        self.ime_bottom_px = px;
        if let Some(w) = &self.window {
            let s = w.inner_size();
            self.apply_window_size(s.width, s.height);
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

    /// 快捷键行方向键长按连发（2026-09-19 用户拍板：tmux 里方向键长按
    /// 要能连续移动）：主循环每圈 poll，到点推键码进 ime_queue——与点按
    /// 同一条路（修饰键粘滞/键码翻译下游不变）
    fn check_bar_repeat(&mut self) {
        let now = std::time::Instant::now();
        let Some(code) = self.bar_repeat.as_mut().and_then(|r| r.poll(now)) else {
            return;
        };
        crate::ime_queue::global().push_key_code(code);
        self.dirty = true;
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
                .saturating_sub(self.chrome_inset() + bar_h)
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
        let ime_bottom = self.chrome_inset();
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
        let ime_bottom = self.chrome_inset();
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
            .bar_selection_geometry(&snap, w, h, self.chrome_inset())?;
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
            .bar_selection_geometry(&snap, w, h, self.chrome_inset())?;
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
            // BAR-079 中继：入场代 bump = 覆盖再召唤（坍缩②）/静默挤出（③）
            // ——把该面板的缝重定基到屏外位：目标不动则重播入场，目标已屏外
            // 则瞬移落定。纯机械中继：bump 与否的契约判定全在状态核（A 档钉）
            if let Some(prev) = self.last_ai_snap {
                let now = crate::report::boot_ms() as u64;
                if let Some(size) = self.window.as_ref().map(|w| w.inner_size()) {
                    if snap.ai_epoch != prev.ai_epoch && size.height > 0 {
                        crate::ui::seam::replay_ai_panel_offset_y(-(size.height as f32), now);
                    }
                    if snap.cfg_epoch != prev.cfg_epoch && size.width > 0 {
                        crate::ui::seam::replay_config_panel_offset_x(size.width as f32, now);
                    }
                    if snap.ft_epoch != prev.ft_epoch && size.width > 0 {
                        crate::ui::seam::replay_filetree_panel_offset_x(-(size.width as f32), now);
                    }
                    if snap.pt_epoch != prev.pt_epoch && size.width > 0 {
                        crate::ui::seam::replay_parser_panel_offset_x(size.width as f32, now);
                    }
                }
            }
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
        // 标签栏游标弹簧（宪法 §四）：动画进行中逐圈置脏——弹簧收敛
        // 后 cursor_x == target 自然停脏（零空烧）
        if let Some(bar) = &self.tab_bar {
            let g = bar.lock().unwrap();
            let now = crate::report::boot_ms() as u64;
            if (g.cursor_x(now) - g.cursor_target()).abs() > 0.5 {
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

    /// 初始化渲染后端：GLES present 优先（期 1 第 1 层），失败回退
    /// softbuffer（上下文 + 表面），按窗口尺寸配置
    fn init_gfx(window: &Arc<Window>) -> Gfx {
        if GLES_FIRST {
            match crate::gles_present::GlesPresent::new(window) {
                Ok(g) => {
                    crate::report::report(
                        "boot",
                        &format!("GLES present 后端上线 +{}ms", boot_ms()),
                    );
                    return Gfx::Gles(Box::new(g));
                }
                Err(e) => {
                    crate::report::report("boot", &format!("GLES 初始化失败，回退 softbuffer: {e}"))
                }
            }
        }
        let context = softbuffer::Context::new(window.clone()).expect("创建 softbuffer 上下文失败");
        crate::report::report("boot", "softbuffer 上下文建成");
        let mut surface =
            softbuffer::Surface::new(&context, window.clone()).expect("创建 softbuffer 表面失败");
        crate::report::report("boot", &format!("softbuffer 表面建成 +{}ms", boot_ms()));
        let size = window.inner_size();
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            surface.resize(w, h).expect("surface resize 失败");
        }
        Gfx::Soft {
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

        // 设置读盘（设置页 v1）：servers.json/terminal.json → App 字段 +
        // ws 连接插件的默认 ConnConfig。默认会话指向的服务器优先，
        // 否则第一条；wsUrl 空则按 tunnel.localPort 拼回环地址；
        // 无条目 = ConnConfig::default()（8021 现状锚，行为零变化）
        let (servers, term_cfg) = load_settings(self.android_app.as_ref());
        // 索引先行（借还瞬清，servers 之后整体 move 进 App 字段不打架）
        let default_idx = match &term_cfg.default_session {
            crate::settings::DefaultSession::Server(id) => servers
                .iter()
                .position(|s| &s.id == id || &s.name == id)
                .or_else(|| {
                    crate::report::report_sync(
                        "term",
                        &format!("defaultSession 指向的服务器「{id}」不存在，回退第一条"),
                    );
                    (!servers.is_empty()).then_some(0)
                }),
            crate::settings::DefaultSession::Local => (!servers.is_empty()).then_some(0),
        };
        let prefer_remote = default_idx.is_some()
            && matches!(
                term_cfg.default_session,
                crate::settings::DefaultSession::Server(_)
            );
        let conn_cfg = match default_idx {
            Some(i) => {
                let s = &servers[i];
                ConnConfig {
                    url: if s.ws_url.is_empty() {
                        format!("ws://127.0.0.1:{}/ws", s.tunnel.local_port)
                    } else {
                        s.ws_url.clone()
                    },
                    command: s.command.clone(),
                }
            }
            None => ConnConfig::default(),
        };
        self.switch_hotkey_bytes = term_cfg.switch_hotkey.bytes();
        // 解析页对象轴启动同步（两轴宪法 §一）：默认会话是谁，启动相
        // 就是谁——之后 Ctrl-] 切换在 switch_session 里随行同步
        crate::endpoint::sync(crate::endpoint::of_default_session(
            &term_cfg.default_session,
        ));
        self.terminal_cfg = term_cfg;
        self.settings_servers = servers;
        // 解析页 tmux 插件：远程连接配置缓存（ws url + 启动命令）+ 本端
        // 附着会话名（启动命令提取；attach 切换后更新）。无服务器条目 =
        // None——插件显示占位（执行通道无处连）
        self.remote_conn_cfg = default_idx.map(|_| conn_cfg.clone());
        self.remote_attached = conn_cfg
            .command
            .as_deref()
            .and_then(crate::tmux_ctl::session_name_of);

        // L3 内置 ssh 正连隧道（2026-09-19 用户拍板：运行时通道收归 na
        // 自持，取代 Termux 外挂 ssh -L；让位/接管语义与看门狗在
        // tunnel.rs）。有服务器条目 + L3 prefix 就绪才起；缺件不静默
        let tunnel_srv = default_idx
            .and_then(|i| self.settings_servers.get(i))
            .cloned();
        self.tunnel_snap = tunnel_srv.clone().and_then(|srv| {
            let prefix = crate::local_pty::android_prefix();
            if !crate::bootstrap::prefix_ready(&prefix) {
                crate::report::report("tunnel", "L3 prefix 未装，隧道不启动（装好 L3 后重开 app）");
                return None;
            }
            Some(crate::tunnel::start(prefix, srv.clone()))
        });

        // na-server 主体拉起链（2026-09-20，docs/active/na-server.md §二）：
        // 后端 = na-server 才起——看门狗等隧道可用后经 SSH exec 幂等 ensure
        // （服务器上活则接管/死则建起拉起）。幂等，设置重载重复调安全
        if let Some(srv) = &tunnel_srv
            && srv.backend == crate::settings::Backend::NaServer
        {
            let prefix = crate::local_pty::android_prefix();
            if crate::bootstrap::prefix_ready(&prefix) {
                crate::na_server_sup::start(prefix, srv.clone());
            }
        }

        // 服务卡数据面（2026-09-20，docs/active/na-server.md §四）：
        // 后端 + 隧道本地口喂给 health 轮询器（幂等；后端翻相自清
        // 数据）。无服务器条目 = Kfmv4 托管态
        crate::svc_health::configure(
            tunnel_srv.as_ref().map(|s| s.backend).unwrap_or_default(),
            tunnel_srv
                .as_ref()
                .map(|s| s.tunnel.local_port)
                .unwrap_or(crate::tunnel::NA_SERVER_PORT),
        );

        // 插件基座：终端模拟器 + 连接 provider（边界手术第一/二刀）——
        // 「用哪个终端芯、连哪、怎么连」都不归主循环；工厂是服务，实例归调用方。
        // 瞬时返回契约预算 50ms 是 harness 政策(G5 归层:cordis-na 默认关,
        // 这里显式开启,规格书 §4.3)
        let base = Base::new(vec![
            PluginEntry {
                id: crate::plugins::conn_provider_ws::PLUGIN_NAME,
                disabled: false,
                config: Some(Box::new(move || {
                    Arc::new(conn_cfg.clone()) as Arc<dyn std::any::Any + Send + Sync>
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

        // 配置卡标签栏（主题宪法 §四，2026-09-12）：池名表 v1b 两池——
        // 「系统管理」+「组件池」（九修 2026-09-13：组件库实机花名册，
        // 宪法 §五 目录语义 7；表是 Vec 天然可扩——API 池随后追加）；
        // 共享句柄注册给 gate 值守倒帧（D9 同源）。初始视口 720 占位，
        // draw_frame 每帧按真实屏宽 set_viewport_w 纠
        {
            let bar = std::sync::Arc::new(std::sync::Mutex::new(crate::ui::tab_bar::TabBar::new(
                &["系统管理", "组件池"],
                720,
            )));
            crate::ui::tab_bar::register_tab_bar(bar.clone());
            // 十一修（宪法 §四 每标签独立随机双色）：presence 召唤配置卡时
            // 逐标签重随色列喂进标签栏，accent 取选中项——绑定同一份 Arc
            if let Some(ai) = &self.ai_presence {
                ai.bind_tab_bar(bar.clone());
            }
            self.tab_bar = Some(bar);
        }

        // 配置卡双池（主题宪法 §五，2026-09-12 骨架）：上池内容高骨架期
        // 恒 0 = 空占位（A2）；共享句柄注册给 gate 值守倒帧（D9 同源）。
        // 初始视口 720x1280 占位，draw_frame 每帧按真实屏尺寸 set_viewport 纠
        {
            let pool = std::sync::Arc::new(std::sync::Mutex::new(
                crate::ui::dual_pool::DualPool::new(720, 1280),
            ));
            crate::ui::dual_pool::register_dual_pool(pool.clone());
            self.dual_pool = Some(pool);
        }

        // 配置页三层目录状态核（宪法 §五 目录语义，2026-09-13）：
        // 行表/字段由 rebuild_cfg_rows 按 settings 数据重建；共享句柄
        // 注册给 gate 值守倒帧（D9 同源，与标签栏/双池同规）
        {
            let page =
                std::sync::Arc::new(std::sync::Mutex::new(crate::ui::cfg_page::CfgPage::new()));
            crate::ui::cfg_page::register_cfg_page(page.clone());
            self.cfg_page = Some(page);
            self.rebuild_cfg_rows();
        }

        // 解析页 tmux 插件状态核（2026-09-19）：共享句柄注册（gate 值守
        // 倒帧同源）；附着名 = 当前对象那份账（启动命令提取喂入，attach
        // 后由壳更新；两轴第 6 步②起本地相有自己的附着账）
        {
            let page = std::sync::Arc::new(std::sync::Mutex::new(
                crate::ui::parser_page::ParserPage::new(),
            ));
            page.lock().unwrap().set_attached(self.cur_attached());
            crate::ui::parser_page::register_parser_page(page.clone());
            self.parser_page = Some(page);
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
                // 发送即跳 AI 全屏页（2026-09-04 用户拍板：输入栏发完
                // 直接转进面板看生成，不许停在终端页等手动点球）。
                // 幂等——已在 AI 页 = 无效果
                ai2.tap_overlay();
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
                // 默认会话（设置页 v1 terminal.json defaultSession）：
                // Server(id 解析成功) → 远程为活跃槽、本地待机；
                // 否则维持现状锚（本地活跃，远程待机）。prefer_remote
                // 在设置读盘段已算好（解析成功才 true）
                let (active_tx, active_name, standby_tx, standby_name) = if prefer_remote {
                    (r.outbound, "remote", l.outbound, "local")
                } else {
                    (l.outbound, "local", r.outbound, "remote")
                };
                let mut router = crate::session_router::SessionRouter::new(active_tx, active_name);
                if let Err(e) = router.add_standby(standby_tx, standby_name) {
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
    /// 可用区域 = 窗口
    /// - 四周边距（卡片壳几何：横 49 / 纵 31，2026-09-11）
    /// - 真实软键盘 inset（BAR-006，JNI 轮询，insets.rs）
    /// - 快捷键行高（BAR-017，Rust 自绘常驻让位）
    ///
    /// 顶带恒定（margin_top：壳环靠泊接管防切，不跟格高走，2026-09-11）
    fn apply_window_size(&mut self, w: u32, h: u32) {
        // BAR-108：末次真实屏尺寸记账——退后台弃窗后触摸命中几何靠它兜底
        self.last_win_px = (w, h);
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
                + crate::keybar::HEIGHT_PX
                + crate::input_bar::HEIGHT_PX, // 期 0 组件三：输入栏常驻让位
                                               // （textarea 覆盖式悬浮：网格只让单行带高，栏长高向上浮盖终端
                                               // 底部行——不触发 resize→SIGWINCH→重绘洪峰链，nz case-002 教训）
                                               // 2026-09-18「键盘弹起改视口平移」：键盘 inset 同此理移出
                                               // grid 账——行数纹丝不动，遮挡靠视口平移（下方 sync_kb_shift）
        );
        let (cols, rows) = termview::grid_dims(usable_w, usable_h, cw, ch);
        // resize 会抖动服务器 pty，尺寸没变不重发（键盘弹收走到这里时
        // cols/rows 恒定——tmux 侧零 SIGWINCH 零重排的铁证契约）；
        // 会话切换/重连的补发走 last_grid 专径（3050/3141 行族），不受影响
        if (cols, rows) != self.last_grid {
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
        }
        // 键盘遮挡带 → 视口上移（追光标钳制；看历史/光标可见时恒 0）
        let occlude = self.ime_bottom_px + crate::keybar::HEIGHT_PX + crate::input_bar::HEIGHT_PX;
        if term.lock().unwrap().sync_kb_shift(h, occlude) {
            let shift = term.lock().unwrap().kb_shift();
            crate::report::report("ime", &format!("视口平移 {shift} 行"));
        }
        self.dirty = true;
    }

    /// 配置页行表/上池/下拉重建（宪法 §五 目录语义二版，设置页 §2.3）：
    /// 下池 = 子目录（系统管理大类目前仅一行「系统管理」——服务器列表
    /// 不进下池，用户拍板）；上池 = 服务器配置表单（默认服务器下拉行 +
    /// 服务器切换行 + 下拉选中服务器的字段框行）；下拉选项 = 本地终端 +
    /// 配置页内容重建（九修 tab 维分流）：tab 0 系统管理 = 服务器配置
    /// 行表（服务器池选中位从 terminal_cfg.default_session 解析）；
    /// tab 1 组件池 = comp_registry 常量表直读（唯一信息源纪律——大类
    /// = 下池行、类内组件 = 上池行，宪法 §五 目录语义 7）。数据变更/
    /// 换选/切页后必调（set_* 内部判等，没变不空涨代际）
    fn rebuild_cfg_rows(&mut self) {
        let Some(page) = &self.cfg_page else { return };
        if page.lock().unwrap().tab() == 1 {
            use crate::ui::comp_registry as cr;
            let focus = page.lock().unwrap().focus();
            let rows: Vec<crate::ui::cfg_page::RowView> = cr::CATEGORIES
                .iter()
                .map(|c| crate::ui::cfg_page::RowView {
                    title: (*c).into(),
                    meta: format!("{} 个组件", cr::count_of(c)),
                })
                .collect();
            let cat = cr::CATEGORIES
                .get(focus)
                .copied()
                .unwrap_or(cr::CATEGORIES[0]);
            let upper: Vec<crate::ui::cfg_page::UpperRow> = cr::entries_of(cat)
                .iter()
                .map(|&i| {
                    let e = &cr::COMPONENTS[i];
                    crate::ui::cfg_page::UpperRow {
                        label: e.name.into(),
                        value: e.status.label().into(),
                        is_dropdown: false,
                    }
                })
                .collect();
            let mut p = page.lock().unwrap();
            p.set_rows(rows);
            p.set_upper(upper);
            return;
        }
        let rows = vec![crate::ui::cfg_page::RowView {
            title: "系统管理".into(),
            meta: "服务器配置".into(),
        }];
        // 下拉选项 = 本地终端 + 服务器池；选中位 = 当前默认会话
        let mut options = vec!["本地终端".to_string()];
        for s in &self.settings_servers {
            options.push(if s.name.is_empty() {
                s.id.clone()
            } else {
                s.name.clone()
            });
        }
        let sel = match &self.terminal_cfg.default_session {
            crate::settings::DefaultSession::Local => 0,
            crate::settings::DefaultSession::Server(id) => self
                .settings_servers
                .iter()
                .position(|s| &s.id == id || &s.name == id)
                .map_or(0, |i| i + 1),
        };
        // 上池字段框行：默认服务器下拉行 + 服务器切换行 + 选中服务器字段
        let mut upper = vec![
            crate::ui::cfg_page::UpperRow {
                label: "默认服务器".into(),
                value: options.get(sel).cloned().unwrap_or_default(),
                is_dropdown: true,
            },
            crate::ui::cfg_page::UpperRow {
                label: "服务器切换".into(),
                value: self.terminal_cfg.switch_hotkey.display(),
                is_dropdown: false,
            },
        ];
        if sel > 0
            && let Some(s) = self.settings_servers.get(sel - 1)
        {
            let fields: [(&str, String); 8] = [
                (
                    "名称",
                    if s.name.is_empty() {
                        s.id.clone()
                    } else {
                        s.name.clone()
                    },
                ),
                ("服务器 IP", s.ssh.host.clone()),
                ("端口", s.ssh.port.to_string()),
                ("用户", s.ssh.user.clone()),
                (
                    "密钥地址",
                    if s.ssh.key_path.is_empty() {
                        "（未配）".into()
                    } else {
                        s.ssh.key_path.clone()
                    },
                ),
                (
                    "密码",
                    if s.ssh.password.is_empty() {
                        "（空 = 密钥登录）".into()
                    } else {
                        "已配置".into()
                    },
                ),
                (
                    "wsUrl",
                    if s.ws_url.is_empty() {
                        format!("ws://127.0.0.1:{}/ws", s.tunnel.local_port)
                    } else {
                        s.ws_url.clone()
                    },
                ),
                (
                    "切换快捷键",
                    s.hotkey.as_ref().map_or("未绑定".into(), |h| h.display()),
                ),
            ];
            for (label, value) in fields {
                upper.push(crate::ui::cfg_page::UpperRow {
                    label: label.into(),
                    value,
                    is_dropdown: false,
                });
            }
        }
        let mut p = page.lock().unwrap();
        p.set_rows(rows);
        p.set_options(options, sel);
        p.set_upper(upper);
    }

    /// 下拉换选 = 默认服务器变更（设置页 §2.3 二版）：terminal.json
    /// defaultSession 写盘（冷启动生效）+ 内存同步 + 上池重建。
    /// 写盘失败 = 上报不炸（配置文件纪律：坏了回退，不许炸终端）
    fn apply_default_server_pick(&mut self) {
        let Some(page) = &self.cfg_page else { return };
        let sel = page.lock().unwrap().option_sel();
        self.terminal_cfg.default_session = if sel == 0 {
            crate::settings::DefaultSession::Local
        } else {
            self.settings_servers
                .get(sel - 1)
                .map_or(crate::settings::DefaultSession::Local, |s| {
                    crate::settings::DefaultSession::Server(s.id.clone())
                })
        };
        let ds_display = match &self.terminal_cfg.default_session {
            crate::settings::DefaultSession::Local => "本地终端".to_string(),
            crate::settings::DefaultSession::Server(id) => id.clone(),
        };
        if let Some(dir) = self
            .android_app
            .as_ref()
            .and_then(|a| a.internal_data_path())
        {
            let path = dir.join("settings").join("terminal.json");
            let json = crate::settings::terminal_to_json(&self.terminal_cfg);
            if let Err(e) = std::fs::write(&path, json) {
                crate::report::report("term", &format!("terminal.json 写盘失败: {e}"));
            }
        }
        crate::report::report("ui", &format!("默认服务器换选→{ds_display}（已落盘）"));
        self.rebuild_cfg_rows();
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
            let hk = self.terminal_cfg.switch_hotkey.display();
            let banner =
                format!("\r\n\x1b[36m[kfm-na → {name_s} 会话（{hk} 切回 {name_a}）]\x1b[0m\r\n");
            t.lock().unwrap().feed(banner.as_bytes());
        }
        self.session_over = self.health(name_s).dead;
        crate::report::report("term", &format!("会话切换: {name_a} → {name_s}"));
        // 解析页对象轴跟随中央终端（两轴宪法 §一：中央连着谁就解析谁）——
        // 翻相 epoch+1 进涂装 sig 自动重烘；同名不抖
        crate::endpoint::sync(crate::endpoint::of_session_name(name_s));
        // 附着牌随行：对象换了附着名就是另一份账（状态核同源，涂装见
        // epoch 重烘时自然画新牌）
        if let Some(p) = &self.parser_page {
            p.lock().unwrap().set_attached(self.cur_attached());
        }
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

    /// 当前对象那份附着账（两轴第 6 步②：服务器/本地各一本，对象轴
    /// 随行切换后页牌/重排/attach 判定都吃这份——不许各消费点自译）
    fn cur_attached(&self) -> Option<String> {
        match crate::endpoint::current() {
            crate::endpoint::EndpointKind::Server => self.remote_attached.clone(),
            crate::endpoint::EndpointKind::Local => self.local_attached.clone(),
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

    // ---- 解析页 tmux 插件（2026-09-19 用户立项：窗口管理器 + 重排钮）----

    /// 命中动作分发（Ended 臂唯一入口；snap 是点按当时的快照——几何与
    /// 行表同一份，动作不许拿新锁里的「可能已经刷新过」的行表对号入座）
    fn parser_dispatch(
        &mut self,
        snap: crate::ui::parser_page::ParserPageSnap,
        hit: crate::ui::parser_page::Hit,
        mode: crate::ui::parser_page::Mode,
    ) {
        use crate::ui::parser_page as pp;
        match hit {
            pp::Hit::Session(i) => {
                if let Some(name) = snap.sessions.get(i).map(|s| s.name.clone()) {
                    crate::report::report("ui", &format!("tmux 插件点行: attach {name}"));
                    self.parser_attach(name);
                }
            }
            pp::Hit::Kill(i) => {
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().begin_confirm(i);
                    crate::report::report("ui", &format!("tmux 插件点×: 确认跳框 {i}"));
                }
            }
            pp::Hit::ModalOk => {
                let target = self
                    .parser_page
                    .as_ref()
                    .and_then(|p| p.lock().unwrap().confirm_target());
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().cancel_confirm();
                }
                if let Some(name) = target
                    && self.endpoint_exec_ok()
                {
                    crate::report::report("ui", &format!("tmux 插件: 确认关闭 {name}"));
                    self.parser_exec = Some((
                        ParserExec::Kill,
                        self.endpoint_exec(crate::tmux_ctl::cmd_kill(&name)),
                    ));
                }
            }
            pp::Hit::ModalCancel | pp::Hit::ModalDismiss => {
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().cancel_confirm();
                }
                crate::report::report("ui", "tmux 插件: 关闭取消");
            }
            pp::Hit::Button(i) => match pp::button_action(mode, i) {
                Some(pp::Action::Reflow) => self.parser_reflow(),
                Some(pp::Action::New) => {
                    if let Some(p) = &self.parser_page {
                        p.lock().unwrap().begin_naming();
                    }
                    if let Some(w) = &self.window {
                        w.set_ime_allowed(true);
                    }
                    if let Some(insets) = &self.ime_insets {
                        insets.force_show();
                    }
                    crate::report::report("ui", "tmux 插件: 命名态开（弹键盘）");
                }
                Some(pp::Action::NamingOk) => {
                    let raw = self
                        .parser_page
                        .as_ref()
                        .and_then(|p| p.lock().unwrap().naming_take());
                    self.parser_ime_off();
                    self.parser_new_submit(raw);
                }
                Some(pp::Action::NamingCancel) => {
                    if let Some(p) = &self.parser_page {
                        p.lock().unwrap().cancel_naming();
                    }
                    self.parser_ime_off();
                    crate::report::report("ui", "tmux 插件: 命名取消");
                }
                None => {}
            },
        }
        self.dirty = true;
    }

    fn parser_ime_off(&mut self) {
        if let Some(w) = &self.window {
            w.set_ime_allowed(false);
        }
        if let Some(insets) = &self.ime_insets {
            insets.force_hide();
        }
    }

    /// tmux 卡 exec 通道可用性（两轴契约第 4 步：server 相原样迁入——
    /// 通道选择唯一源，裁决纯函数 endpoint::plan_exec 钉着；第 6 步本地
    /// 相 exec 腿接线 = LocalPty 也算可用）。false = 服务器相没配置；
    /// 各调用点照旧自有报错语义（静默或挂错误文案），本层不替它们措辞
    fn endpoint_exec_ok(&self) -> bool {
        use crate::endpoint::{ExecPlan, plan_exec};
        matches!(
            plan_exec(
                crate::endpoint::current(),
                self.remote_conn_cfg.as_ref().map(|c| c.url.as_str()),
            ),
            ExecPlan::Ws(_) | ExecPlan::LocalPty
        )
    }

    /// tmux 卡 exec 通道唯一入口：调用前必须先 endpoint_exec_ok() 裁决
    /// （在途闸/报错语义各调用点自理），ok 后本入口必出通道——
    /// expect 的 panic = 装配错误显形，不许静默吞
    fn endpoint_exec(&self, cmd: String) -> std::sync::mpsc::Receiver<Result<String, String>> {
        use crate::endpoint::{ExecPlan, plan_exec};
        match plan_exec(
            crate::endpoint::current(),
            self.remote_conn_cfg.as_ref().map(|c| c.url.as_str()),
        ) {
            ExecPlan::Ws(url) => crate::tmux_exec::exec(url.to_string(), cmd),
            ExecPlan::LocalPty => crate::local_pty::local_exec(cmd),
            ExecPlan::NoServer => {
                panic!("exec 通道不可用——调用点漏过 endpoint_exec_ok 裁决")
            }
        }
    }

    /// 列会话（插件数据唯一来源 = 服务器真表，nz P5 同规）：在途不叠
    fn parser_refresh(&mut self) {
        if !self.endpoint_exec_ok() {
            if let Some(p) = &self.parser_page {
                p.lock().unwrap().set_error("无远程服务器配置".into());
            }
            return;
        }
        if self.parser_exec.is_some() {
            return;
        }
        if let Some(p) = &self.parser_page {
            p.lock().unwrap().set_loading();
        }
        self.parser_exec = Some((
            ParserExec::List,
            self.endpoint_exec(crate::tmux_ctl::cmd_list()),
        ));
        self.dirty = true;
    }

    /// 程序化收起解析页（2026-09-19 用户拍板：点会话行 attach / 点重排
    /// 后回终端主页）。只弹栈 + 脏帧——target 只问栈（stage::
    /// panel_target_and_draw，BAR-084 单源），缝采样器见目标 0→+w 自动
    /// 缓动播退场、活性期照画，无需 replay 踢（replay 只为「目标不变但
    /// 历史变了」的覆盖再召唤场景重定基）
    fn parser_dismiss(&mut self) {
        use crate::ai_presence::Panel;
        if let Some(ai) = &self.ai_presence
            && ai.dismiss_top(Panel::Parser)
        {
            crate::report::report("ui", "tmux 插件: 程序化收起解析页");
            self.dirty = true;
        }
    }

    /// 重排：窗口尺寸钉到 na 当前网格（manual 即生效；largest/latest
    /// 下 tmux 自动翻 manual——2026-09-19 服务器实证）。附着账吃当前
    /// 对象那份（第 6 步②：本地相重排的是手机本地 tmux）
    fn parser_reflow(&mut self) {
        let Some(sess) = self.cur_attached() else {
            if let Some(p) = &self.parser_page {
                p.lock().unwrap().set_error("重排需要本端已附着会话".into());
            }
            self.dirty = true;
            return;
        };
        if !self.endpoint_exec_ok() {
            if let Some(p) = &self.parser_page {
                p.lock().unwrap().set_error("重排需要本端已附着会话".into());
            }
            self.dirty = true;
            return;
        }
        if self.parser_exec.is_some() {
            return;
        }
        let (cols, rows) = self.last_grid;
        crate::report::report("ui", &format!("tmux 插件: 重排 {sess} → {cols}x{rows}"));
        self.parser_exec = Some((
            ParserExec::Reflow,
            self.endpoint_exec(crate::tmux_ctl::cmd_reflow(&sess, cols, rows)),
        ));
        self.dirty = true;
        // 重排已派发 = 回终端主页看效果（用户拍板：点重排后收起解析页）
        self.parser_dismiss();
    }

    /// 新建提交：None/空 = tmux 自动编号；非法字符 = 报错不执行
    fn parser_new_submit(&mut self, raw: Option<String>) {
        let raw = raw.unwrap_or_default();
        let name = if raw.trim().is_empty() {
            None
        } else {
            match crate::tmux_ctl::sanitize_name(&raw) {
                Some(n) => Some(n),
                None => {
                    if let Some(p) = &self.parser_page {
                        p.lock()
                            .unwrap()
                            .set_error("名字含非法字符（' | : ; 控制符）".into());
                    }
                    self.dirty = true;
                    return;
                }
            }
        };
        if !self.endpoint_exec_ok() {
            return;
        }
        if self.parser_exec.is_some() {
            return;
        }
        crate::report::report("ui", &format!("tmux 插件: 新建会话 {name:?}"));
        self.parser_exec = Some((
            ParserExec::New,
            self.endpoint_exec(crate::tmux_ctl::cmd_new(name.as_deref())),
        ));
        self.dirty = true;
    }

    /// attach 切换（nz P7 嵌套禁止的 na 落地：不重开客户端内 attach，
    /// 而是关掉当前会话、按新命令重孵——复用断线重连同一条工序）：
    /// P1 同规——已附着同名 = 零动作；非本相活跃 = 报错引导
    /// 重孵公共段（attach/脱离两臂同款工序，第 6 步②起按名分工厂）：
    /// 关旧 → 工厂按 cfg 起新 → router 换心 + 泵换入向通道 → 补
    /// Resize。失败返回原因，页面上挂错误与 report 归调用方
    fn respawn_named_with(
        &mut self,
        name: &'static str,
        new_cfg: &ConnConfig,
    ) -> Result<(), String> {
        if let Some(r) = self.router_handle() {
            r.lock().unwrap().send(TermCmd::Close);
        }
        let handle = match name {
            "local" => self
                .base
                .as_ref()
                .and_then(|b| b.ctx().get::<crate::local_pty::LocalPtyFactory>().ok())
                .map(|f| f.spawn(new_cfg)),
            _ => self
                .base
                .as_ref()
                .and_then(|b| b.ctx().get::<dyn TermFactory>().ok())
                .map(|f| f.spawn(new_cfg)),
        };
        let Some(h) = handle else {
            return Err("连接工厂不可用".into());
        };
        {
            let health = self.health_mut(name);
            health.dead = false;
            health.connecting = true;
        }
        if let Some(r) = self.router_handle() {
            r.lock().unwrap().replace_active(h.outbound);
        }
        crate::gate::pump_register(name, h.events);
        self.session_over = false;
        let (cols, rows) = self.last_grid;
        if let Some(r) = self.router_handle() {
            r.lock().unwrap().send(TermCmd::Resize { cols, rows });
        }
        Ok(())
    }

    /// attach 入口的对象轴分流（两轴第 6 步②：服务器相走 ws 重孵，
    /// 本地相走本地 PTY 重孵——两臂同语义同工序，通道不同）
    fn parser_attach(&mut self, name: String) {
        match crate::endpoint::current() {
            crate::endpoint::EndpointKind::Server => self.parser_attach_server(name),
            crate::endpoint::EndpointKind::Local => self.parser_attach_local(name),
        }
    }

    /// 本地相 attach/脱离（与服务器臂逐点对称：点行 = 本地 PTY 重孵
    /// 带 tmux attach 命令；点已附着 = 裸 shell 重孵脱离，BAR-116
    /// 同规）。差异只在：工厂 = LocalPtyFactory、活跃槽守卫 = local、
    /// 无「服务器配置缺失」臂（本地 exec 不需要配置）
    fn parser_attach_local(&mut self, name: String) {
        if self.local_attached.as_deref() == Some(name.as_str()) {
            crate::report::report(
                "ui",
                &format!("tmux 插件: {name} 已附着 = 脱离回本地 shell"),
            );
            if self
                .router_handle()
                .map(|r| r.lock().unwrap().active_name())
                != Some("local")
            {
                if let Some(p) = &self.parser_page {
                    p.lock()
                        .unwrap()
                        .set_error("先切到本地终端再脱离会话".into());
                }
                crate::report::report("ui", "tmux 插件: 脱离被拒——活跃非本地");
                self.dirty = true;
                return;
            }
            let shell_cfg = ConnConfig {
                url: String::new(),
                command: None, // 裸 shell = 交互本地方案（L3 bash）
            };
            if let Err(e) = self.respawn_named_with("local", &shell_cfg) {
                crate::report::report_sync("term", &format!("tmux 插件脱离失败: {e}"));
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().set_error(format!("脱离失败: {e}"));
                }
                self.dirty = true;
                return;
            }
            self.local_attached = None;
            if let Some(p) = &self.parser_page {
                p.lock().unwrap().set_attached(None);
            }
            if let Some(t) = self.term_handle() {
                // 裸 shell 不重画——旧 tmux 残帧必须先清（BAR-116 同规）
                let banner = format!(
                    "\x1b[2J\x1b[H\x1b[36m[kfm-na: 已脱离 tmux 会话 {name}，回到本地 shell]\x1b[0m\r\n"
                );
                t.lock().unwrap().feed(banner.as_bytes());
                t.lock().unwrap().scroll_to_bottom();
            }
            self.dirty = true;
            self.parser_dismiss();
            return;
        }
        let Some(p) = self.parser_page.clone() else {
            return;
        };
        if self
            .router_handle()
            .map(|r| r.lock().unwrap().active_name())
            != Some("local")
        {
            p.lock()
                .unwrap()
                .set_error("先切到本地终端再切换会话".into());
            crate::report::report("ui", "tmux 插件: attach 被拒——活跃非本地");
            self.dirty = true;
            return;
        }
        // 关旧 → 新命令重孵（公共段；脱离臂同路）。url 本地路径忽略
        let new_cfg = ConnConfig {
            url: String::new(),
            command: Some(crate::tmux_ctl::cmd_attach(&name)),
        };
        if let Err(e) = self.respawn_named_with("local", &new_cfg) {
            crate::report::report_sync("term", &format!("tmux 插件 attach 失败: {e}"));
            p.lock().unwrap().set_error(format!("attach 失败: {e}"));
            self.dirty = true;
            return;
        }
        self.local_attached = Some(name.clone());
        p.lock().unwrap().set_attached(Some(name.clone()));
        if let Some(t) = self.term_handle() {
            let banner = format!("\r\n\x1b[36m[kfm-na: 切换到 tmux 会话 {name}]\x1b[0m\r\n");
            t.lock().unwrap().feed(banner.as_bytes());
            t.lock().unwrap().scroll_to_bottom();
        }
        crate::report::report("ui", &format!("tmux 插件: attach {name} 本地重孵已发"));
        self.dirty = true;
        self.parser_refresh();
        // attach 重孵已发 = 回终端主页接新会话（服务器臂同规）
        self.parser_dismiss();
    }

    fn parser_attach_server(&mut self, name: String) {
        if self.remote_attached.as_deref() == Some(name.as_str()) {
            // 点已附着会话 = 脱离回服务器命令行（2026-09-19 用户二拍，nz
            // 同款语义：点聚焦标签 = 回主内容）。初版只收页不脱离——附着
            // 没动，终端里还是 tmux 会话，用户实报「没回到服务器的终端」
            // +「聚焦框该灭」。正身 = 裸 shell 重孵远程 + 附着牌清空
            // （聚焦框灭）+ 收页；tmux 会话在服务器侧毫发无损，再点任意
            // 框重新进入（BAR-116）
            crate::report::report(
                "ui",
                &format!("tmux 插件: {name} 已附着 = 脱离回服务器 shell"),
            );
            if self
                .router_handle()
                .map(|r| r.lock().unwrap().active_name())
                != Some("remote")
            {
                if let Some(p) = &self.parser_page {
                    p.lock()
                        .unwrap()
                        .set_error("先切到远程终端再脱离会话".into());
                }
                crate::report::report("ui", "tmux 插件: 脱离被拒——活跃非远程");
                self.dirty = true;
                return;
            }
            let Some(cfg) = self.remote_conn_cfg.clone() else {
                crate::report::report("ui", "tmux 插件: 脱离无远程服务器配置");
                return;
            };
            // command None = 交互 shell（kfmv4 terminal-pty：command 空则
            // 起默认 shell）——脱离 tmux 回服务器命令行的协议面
            let shell_cfg = ConnConfig {
                url: cfg.url.clone(),
                command: None,
            };
            if let Err(e) = self.respawn_named_with("remote", &shell_cfg) {
                crate::report::report_sync("term", &format!("tmux 插件脱离失败: {e}"));
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().set_error(format!("脱离失败: {e}"));
                }
                self.dirty = true;
                return;
            }
            self.remote_attached = None;
            if let Some(p) = &self.parser_page {
                p.lock().unwrap().set_attached(None);
            }
            if let Some(t) = self.term_handle() {
                // 裸 shell 不会像 tmux attach 那样全屏重画——旧网格残留
                // 必须先清，否则旧 tmux 残帧混进新 shell（redroid 实拍：
                // 输出「DETACH_OK_42」拼上旧残字「e lines…」，BAR-116）
                let banner = format!(
                    "\x1b[2J\x1b[H\x1b[36m[kfm-na: 已脱离 tmux 会话 {name}，回到服务器 shell]\x1b[0m\r\n"
                );
                t.lock().unwrap().feed(banner.as_bytes());
                t.lock().unwrap().scroll_to_bottom();
            }
            self.dirty = true;
            self.parser_dismiss();
            return;
        }
        let Some(p) = self.parser_page.clone() else {
            return;
        };
        if self
            .router_handle()
            .map(|r| r.lock().unwrap().active_name())
            != Some("remote")
        {
            p.lock()
                .unwrap()
                .set_error("先切到远程终端再切换会话".into());
            crate::report::report("ui", "tmux 插件: attach 被拒——活跃非远程");
            self.dirty = true;
            return;
        }
        let Some(cfg) = self.remote_conn_cfg.clone() else {
            // 静默死点留痕（排障手册纪律：点行零响应必须有声）——
            // 无服务器条目时 attach 不能凭空消失
            crate::report::report("ui", "tmux 插件: attach 无远程服务器配置");
            p.lock()
                .unwrap()
                .set_error("无远程服务器配置（设置页补服务器条目）".into());
            self.dirty = true;
            return;
        };
        // 关旧 → 新命令重孵（公共段；脱离臂同路）
        let new_cfg = ConnConfig {
            url: cfg.url.clone(),
            command: Some(crate::tmux_ctl::cmd_attach(&name)),
        };
        if let Err(e) = self.respawn_named_with("remote", &new_cfg) {
            crate::report::report_sync("term", &format!("tmux 插件 attach 失败: {e}"));
            p.lock().unwrap().set_error(format!("attach 失败: {e}"));
            self.dirty = true;
            return;
        }
        self.remote_attached = Some(name.clone());
        p.lock().unwrap().set_attached(Some(name.clone()));
        if let Some(t) = self.term_handle() {
            let banner = format!("\r\n\x1b[36m[kfm-na: 切换到 tmux 会话 {name}]\x1b[0m\r\n");
            t.lock().unwrap().feed(banner.as_bytes());
            t.lock().unwrap().scroll_to_bottom();
        }
        crate::report::report("ui", &format!("tmux 插件: attach {name} 重孵已发"));
        self.dirty = true;
        // 新会话列表重排口径可能变（active 窗换了）——顺手刷新
        self.parser_refresh();
        // attach 重孵已发 = 回终端主页接新会话（用户拍板：点行后收起解析页；
        // 早期报错臂——非远程活跃/工厂不可用——不收，错误得留在页面上可见）
        self.parser_dismiss();
    }

    /// 执行排水（about_to_wait 每圈）：Ok 后按种类善后
    fn parser_exec_done(&mut self, kind: ParserExec, res: Result<String, String>) {
        match (kind, res) {
            (ParserExec::List, Ok(out)) => {
                let ss = crate::tmux_ctl::parse_session_list(&out);
                crate::report::report("ui", &format!("tmux 插件: 会话表 {} 条", ss.len()));
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().set_sessions(ss);
                    // 行表变了滚动上限跟着变（会话变少 max 缩）——
                    // 拿当下几何钳回，脏 scroll 不残留（页面滚动同规：
                    // 链底账随卡高变）
                    if let Some((sw, sh)) = self.screen_px() {
                        let (smax, pmax) = {
                            let pg = p.lock().unwrap();
                            let snap = pg.snap();
                            let lay = crate::ui::parser_page::layout_vp(
                                sw,
                                sh,
                                self.cur_bar_h()
                                    + crate::ui::parser_chain::reserved_below_tmux(
                                        crate::ui::svc_card::current().lines.len(),
                                    ),
                                snap.sessions.len(),
                                crate::ui::svc_card::current().lines.len(),
                                crate::ui::parser_page::Mode::Normal,
                                snap.scroll,
                                snap.page_scroll,
                                crate::ui::parser_page::visible_bottom(
                                    sh,
                                    self.chrome_inset() + self.cur_bar_h(),
                                ),
                            );
                            (lay.scroll_max, lay.page_scroll_max)
                        };
                        let mut pg = p.lock().unwrap();
                        pg.clamp_scroll(smax);
                        pg.clamp_page_scroll(pmax);
                    }
                }
            }
            (ParserExec::New, Ok(out)) => {
                // -P -F 回打印的新会话名（首行）；拿到就 attach 过去
                let name = out
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .to_string();
                crate::report::report("ui", &format!("tmux 插件: 新建落成 {name}"));
                if !name.is_empty() {
                    self.parser_attach(name);
                } else {
                    self.parser_refresh();
                }
            }
            (ParserExec::Kill, Ok(_)) | (ParserExec::Reflow, Ok(_)) => {
                self.parser_refresh();
            }
            (_, Err(e)) => {
                crate::report::report("ui", &format!("tmux 插件: 执行失败 {e}"));
                if let Some(p) = &self.parser_page {
                    p.lock().unwrap().set_error(e);
                }
            }
        }
        self.dirty = true;
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
        // 本地会话重孵 = 裸 shell（default_config 无命令）——本地附着账
        // 同步勾销，页牌随行（远程臂不清：default_config 带设置里的
        // attach 命令，重孵即重附，附着账依然成立——不对称来自配置差，
        // 不是工序差）
        if name == "local" && self.local_attached.is_some() {
            crate::report::report("term", "本地重孵 = 裸 shell，附着账勾销");
            self.local_attached = None;
            if let Some(p) = &self.parser_page {
                p.lock().unwrap().set_attached(None);
            }
        }
        {
            let health = self.health_mut(name);
            health.connecting = true;
        }
        // 重孵真实发生即记账——时间闸量的是重孵密度
        // （crate::session::auto_respawn_due），手动触发
        // （kick_reconnect）也算一次重孵，同样刷新
        self.last_auto_respawn_ms = Some(boot_ms() as u64);
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

    /// 死会话续链轮询（BAR-117，about_to_wait 每圈调），两条腿：
    /// ①隧道「不可用→可用」上升沿踢活跃死会话重孵（裁决纯函数
    ///    tunnel::usable_edge_kick）——传输恢复即续链，不等 5s；
    /// ②活跃死会话且距上次重孵够钟（session::auto_respawn_due 同一把
    ///    5s 闸）→ 再踢——死亡驱动链断在「最后一次失败撞闸压住后再无
    ///    死亡事件」上（redroid 判卷实证：TCP refused 百 ms 返回永远
    ///    撞闸，持续断网期链必断），时间到即续链 = 每 5s 敲门一次。
    /// 两条腿都走 respawn_session（记账同刷时间闸，密度守恒）。
    fn poll_tunnel_kick(&mut self) {
        if let Some(snap) = crate::tunnel::snap() {
            let (epoch, state) = {
                let g = snap.lock().unwrap();
                (g.epoch, g.state.clone())
            };
            if epoch != self.last_tunnel_epoch {
                self.last_tunnel_epoch = epoch;
                let prev_usable = self.last_tunnel_usable;
                self.last_tunnel_usable = crate::tunnel::usable(&state);
                if crate::tunnel::usable_edge_kick(prev_usable, &state, self.session_over) {
                    let name = self
                        .router_handle()
                        .map(|r| r.lock().unwrap().active_name());
                    if let Some(name) = name {
                        crate::report::report(
                            "tunnel",
                            &format!("隧道可用沿 → 踢活跃死会话重孵: {name}"),
                        );
                        self.respawn_session(name);
                    }
                }
            }
        }
        if self.session_over
            && crate::session::auto_respawn_due(self.last_auto_respawn_ms, boot_ms() as u64)
            && let Some(name) = self
                .router_handle()
                .map(|r| r.lock().unwrap().active_name())
        {
            crate::report::report("term", &format!("死会话续链: {name} 够钟重孵"));
            self.respawn_session(name);
        }
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
                    h.connecting = false;
                }
                crate::gate::note_session_alive(name, false); // 复活同步进 stats
                if is_active {
                    if self.session_over {
                        // 复活翻牌 → 断线卡该灭：置脏逼一帧（sig 死活维
                        // 翻转 → 终卡槽重烘）。不置脏 = 卡赖在屏上鬼影
                        self.dirty = true;
                    }
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

    /// 会话死亡登记：钉健康牌;活跃方死亡 → 过自动重孵时间闸
    /// （session::auto_respawn_due：首次立即，其后 ≥MIN_AUTO_RESPAWN_MS
    /// 才再放行——瞬死循环节流，redroid 案）;待机方死亡只记账不吵
    /// （切换那一刻再重连——断网期给待机自动重连是烧钱风暴）
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
            // 死活翻牌 → 断线卡该出：置脏逼一帧（终卡槽 sig 死活维翻转
            // 才重烘；不置脏 = 帧不来，卡永远不画——redroid 判卷现场定罪）
            self.dirty = true;
        }
        {
            let h = self.health_mut(name);
            h.dead = true;
            h.connecting = false;
        }
        crate::gate::note_session_alive(name, true); // 死活现况进 stats(考官前置探针)
        if is_active
            && crate::session::auto_respawn_due(self.last_auto_respawn_ms, boot_ms() as u64)
        {
            self.respawn_session(name);
        }
    }

    /// 排干 Java 皮（KfmInputConnection/快捷键行）经 JNI 注入的输入——
    /// 中文落字从这里进终端（NativeActivity 无 InputConnection 的补丁，
    /// 链路见 ime_queue.rs 文件头）。键码在排干侧按当下光标模式翻序列
    /// （模式位只有这里的 Term 知道，keymap.rs 吃 app_cursor 参数）
    fn drain_ime_inject(&mut self) {
        let mut items = crate::ime_queue::global().drain();
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
        // 解析页命名态分流（tmux 插件，2026-09-19）：键盘按键全归命名
        // 行——Enter=提交、退格删字、Esc=取消（收键盘）；先于输入栏判
        //（命名态不会与栏聚焦并存，但单一判定点不许靠「不会并存」）
        if self
            .parser_page
            .as_ref()
            .is_some_and(|p| p.lock().unwrap().naming_active())
        {
            for item in items {
                match item {
                    crate::ime_queue::Inject::Text(s) => {
                        if let Some(p) = &self.parser_page {
                            p.lock().unwrap().naming_push(&s);
                        }
                    }
                    crate::ime_queue::Inject::Key(66) => {
                        let raw = self
                            .parser_page
                            .as_ref()
                            .and_then(|p| p.lock().unwrap().naming_take());
                        self.parser_ime_off();
                        self.parser_new_submit(raw);
                    }
                    crate::ime_queue::Inject::Key(67) => {
                        if let Some(p) = &self.parser_page {
                            p.lock().unwrap().naming_pop();
                        }
                    }
                    crate::ime_queue::Inject::Key(111) => {
                        if let Some(p) = &self.parser_page {
                            p.lock().unwrap().cancel_naming();
                        }
                        self.parser_ime_off();
                    }
                    _ => {}
                }
            }
            self.dirty = true;
            return;
        }
        // 输入栏聚焦分流（期 0 组件三，§五 焦点二态）：键盘按键全归栏，
        // 不下终端——Enter=栏内换行（2026-09-04 用户拍板：发送只走 ▶ 钮/
        // gate submit 注入）、退格删字、Esc 失焦、文本追加。
        // BAR-111：方向十字例外直通终端（栏内 v1 无方向语义，吞了 = 死键，
        // 键盘在场时快捷键行方向键点按/长按连发全灭的真凶）
        if self.input_bar.as_ref().is_some_and(|b| b.is_focused()) {
            let (bar_items, term_items) = crate::ime_queue::split_bar_focus(items);
            if let Some(bar) = &self.input_bar {
                for item in bar_items {
                    match item {
                        crate::ime_queue::Inject::Text(s) => {
                            crate::report::report("ime-input", &format!("commit: {s:?}"));
                            bar.insert_text(&s)
                        }
                        crate::ime_queue::Inject::Key(66) => {
                            // KC_ENTER：栏内换行（2026-09-04 拍板，发送只走 ▶）
                            crate::report::report("ime-input", "enter: 栏内换行");
                            bar.insert_text("\n");
                        }
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
            }
            self.dirty = true;
            if term_items.is_empty() {
                return;
            }
            items = term_items; // BAR-111：方向键续走下方终端路径
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
            // L1 会话切换闸：切换键（默认 Ctrl-]=\x1d，terminal.json
            // switchHotkey 可配，keymap 同一把尺落成字节）不发对端，
            // 活跃/待机槽互换（telnet 转义符惯例）
            if !self.switch_hotkey_bytes.is_empty()
                && bytes.as_bytes() == self.switch_hotkey_bytes.as_slice()
            {
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
            if let Some(bar) = &self.input_bar {
                match &event.logical_key {
                    // Enter=栏内换行（2026-09-04 拍板，发送只走 ▶ 钮）
                    Key::Named(NamedKey::Enter) => bar.insert_text("\n"),
                    Key::Named(NamedKey::Backspace) => bar.backspace(),
                    Key::Named(NamedKey::Escape) => bar.unfocus(),
                    _ => {
                        if let Some(t) = &event.text {
                            bar.insert_text(t);
                        }
                    }
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

    /// AI 字形槽位查找（主槽优先、另一字体槽兜底——路由认知翻转时
    /// 不至于全盲；网格路径双键回退同款）。供 ai_glyphs_to_instances
    /// 的内联闭包调用
    fn ai_slot_of(
        atlas: &crate::glyph_atlas::GlyphAtlas,
        c: char,
        font: u8,
    ) -> (
        crate::glyph_atlas::GlyphKey,
        Option<crate::glyph_atlas::GlyphSlot>,
    ) {
        let k0 = crate::glyph_atlas::GlyphKey {
            font,
            c,
            size: crate::glyph_atlas::GLYPH_SIZE_AI,
        };
        if let Some(s) = atlas.slot(&k0) {
            return (k0, Some(s));
        }
        let k1 = crate::glyph_atlas::GlyphKey {
            font: 1 - font,
            c,
            size: crate::glyph_atlas::GLYPH_SIZE_AI,
        };
        if let Some(s) = atlas.slot(&k1) {
            return (k1, Some(s));
        }
        (k0, None)
    }

    /// 当前栏带高（render_inputbar 同源实测折行——眼手同尺单源，
    /// rasterize 与 draw_frame_gles 共用，2026-08-31 排障实锤的延伸）
    fn current_bar_h(
        term: &dyn TermEmu,
        bar_snap: Option<&crate::input_bar::BarSnap>,
        w: u32,
    ) -> u32 {
        bar_snap.map_or(crate::input_bar::HEIGHT_PX, |bs| {
            crate::input_bar::height_for_lines(term.bar_text_lines(&bs.text, w))
        })
    }

    /// tofu 目击上报：双字体都缺的字符（方框的真身），新字才报
    fn report_tofu(term: &mut dyn TermEmu) {
        let tofu = term.take_tofu_chars();
        if !tofu.is_empty() {
            let list = tofu
                .iter()
                .map(|c| format!("U+{:04X}({c})", *c as u32))
                .collect::<Vec<_>>()
                .join(" ");
            crate::report::report("term", &format!("tofu 目击: {list}"));
        }
    }

    /// 下层 chrome（快捷键行 + 面板底装修）：**softbuffer 兜底路径专用**
    /// （2026-09-07 起 GLES 走图层槽位，见 draw_frame_gles——本函数不再
    /// 参与 GLES 帧装配，性能不再投入，立项书红线保留）。ai_glyphs =
    /// Some(GLES)：面板只画底装修（紫底 + 边框环，panel_off 刚体平移），
    /// 文字实例收集进列表（GPU 图集管线）；None（softbuffer）：面板全
    /// CPU——稳态直画，过渡帧整页离屏渲染后按偏移压盖（BAR-062 考题区）。
    /// 四面板（§五B 四公民）两路径同规：z 序单源 panel_z_order（BAR-083
    /// 动者在上泛化）给底→顶次序，逐槽画；
    /// X 平移直画带裁剪（无淡出——alpha 是 GLES 合成期 tint）。
    /// 返回 ai_layout（布局读数，调用方写回 scroll_sync_layout——眼手同尺）
    #[allow(clippy::too_many_arguments)]
    fn paint_under(
        term: &mut dyn TermEmu,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ime_bottom_px: u32,
        bar_h: u32,
        mods: u8,
        panel_off: i32,
        cfg_off: i32,
        ft_off: i32,
        pt_off: i32,
        z_order: [crate::ai_presence::Panel; 4],
        // 三公民页面 accent（[cfg, ft, pt]，来自 PresenceSnap——静态装配
        // 无 self，快照同行是唯一来源；调用方 None 时给 FALLBACK）
        accents: [crate::ui::accent::AccentPair; 3],
        chat_msgs: &[(bool, String, String)],
        chat_scroll: u32,
        chat_live: bool,
        panel_scratch: &mut Vec<u32>,
        mut ai_glyphs: Option<&mut Vec<crate::glyph_atlas::AiGlyph>>,
        tab_snap: Option<&crate::ui::tab_bar::TabBarSnap>,
        pool_snap: Option<&crate::ui::dual_pool::DualPoolSnap>,
        cfg_snap: Option<&crate::ui::cfg_page::CfgPageSnap>,
        parser_snap: Option<&crate::ui::parser_page::ParserPageSnap>,
    ) -> Option<(u32, u32)> {
        // 分支判定唯一裁决处（panel_split/cfg_split/ft_split/pt_split）——softbuffer
        // 与 GLES 两路径都从这里取，分支语义漂移 = 眼手两张皮（BAR-063 级
        // 事故温床）。网格+键行让位 = 四面板都没靠泊（§五B 四公民）
        let (ai_grid, panel_visible) = crate::termview::panel_split(panel_off, h);
        let (cfg_grid, cfg_visible) = crate::termview::cfg_split(cfg_off, w);
        let (ft_grid, ft_visible) = crate::termview::ft_split(ft_off, w);
        let (pt_grid, pt_visible) = crate::termview::pt_split(pt_off, w);
        let grid_keybar = ai_grid && cfg_grid && ft_grid && pt_grid;
        let bottom_inset = ime_bottom_px + bar_h;
        let mut ai_layout = None;
        // 快捷键行（BAR-017 Rust 自绘覆盖层；inset 必须叠输入栏当前带高
        // ——栏带压在行下沿，漏叠 = 眼手错位，2026-08-31 排障实锤，触摸
        // 几何早就是叠后的）。面板未靠泊才画：靠泊时被面板盖住，画了
        // 白画（GPU 路径还省一次上传带宽）
        if grid_keybar {
            // BAR-085：softbuffer 兜底路径的网格绘制在 8fd907b（图层槽位
            // 化）被连同 gpu_term 形参一起误删——兜底帧只剩 chrome 没有
            // 终端本体（GLES 恒在无人看见，立项书红线「兜底功能等价」
            // 悄悄失守）。恢复：render_into 内含卡片壳涂装+清屏+网格。
            // 壳下缘让位 = 键盘+输入栏带+快捷键行（与 GLES 卡片槽同尺）
            term.render_into(buf, w, h, bottom_inset + crate::keybar::HEIGHT_PX);
            term.render_keybar(buf, w, h, bottom_inset, mods);
        }
        // 四面板按 z_order 底→顶逐槽画（BAR-083 动者在上，调用方算好传入）。
        // softbuffer 兜底路径保留旧「覆盖」语义（被覆盖者 placement 冻结、
        // 遮盖撤走零动画露出）——视口推移只实装在 GLES 主路（2026-09-12，
        // 兜底不再投入的既有档位，欠账记 state.md）
        let acc_of = |p: crate::ai_presence::Panel| match p {
            crate::ai_presence::Panel::Config => accents[0],
            crate::ai_presence::Panel::FileTree => accents[1],
            crate::ai_presence::Panel::Parser => accents[2],
            crate::ai_presence::Panel::Ai => crate::ui::accent::FALLBACK,
        };
        for slot in z_order {
            match slot {
                crate::ai_presence::Panel::Config => {
                    if cfg_visible {
                        crate::termview::paint_cfg_page_chrome(
                            buf,
                            w,
                            h,
                            bottom_inset,
                            cfg_off,
                            acc_of(crate::ai_presence::Panel::Config),
                        );
                        // 标签栏叠在底装修之上（宪法 §四；兜底路径与
                        // GLES 烘焙同源同参——刚体平移传真值 cfg_off）
                        if let Some(ts) = tab_snap {
                            term.paint_cfg_tab_bar(
                                buf,
                                w,
                                h,
                                ts,
                                cfg_off,
                                bottom_inset,
                                acc_of(crate::ai_presence::Panel::Config),
                            );
                        }
                        // 双池（宪法 §五）：兜底路径同源同参。十七修
                        // §六：页面级平移中框由 pool_content 双代自理
                        let page_pan = cfg_snap.is_some_and(|cs| {
                            cs.pan
                                .as_ref()
                                .is_some_and(|p| p.scope == crate::ui::cfg_page::PanScope::Page)
                        });
                        if let Some(ps) = pool_snap {
                            if !page_pan {
                                term.paint_cfg_dual_pool(
                                    buf,
                                    w,
                                    h,
                                    ps,
                                    cfg_off,
                                    acc_of(crate::ai_presence::Panel::Config),
                                );
                            }
                            // 池内容（宪法 §五 目录语义：下池子目录行表/
                            // 上池联动下拉+字段行）——双池框之上同层；
                            // 兜底路径双代同画自理，hold 域恒 None
                            if let Some(cs) = cfg_snap {
                                term.paint_cfg_pool_content(
                                    buf,
                                    w,
                                    h,
                                    ps,
                                    cs,
                                    cfg_off,
                                    acc_of(crate::ai_presence::Panel::Config),
                                    crate::report::boot_ms() as u64,
                                    None,
                                    false, // 兜底整页自带光标
                                );
                            }
                        }
                    }
                }
                crate::ai_presence::Panel::FileTree => {
                    if ft_visible {
                        crate::termview::paint_ft_page_chrome(
                            buf,
                            w,
                            h,
                            bottom_inset,
                            ft_off,
                            acc_of(crate::ai_presence::Panel::FileTree),
                        );
                    }
                }
                crate::ai_presence::Panel::Parser => {
                    if pt_visible {
                        crate::termview::paint_parser_page_chrome(
                            buf,
                            w,
                            h,
                            // 视口化（2026-09-20 用户拍板「卡弹小」）：
                            // 壳吃 bottom_inset——键盘在场页环弹小到
                            // 输入栏带以上，环底 = 页面滚动视口底；
                            // 内容布局仍只吃栏带高（BAR-119 只盖不重排），
                            // 逾视底归键盘遮盖、逾视顶归页缘裁剪带
                            bottom_inset,
                            pt_off,
                            acc_of(crate::ai_presence::Panel::Parser),
                        );
                        // tmux 插件卡内容（2026-09-19 v1）：兜底路径与
                        // GLES 烘焙同源同参——刚体平移传真值 pt_off
                        if let Some(psnap) = parser_snap {
                            term.paint_parser_content(
                                buf,
                                w,
                                h,
                                // BAR-119：解析页布局永不吃键盘 inset
                                // （只盖不重排）
                                bar_h,
                                // 键盘 inset 只喂滚动窗可视底+页缘裁剪带
                                ime_bottom_px,
                                pt_off,
                                psnap,
                                acc_of(crate::ai_presence::Panel::Parser),
                            );
                        }
                    }
                }
                crate::ai_presence::Panel::Ai => {
                    // AI 面板（三分支，panel_off 是缝采样值——无 ui-fx 占槽
                    // 时恒等于目标值 0 或 -h，退化为硬切；中间值 = 弹簧过渡帧）。
                    // 视口下沿让位键盘 + 输入栏带高（2026-09-04 用户拍板：追底
                    // 追到栏带上沿，不越过栏带）；live = 末条流式中
                    if panel_visible {
                        if let Some(out) = &mut ai_glyphs {
                            // GPU：文字实例收集（panel_off 已进行 y）+ 底装修
                            let (layout, glyphs) = term.ai_page_glyphs(
                                w,
                                h,
                                chat_msgs,
                                chat_scroll,
                                bottom_inset,
                                chat_live,
                                panel_off,
                            );
                            **out = glyphs;
                            crate::termview::paint_ai_page_chrome(
                                buf,
                                w,
                                h,
                                bottom_inset,
                                panel_off,
                            );
                            ai_layout = Some(layout);
                        } else if panel_off == 0 {
                            // 面板靠泊（AI 全屏页稳态，期 0③ 真对话页）：不画
                            // 终端网格与快捷键行，深紫暗底 + 消息行视口
                            ai_layout = Some(term.render_ai_page(
                                buf,
                                w,
                                h,
                                chat_msgs,
                                chat_scroll,
                                bottom_inset,
                                chat_live,
                            ));
                        } else {
                            // softbuffer 过渡帧：终端 + 快捷键行在下（照画——
                            // 快捷键行层级低于面板，被落下来的面板盖住是自然
                            // 结果，用户 2026-09-04 拍板；BAR-063：过渡帧不画
                            // 它 = 动画两端硬切 = 闪烁），AI 面板整页离屏渲染
                            // 后按偏移压盖——与直接渲染像素等价
                            panel_scratch.clear();
                            panel_scratch.resize((w as usize) * (h as usize), 0);
                            ai_layout = Some(term.render_ai_page(
                                panel_scratch,
                                w,
                                h,
                                chat_msgs,
                                chat_scroll,
                                bottom_inset,
                                chat_live,
                            ));
                            crate::termview::blit_panel_shifted(
                                buf,
                                panel_scratch,
                                w,
                                h,
                                panel_off,
                            );
                        }
                    }
                }
            }
        }
        ai_layout
    }

    /// 上层 chrome（输入栏 + 光球 + 放大镜，2026-09-05 双层合成）——
    /// 两路径共用。z 序：AI 面板与文字之上（常驻 chrome 任何会话都在，
    /// §二——AI 页也画）。输入栏 sending 图标态跟 AI 运行态硬切（kfmv4
    /// .ai-send-btn.sending ▶ ↔ ⏸）；caret_on = 光标闪烁相位（调用方
    /// 按 CARET_BLINK_MS 算好传入）；放大镜画在所有内容之上
    #[allow(clippy::too_many_arguments)]
    fn paint_over(
        term: &mut dyn TermEmu,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ime_bottom_px: u32,
        bar_snap: Option<&crate::input_bar::BarSnap>,
        sending: bool,
        caret_on: bool,
        ai_snap: Option<crate::ai_presence::PresenceSnap>,
        magnifier_at: Option<(f64, f64)>,
        orb_alpha_out: bool,
    ) {
        if let Some(bs) = bar_snap {
            term.render_inputbar(buf, w, h, ime_bottom_px, bs, sending, caret_on);
        }
        // 光球：四态增益硬切读 ai_presence::orb_gain（闲/运行/pressed/AI页）
        if let Some(s) = ai_snap {
            let (gain, halo_gain) = crate::ai_presence::orb_gain(s.ai_running, s.pressed, s.page);
            term.render_orb(buf, w, h, s.x, s.y, gain, halo_gain, orb_alpha_out);
        }
        // 选区边界拖动中的放大镜浮窗
        if let Some((mx, my)) = magnifier_at {
            term.render_magnifier(buf, w, h, mx, my);
        }
    }

    /// 光栅化一帧（softbuffer 兜底路径，全 CPU 单层）：终端网格 + 下层
    /// chrome + 上层 chrome 进任意像素缓冲。GLES 不走这里（双层装配在
    /// draw_frame_gles——两路径共享 paint_under/paint_over/panel_split，
    /// AI 页文字 GPU 化只发生在 GLES；softbuffer 是立项书红线保留的
    /// 兜底，性能不再投入）。后台离屏倒帧不走这里——值守线程
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
        chat_live: bool,
        bar_snap: Option<&crate::input_bar::BarSnap>,
        caret_on: bool,
        buf: &mut [u32],
        w: u32,
        h: u32,
        panel_off: i32,
        cfg_off: i32,
        ft_off: i32,
        pt_off: i32,
        z_order: [crate::ai_presence::Panel; 4],
        panel_scratch: &mut Vec<u32>,
        tab_snap: Option<&crate::ui::tab_bar::TabBarSnap>,
        pool_snap: Option<&crate::ui::dual_pool::DualPoolSnap>,
        cfg_snap: Option<&crate::ui::cfg_page::CfgPageSnap>,
        parser_snap: Option<&crate::ui::parser_page::ParserPageSnap>,
    ) -> Option<(u32, u32)> {
        let Some(term) = term else {
            buf.fill(KFM_PURPLE); // 字体全灭的降级画面：紫屏 + 已有上报
            return None;
        };
        // 当前栏带高（眼手同尺单源，见 current_bar_h）
        let bar_h = Self::current_bar_h(&**term, bar_snap, w);
        let accents = ai_snap.map_or([crate::ui::accent::FALLBACK; 3], |s| {
            [s.accent_cfg, s.accent_ft, s.accent_pt]
        });
        let ai_layout = Self::paint_under(
            &mut **term,
            buf,
            w,
            h,
            ime_bottom_px,
            bar_h,
            mods,
            panel_off,
            cfg_off,
            ft_off,
            pt_off,
            z_order,
            accents,
            chat_msgs,
            chat_scroll,
            chat_live,
            panel_scratch,
            None,
            tab_snap,
            pool_snap,
            cfg_snap,
            parser_snap,
        );
        let sending = ai_snap.is_some_and(|s| s.ai_running);
        Self::paint_over(
            &mut **term,
            buf,
            w,
            h,
            ime_bottom_px,
            bar_snap,
            sending,
            caret_on,
            ai_snap,
            magnifier_at,
            false,
        );
        Self::report_tofu(&mut **term);
        ai_layout
    }

    /// 平移合成参数求值（十九修 D8）：带/偏移与涂装域同尺的纯函数
    /// （page_pan_band/upper_pan_band/pan_offsets）的 GLES 装配；无
    /// 平移出 None（配置槽单 draw 照旧）。关联函数无 self——
    /// draw_frame_gles 同款按参传
    fn pan_composite(
        cfg_snap: Option<&crate::ui::cfg_page::CfgPageSnap>,
        pool_snap: Option<&crate::ui::dual_pool::DualPoolSnap>,
        w: u32,
    ) -> Option<crate::gles_present::PanComp> {
        let cs = cfg_snap?;
        let p = cs.pan.as_ref()?;
        let ps = pool_snap?;
        let (band, travel) = match p.scope {
            crate::ui::cfg_page::PanScope::Page => {
                let band = crate::termview::page_pan_band(
                    w,
                    0,
                    p.old.pool.upper.y,
                    ps.upper.y,
                    p.old.pool.lower.y + p.old.pool.lower.h as i64,
                    ps.lower.y + ps.lower.h as i64,
                );
                (band, ps.upper.w as i64 + crate::ui::cfg_page::PAN_GAP_PAGE)
            }
            crate::ui::cfg_page::PanScope::Upper => {
                let band = crate::termview::upper_pan_band(&ps.upper, 0);
                let travel = (band.2 - band.0) + crate::ui::cfg_page::PAN_GAP_UPPER;
                (band, travel)
            }
            // 二十修 §六② 下拉换选：体行带 = 上池内容矩形挖掉行 0
            // （行 0 触发器钉住），留隙律/曲线与 Upper 域同尺
            crate::ui::cfg_page::PanScope::UpperBody => {
                let band = crate::termview::upper_body_pan_band(&ps.upper, 0, cs.upper_scroll);
                let travel = (band.2 - band.0) + crate::ui::cfg_page::PAN_GAP_UPPER;
                (band, travel)
            }
        };
        let (d_old, d_new) = crate::ui::cfg_page::pan_offsets(p.dir, p.t, travel);
        // 二十修 §六②：UpperBody 域行 0 触发器钉住条带（带外、从新代
        // 层 dx=0 补画）；行矩形 y 与池高无关（顶锚定），当前几何即
        // 终点几何
        let pinned_strip = if p.scope == crate::ui::cfg_page::PanScope::UpperBody {
            let r0 = crate::ui::cfg_page::upper_row_rect(0, &ps.upper, cs.upper_scroll);
            Some((
                r0.y as i32,
                (r0.y + crate::ui::cfg_page::FIELD_ROW_H as i64) as i32,
            ))
        } else {
            None
        };
        Some(crate::gles_present::PanComp {
            band: (band.0 as i32, band.1 as i32, band.2 as i32, band.3 as i32),
            t: p.t,
            old_dx: d_old as f32,
            new_dx: d_new as f32,
            // 隙底语义分域（十八修钉）：Page 间隙带=页背景（填）；
            // Upper/UpperBody 静物=稳态涂装的池内芯（不填，带外静物
            // 原样透出）
            clear_bg: p.scope == crate::ui::cfg_page::PanScope::Page,
            scope_page: p.scope == crate::ui::cfg_page::PanScope::Page,
            pinned_strip,
        })
    }

    /// GLES 一帧的图层装配（2026-09-07 槽位化，ui-base §八）：终端网格
    /// GPU 实例 → 键行槽 → 面板槽（placement.y=panel_off，tint.α=panel_fade）
    /// → AI 文字 GPU 实例（u_alpha=panel_fade 随面板显影）→ 上层槽。
    /// chrome 三槽置脏烘焙（LayerSigs 记账），动画帧零光栅零上传；AI 页
    /// 文字走图集管线（panel_off 进实例 y）。返回 ai_layout（调用方写回
    /// scroll_sync_layout——眼手同尺）
    #[allow(clippy::too_many_arguments)]
    fn draw_frame_gles(
        g: &mut crate::gles_present::GlesPresent,
        th: &Option<crate::gate::SharedTerm>,
        mods: u8,
        caret_on: bool,
        chat_msgs: &[(bool, String, String)],
        chat_scroll: u32,
        chat_live: bool,
        ai_snap: Option<crate::ai_presence::PresenceSnap>,
        bar_snap: Option<&crate::input_bar::BarSnap>,
        magnifier_at: Option<(f64, f64)>,
        ime_bottom_px_raw: u32,
        chrome_inset_px: &mut u32,
        sigs: &mut LayerSigs,
        tab_snap: Option<&crate::ui::tab_bar::TabBarSnap>,
        pool_snap: Option<&crate::ui::dual_pool::DualPoolSnap>,
        cfg_snap: Option<&crate::ui::cfg_page::CfgPageSnap>,
        parser_snap: Option<&crate::ui::parser_page::ParserPageSnap>,
        session_over: bool, // 活跃会话死活（A 断线状态卡：终卡槽 sig 末维+烘焙触发源）
        drag: Option<(crate::ai_presence::Panel, f32)>,
    ) -> Option<(u32, u32)> {
        let (w, h) = g.size();
        if !TERMINAL_MODE {
            g.present_solid(KFM_PURPLE);
            return None;
        }
        crate::gate::note_frame_size(w, h); // 给后台倒帧值守记账
        // 键盘 inset chrome 跟随过缝（ui-base §三第二道缝）：采样值经
        // 调用方写回字段——触摸命中下一拍吃同一份（眼手同尺）。终端
        // resize 不在这里：永远吃真实值（pty resize 抖动红线）
        *chrome_inset_px = crate::ui::seam::sample_chrome_ime_inset(
            ime_bottom_px_raw as f32,
            crate::report::boot_ms() as u64,
        )
        .max(0.0) as u32;
        // AI 面板 Y 偏移过缝（ui-base §三）：目标值 = AI 在栈 0 靠泊 /
        // 不在栈 -屏高屏外；无 ui-fx 占槽 = 直通目标值（硬切）。
        // 被覆盖时自身 off 恒靠泊位（目标值只问栈），上方推移经
        // viewport_push::covered_extra 合成期另加（§五B 2026-09-12 改写）
        let ai_page = ai_snap.is_some_and(|s| s.page == crate::ai_presence::Page::AiFullscreen);
        let panel_target = if ai_page { 0.0 } else { -(h as f32) };
        let panel_off = crate::ui::seam::sample_ai_panel_offset_y(
            panel_target,
            crate::report::boot_ms() as u64,
        ) as i32;
        // 淡入淡出已取消（2026-09-11 用户拍板「不好看」）：面板全程恒实，
        // panel_fade_alpha 退役备查（fx_ease.rs 内考题钉住）
        let panel_fade = 1.0_f32;
        // 面板栈读数重建（底→顶；snap 只露 top/covered 两格，栈规 ≤2）
        use crate::ai_presence::Panel;
        let mut stack_vec: Vec<Panel> = Vec::new();
        if let Some(s) = ai_snap {
            if let Some(c) = s.covered {
                stack_vec.push(c);
            }
            if let Some(t) = s.top {
                stack_vec.push(t);
            }
        }
        // 配置/文件树/解析面板 X 偏移过缝（§五B 第三/四/五道缝）。target 只问栈
        // （BAR-084 单源 panel_target_and_draw：活性泄漏进 target = 退场
        // 回粘）；draw = 在栈或缝/拖拽活跃（退场动画画完）。右缘家屏外
        // +w（配置/解析），文件树家屏外 -w（左缘来向）
        let cfg_in = stack_vec.contains(&Panel::Config);
        let ft_in = stack_vec.contains(&Panel::FileTree);
        let pt_in = stack_vec.contains(&Panel::Parser);
        let drag_cfg = drag.filter(|(p, _)| *p == Panel::Config).map(|(_, o)| o);
        let drag_ft = drag.filter(|(p, _)| *p == Panel::FileTree).map(|(_, o)| o);
        let drag_pt = drag.filter(|(p, _)| *p == Panel::Parser).map(|(_, o)| o);
        let cfg_active = crate::ui::seam::config_panel_offset_x_active() || drag_cfg.is_some();
        let ft_active = crate::ui::seam::filetree_panel_offset_x_active() || drag_ft.is_some();
        let pt_active = crate::ui::seam::parser_panel_offset_x_active() || drag_pt.is_some();
        let (cfg_target, cfg_draw) =
            crate::ui::stage::panel_target_and_draw(cfg_in, cfg_active, w as f32);
        let (ft_target, ft_draw) =
            crate::ui::stage::panel_target_and_draw(ft_in, ft_active, -(w as f32));
        let (pt_target, pt_draw) =
            crate::ui::stage::panel_target_and_draw(pt_in, pt_active, w as f32);
        // z 序单源（stage::panel_z_order，BAR-083「动者在上」四公民泛化）：
        // 撤顶面板瞬栈顶翻成底下的不透明面板，动者仍压顶滑出可见；
        // 活性 = 缝动画中或拖拽锁定中（跟手期手指压着的面板在顶）
        let z_order = crate::ui::stage::panel_z_order(
            &stack_vec,
            [
                crate::ui::seam::ai_panel_offset_y_active(),
                cfg_active,
                ft_active,
                pt_active,
            ],
        );
        // 跟手拖拽锁定期旁路缝采样（panel_drag：直接操纵不是动画——
        // 手指停画面停，零插值滞后；缝底下的自动动画采样被盖住不可见）。
        // 拖拽偏移是「距靠泊距离」，按家折符号（右缘家 +/文件树家 -）
        let cfg_off = match drag_cfg {
            Some(off) => off as i32,
            None => crate::ui::seam::sample_config_panel_offset_x(
                cfg_target,
                crate::report::boot_ms() as u64,
            ) as i32,
        };
        let cfg_fade = 1.0_f32;
        let ft_off = match drag_ft {
            Some(off) => -(off as i32),
            None => crate::ui::seam::sample_filetree_panel_offset_x(
                ft_target,
                crate::report::boot_ms() as u64,
            ) as i32,
        };
        let ft_fade = 1.0_f32;
        let pt_off = match drag_pt {
            Some(off) => off as i32,
            None => crate::ui::seam::sample_parser_panel_offset_x(
                pt_target,
                crate::report::boot_ms() as u64,
            ) as i32,
        };
        let pt_fade = 1.0_f32;
        // 视口推移（2026-09-12 用户拍板，ui/viewport_push.rs）：基座页
        // （终端卡槽/网格实例/键行槽）随面板 off 平移；被压面板吃上方
        // 推移（交叉轴叠加 [配置,AI]：配置随 AI 下移）。off 已含缝采样
        // 与拖拽旁路——跟手期底页随动零新机制。§五B「被盖 placement
        // 冻结」就此改写为「被压随动」：遮盖撤走从「零动画露出」变成
        // 「随推移滑回」。Q 弹形变同日二审取消（实拍不合预期）——scale
        // 恒 1.0，仿射管线保留
        let (vpush, _p_max) =
            crate::ui::viewport_push::viewport_push(panel_off, cfg_off, ft_off, pt_off, w, h);
        let term_place = (vpush.dx, vpush.dy, 1.0);
        let ai_extra = crate::ui::viewport_push::covered_extra(
            &stack_vec,
            Panel::Ai,
            panel_off,
            cfg_off,
            ft_off,
            pt_off,
            w,
            h,
        );
        let cfg_extra = crate::ui::viewport_push::covered_extra(
            &stack_vec,
            Panel::Config,
            panel_off,
            cfg_off,
            ft_off,
            pt_off,
            w,
            h,
        );
        let ft_extra = crate::ui::viewport_push::covered_extra(
            &stack_vec,
            Panel::FileTree,
            panel_off,
            cfg_off,
            ft_off,
            pt_off,
            w,
            h,
        );
        let pt_extra = crate::ui::viewport_push::covered_extra(
            &stack_vec,
            Panel::Parser,
            panel_off,
            cfg_off,
            ft_off,
            pt_off,
            w,
            h,
        );
        let (ai_grid, panel_visible) = crate::termview::panel_split(panel_off, h);
        let (cfg_grid, cfg_visible) = crate::termview::cfg_split(cfg_off, w);
        let (ft_grid, ft_visible) = crate::termview::ft_split(ft_off, w);
        let (pt_grid, pt_visible) = crate::termview::pt_split(pt_off, w);
        let cfg_visible = cfg_visible && cfg_draw;
        let ft_visible = ft_visible && ft_draw;
        let pt_visible = pt_visible && pt_draw;
        // 网格+键行让位 = 四面板都没靠泊（任一靠泊在顶即整页盖住终端）
        let grid_keybar = ai_grid && cfg_grid && ft_grid && pt_grid;
        let Some(term_arc) = th else {
            // 字体全灭的降级画面：紫屏（与 soft 路径同规）
            g.present_solid(KFM_PURPLE);
            return None;
        };
        let ime = *chrome_inset_px;
        let bar_h = Self::current_bar_h(&**term_arc.lock().unwrap(), bar_snap, w);

        // 1) 终端网格 GPU 进料（面板靠泊 = 整页被不透明面板盖住，零生成
        // 零绘制；过渡帧照生成——面板落下的过程中要透出终端）
        let t_gen = std::time::Instant::now();
        let mut bg_inst: Vec<crate::glyph_atlas::BgInstance> = Vec::new();
        let mut glyphs_by_page: Vec<Vec<crate::glyph_atlas::GlyphInstance>> = Vec::new();
        if grid_keybar {
            let cells = term_arc.lock().unwrap().gpu_cells(w, h);
            let (cell_w, cell_h) = term_arc.lock().unwrap().cell_size();
            // 格尺寸变 → 图集终端字形全成陈墨，先冲刷再进料
            // （2026-09-11 捏合缩放字号不跟案；gles_present 侧单点，
            //  捏合/缩放读回/未来一切 set_cell_size 路径自动全覆盖）
            g.sync_term_glyph_size(cell_w, cell_h);
            let mut inst = crate::glyph_atlas::grid_to_instances(
                &cells,
                g.atlas(),
                cell_w,
                cell_h,
                crate::termview::DEFAULT_BG,
                |c| {
                    let k0 = crate::glyph_atlas::GlyphKey {
                        font: 0,
                        c,
                        size: crate::glyph_atlas::GLYPH_SIZE_TERM,
                    };
                    if let Some(s) = g.atlas().slot(&k0) {
                        return (k0, Some(s));
                    }
                    let k1 = crate::glyph_atlas::GlyphKey {
                        font: 1,
                        c,
                        size: crate::glyph_atlas::GLYPH_SIZE_TERM,
                    };
                    if let Some(s) = g.atlas().slot(&k1) {
                        return (k1, Some(s));
                    }
                    (k0, None)
                },
            );
            if !inst.misses.is_empty() {
                let t = term_arc.lock().unwrap();
                for k in &inst.misses {
                    if let Some((fid, m, bmp, ox, oy)) = t.rasterize_for_atlas(k.c) {
                        let key = crate::glyph_atlas::GlyphKey {
                            font: fid,
                            c: k.c,
                            size: crate::glyph_atlas::GLYPH_SIZE_TERM,
                        };
                        g.atlas_insert(key, m.width as u32, m.height as u32, &bmp, ox, oy);
                    }
                }
                let inst2 = crate::glyph_atlas::grid_to_instances(
                    &cells,
                    g.atlas(),
                    cell_w,
                    cell_h,
                    crate::termview::DEFAULT_BG,
                    |c| {
                        let k0 = crate::glyph_atlas::GlyphKey {
                            font: 0,
                            c,
                            size: crate::glyph_atlas::GLYPH_SIZE_TERM,
                        };
                        if let Some(s) = g.atlas().slot(&k0) {
                            return (k0, Some(s));
                        }
                        let k1 = crate::glyph_atlas::GlyphKey {
                            font: 1,
                            c,
                            size: crate::glyph_atlas::GLYPH_SIZE_TERM,
                        };
                        if let Some(s) = g.atlas().slot(&k1) {
                            return (k1, Some(s));
                        }
                        (k0, None)
                    },
                );
                inst = inst2;
            }
            // 按图集页分组（每页一次 draw）
            bg_inst = inst.bg;
            glyphs_by_page = vec![Vec::new(); g.atlas().pages().len()];
            for gi in inst.glyph {
                let p = gi.page as usize;
                if p < glyphs_by_page.len() {
                    glyphs_by_page[p].push(gi);
                }
            }
        }
        let gen_us = t_gen.elapsed().as_micros() as u64;

        // 2) 槽位烘焙（ui-base §八 渲染成本模型）：置脏才光栅+上传——
        // 动画帧（panel_off 逐帧变）只动合成期 placement，零光栅零上传。
        // slot_bake 内做 mark_chrome_alpha（「纯黑=空白」约定——黑屏案
        // 2026-09-05 教训：一刀切 |= alpha 会变成不透明黑膜）
        let t_ras = std::time::Instant::now();
        let bottom_inset = ime + bar_h;
        // 八槽可见性单源（BAR-070：图层化首版漏设上层槽 → 输入栏/光球/
        // 放大镜集体隐身——可见性判定收进纯逻辑，每帧八槽都从这出）
        let pan_active = cfg_snap.as_ref().is_some_and(|cs| cs.pan.is_some());
        // BAR-097：Upper 域平移活性（池区拆层上岗旗——池框/下池行由
        // PoolFx/LowerRowsPan 层承担，配置槽 hold 烘焙不画池；Page 域
        // 不动：池留在配置槽随页平移）。二十修：UpperBody 域同律
        // （体行归双代层，行 0 触发器钉住留在 hold 烘焙里）
        let pan_upper = cfg_snap.and_then(|cs| cs.pan.as_ref()).is_some_and(|p| {
            matches!(
                p.scope,
                crate::ui::cfg_page::PanScope::Upper | crate::ui::cfg_page::PanScope::UpperBody
            )
        });
        // BAR-104：喂交接差分机本帧平移域（0=无/1=Upper/2=Page/
        // 3=UpperBody）+ 本帧 cfg epoch（present_frame 内消费；未武装
        // 时仅一次原子写，零开销）。2026-09-17 Page 域接入：差分机不再
        // 只喂 Upper——切标签（Page）与下池点行（Upper）共用一台仪器
        // （观测矩阵 Page 像素级盲区补盲）；二十修 UpperBody 同机
        let pan_scope = cfg_snap
            .and_then(|cs| cs.pan.as_ref())
            .map_or(0u8, |p| match p.scope {
                crate::ui::cfg_page::PanScope::Upper => 1,
                crate::ui::cfg_page::PanScope::Page => 2,
                crate::ui::cfg_page::PanScope::UpperBody => 3,
            });
        crate::gles_present::set_panend_mark(pan_scope, cfg_snap.map_or(0, |cs| cs.epoch));
        let slot_vis = crate::ui::stage::slot_visibility(
            grid_keybar,
            panel_visible,
            cfg_visible,
            ft_visible,
            pt_visible,
            pan_active,
        );
        g.set_slot_visible(crate::gles_present::ChromeSlot::Keybar, slot_vis[0]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::Panel, slot_vis[1]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::Config, slot_vis[2]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::FileTree, slot_vis[3]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::Parser, slot_vis[4]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::Over, slot_vis[5]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::TermCard, slot_vis[6]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::PanOld, slot_vis[7]);
        g.set_slot_visible(crate::gles_present::ChromeSlot::PanMove, slot_vis[7]);
        // BAR-096 拆层：标签栏层与光标层都属配置页（cfg_visible 一票）；
        // 光标层另有"无行不画"（空池无光标）
        g.set_slot_visible(crate::gles_present::ChromeSlot::TabBar, slot_vis[2]);
        g.set_slot_visible(
            crate::gles_present::ChromeSlot::LowerCursor,
            slot_vis[2] && cfg_snap.as_ref().is_some_and(|cs| !cs.rows.is_empty()),
        );
        // BAR-097 池区两层：仅 Upper 平移期上岗（贴死即隐，稳态池区回
        // 配置槽）
        g.set_slot_visible(
            crate::gles_present::ChromeSlot::PoolFx,
            slot_vis[2] && pan_upper,
        );
        g.set_slot_visible(
            crate::gles_present::ChromeSlot::LowerRowsPan,
            slot_vis[2] && pan_upper,
        );
        // 二十四修：下拉面板层——配置页可见且抽屉有余影（progress>0）
        // 才上岗；贴死即隐（稳态零烘焙零绘制）
        g.set_slot_visible(
            crate::gles_present::ChromeSlot::DropdownPanel,
            slot_vis[2]
                && cfg_snap
                    .as_ref()
                    .is_some_and(|cs| cs.dropdown_progress > 0.001),
        );
        // 终端卡片壳槽烘焙（2026-09-11）：恒靠泊零 placement——sig 含
        // ime/bar_h 是因为壳下缘停在快捷键行上沿（键盘开合期逐帧重烘焙
        // 加入 ui-base §八 期 2 债同族清单，不单独立项）
        if grid_keybar && sigs.termcard.feed((w, h, ime, bar_h)) {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::TermCard);
            px.fill(0);
            crate::termview::paint_term_card_chrome(
                px,
                w,
                h,
                bottom_inset + crate::keybar::HEIGHT_PX,
            );
            g.slot_bake(crate::gles_present::ChromeSlot::TermCard);
        }
        // 断线状态卡层（A 断线治理）：独立槽——z 序必须在字形之上
        // （TermCard 槽是最底层，画里面会被网格文字盖死，redroid 判卷
        // 定罪）；可见性 = 裸终端页（slot_vis[6]）+ session_over 双闸，
        // 会话复活/面板靠泊即隐。sig 带死活维：翻转才重烘，稳态零成本
        g.set_slot_visible(
            crate::gles_present::ChromeSlot::DownCard,
            slot_vis[6] && session_over,
        );
        if slot_vis[6] && sigs.downcard.feed((w, h, session_over as u8)) {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::DownCard);
            px.fill(0);
            if session_over {
                term_arc.lock().unwrap().render_down_card(px, w, h);
            }
            g.slot_bake(crate::gles_present::ChromeSlot::DownCard);
        }
        // 键行槽烘焙：sig=render_keybar 读的每个输入（靠泊时槽隐藏，
        // 烘焙物常驻纹理，面板收起重现身零成本）
        if grid_keybar && sigs.keybar.feed((mods, ime, bar_h, w, h)) {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::Keybar);
            px.fill(0);
            term_arc
                .lock()
                .unwrap()
                .render_keybar(px, w, h, bottom_inset, mods);
            g.slot_bake(crate::gles_present::ChromeSlot::Keybar);
        }
        // 面板槽：烘焙画布恒为靠泊位（panel_off=0 画），位移交给合成
        // placement——这就是「动画零光栅」的承载点
        if panel_visible && sigs.panel.feed((w, h, ime, bar_h)) {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::Panel);
            px.fill(0);
            crate::termview::paint_ai_page_chrome(px, w, h, bottom_inset, 0);
            g.slot_bake(crate::gles_present::ChromeSlot::Panel);
        }
        // 配置槽（§五B）：同规——画布恒靠泊位（cfg_off=0），X 位移在合成期。
        // sig 带 accent 两维（宪法 §2.2 召唤即随机：重随必触发重烘焙，
        // 漏维 = 新色不进纹理，满屏旧色——2026-09-12 accent 落地即钉）。
        // accent 来源 = PresenceSnap 三字段（静态装配无 self，快照同行）
        let (acc_cfg, acc_ft, acc_pt) = ai_snap.map_or(
            (
                crate::ui::accent::FALLBACK,
                crate::ui::accent::FALLBACK,
                crate::ui::accent::FALLBACK,
            ),
            |s| (s.accent_cfg, s.accent_ft, s.accent_pt),
        );
        // 标签栏 sig 三维（宪法 §四）：选中/横滚/光标 x——游标弹簧动画
        // 逐帧新值逐帧重烘焙（键盘 inset 同族成本，已记 ui-base §八债单）。
        // 八修换案（开口框→填色标签块）后线长两维随组件退役
        // BAR-096：标签栏三维随标签栏层走（本槽不再吃）
        // 双池 sig 一维（宪法 §五）：上池高——内容进出/屏尺寸变（w/h 已在
        // sig）触发布局重算时必须重烘焙
        let pool_upper_h = pool_snap.map_or(0, |ps| ps.upper.h);
        // 池内容 sig 一维（宪法 §五 目录语义）：cfg_page 代际——聚焦切换/
        // 下拉开合/行表字段重建都 bump（漏维 = 旧行表新聚焦鬼影）
        let cfg_epoch = cfg_snap.map_or(0, |cs| cs.epoch);
        // 动画两维（十五修 §五/§六）：光标行号 ×64 量化（1/64 行 ≈ 2.5px
        // 精度够肉眼无缝）+ 下拉进度 ×1000 量化——动画在播逐帧新值触发
        // 槽重烘焙，收敛后值稳零空烧（同 §四 纪律）。平移不再进 sig
        // （十九修 D8 合成期优先律：pan_q 逐帧重烘焙 = 全页双代重光栅
        // 是真机掉帧病灶——平移呈现全在合成期，烘焙恒画新代稳态）
        // BAR-096：光标行号随下池光标层走（本槽不再吃）
        let dd_progress_q = cfg_snap.map_or(0, |cs| (cs.dropdown_progress * 1000.0).round() as u32);
        // 二十四修：宽度账瞬时值进 sig（收敛/无账恒 0）
        let trig_w_q = cfg_snap
            .and_then(|cs| cs.trigger_w)
            .map_or(0, |v| v.round().max(0.0) as u32);
        // 平移 hold 模式维（BAR-092 补丁）：起步 true / 贴死 false 各翻
        // 转一次 = 各一烘。漏维 = 贴死后 hold 烘焙滞留（上池行消失到
        // 下次交互，rd427 f0008 实咬）
        let pan_hold = cfg_snap.is_some_and(|cs| cs.pan.is_some());
        // 动效预览 sig 一维（十四修 §六）：动画展品开着 = 33ms 时间桶
        // 逐帧变 → 槽逐帧重烘焙；关着恒 0（无动画零烘焙同 §四纪律）
        let anim_bucket = if cfg_visible && Self::cfg_anim_modal_open() {
            crate::report::boot_ms() as u64 / 33
        } else {
            0
        };
        // 十九修 D8 平移升合成期两件（顺序敏感）：
        // ①PanOld 捕获——平移起步帧封存旧代（此刻配置槽纹理还是旧代像素，
        // 必须抢在下面重烘焙之前）。BAR-100 零拷贝化：纹理互换代替
        // 26MB 拷贝+13MB 重传（旧代像素本就在屏上纹理里，互换柄即封存）。
        // 捕获源裁决 = pan_capture_src（纯函数有钉）：上一笔 Upper =
        // 旧代行在上一笔 PanMove（配置槽是 hold 烘焙无上池行）；首笔或
        // 上一笔 Page = 配置槽纹理即旧代（Page 域新代与配置槽同图）。
        // 账键 = (epoch, scope, dir)：新账才捕，账清（贴死/离页）即释放
        let pan_now =
            cfg_snap.and_then(|cs| cs.pan.as_ref().map(|p| (cs.epoch, p.scope as u8, p.dir)));
        if let Some(k) = pan_now {
            if sigs.pan_cap != Some(k) {
                let src_slot =
                    match crate::ui::cfg_page::pan_capture_src(sigs.pan_cap.is_some_and(|pk| {
                        pk.1 == crate::ui::cfg_page::PanScope::Upper as u8
                            || pk.1 == crate::ui::cfg_page::PanScope::UpperBody as u8
                    })) {
                        crate::ui::cfg_page::PanCaptureSrc::PanMove => {
                            crate::gles_present::ChromeSlot::PanMove
                        }
                        crate::ui::cfg_page::PanCaptureSrc::Config => {
                            crate::gles_present::ChromeSlot::Config
                        }
                    };
                g.slot_swap_tex(src_slot, crate::gles_present::ChromeSlot::PanOld);
                // ②新代滑动层——仅 Upper/UpperBody 域重画（配置槽 hold
                // 烘焙无上池行，新行必须独立成层——BAR-092 三咬「新内容
                // 不跟随，贴死闪现」的根治）；Page 域新代与配置槽同图，
                // 合成期复用配置槽纹理（BAR-100，整页重画+13MB 上传全省）
                if k.1 == crate::ui::cfg_page::PanScope::Upper as u8
                    || k.1 == crate::ui::cfg_page::PanScope::UpperBody as u8
                {
                    let pmx = g.slot_canvas(crate::gles_present::ChromeSlot::PanMove);
                    // BAR-104：预填页底色——半透明文字/框边在透明画布上落墨
                    // 丢底色贡献，贴死交接比稳态页内版暗一截（全卡闪变）
                    pmx.fill(crate::ui::accent::CARD_PAGE_BG);
                    if let (Some(ps), Some(t), Some(cs)) = (pool_snap, th, cfg_snap) {
                        let mut settled = cs.clone();
                        settled.pan = None;
                        // 二十四修：新代层锚收敛态——宽度账瞬时值不带进
                        // 一次性烘焙（行 0 活体由钉住条带从配置槽直取，
                        // 本层行 0 本就在带外被裁；归零防残留中间宽）
                        settled.trigger_w = None;
                        // BAR-105：新代层锚**终点几何**（同 LowerRowsPan
                        // 纪律）——起步几何烘的芯渐变分母（denom 含池高）
                        // 与池框底缘随 glide 逐帧过期，贴死帧与稳态重烘
                        // 必错位；锚终点后带内 t=1 与稳态逐像素一致
                        let target_h = crate::ui::dual_pool::dual_pool_handle()
                            .map(|p| p.lock().unwrap().target_upper_h())
                            .unwrap_or(ps.upper.h);
                        let ps_final = crate::ui::dual_pool::final_pool_snap(
                            ps,
                            &crate::ui::dual_pool::pool_area(w, h, bottom_inset),
                            target_h,
                        );
                        t.lock()
                            .unwrap()
                            .paint_cfg_dual_pool(pmx, w, h, &ps_final, 0, acc_cfg);
                        t.lock().unwrap().paint_cfg_pool_content(
                            pmx,
                            w,
                            h,
                            &ps_final,
                            &settled,
                            0,
                            acc_cfg,
                            crate::report::boot_ms() as u64,
                            None, // 新代层全量画（双代层不是 hold 烘焙）
                            true, // BAR-096：选中框由下池光标层合成期提供
                        );
                    }
                    g.slot_bake(crate::gles_present::ChromeSlot::PanMove);
                    crate::report::report("panend", &format!("panmove_bake epoch={}", k.0));
                }
                sigs.pan_cap = Some(k);
            }
        } else {
            sigs.pan_cap = None;
        }
        if cfg_visible
            && sigs.config.feed(ConfigSig {
                w,
                h,
                ime,
                bar_h,
                c1: acc_cfg.c1,
                c2: acc_cfg.c2,
                // BAR-097：Upper 平移期冻结为 0——hold 烘焙不画池（池区
                // 归 PoolFx/LowerRowsPan 层），真值逐帧变只会白触发全页
                // 重烘（ras 28ms 残余的结构性病根）；Page 域/稳态照旧
                // 真值（池在配置槽里，几何变必须重烘）
                pool_upper_h: if pan_upper { 0 } else { pool_upper_h },
                cfg_epoch,
                dd_progress_q,
                trig_w_q,
                pan_hold,
                anim_bucket,
            })
        {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::Config);
            px.fill(0);
            crate::termview::paint_cfg_page_chrome(px, w, h, bottom_inset, 0, acc_cfg);
            // BAR-096 拆层：标签栏不再进配置槽（改由 TabBar 层绘制——
            // 游标滑行只脏 0.65MB 小层）；此处留空 = 页背景，层画在其上
            // 双池（宪法 §五）：与标签栏同槽同 accent——内卡反转在涂装
            // 内部兑现（c2→c1），调用方无感。十九修 D8：烘焙恒画新代
            // 稳态（pan 剥离）——Page 域带内像素被 band fill 覆盖、
            // Upper 域带内上池行不画（pan_hold_scope，静物=池内芯）；
            // 双代呈现全在合成期
            // BAR-097：Upper 平移期池框也不进配置槽（归 PoolFx 层逐帧
            // 烘——本槽 pool_upper_h 冻结后恒定，零重烘）
            if let (Some(ps), Some(t)) = (pool_snap, th) {
                if !pan_upper {
                    t.lock()
                        .unwrap()
                        .paint_cfg_dual_pool(px, w, h, ps, 0, acc_cfg);
                }
                // 池内容（§五 目录语义）：双池框之上同槽。
                // 二十修：hold 域传 scope（Upper=上池行全 hold；
                // UpperBody=体行 1.. hold、行 0 触发器钉住照画）；
                // 二十四修：下拉面板已拆层（DropdownPanel 槽），
                // 本槽从此不含面板像素——PanOld 零拷贝捕获恒净
                if let Some(cs) = cfg_snap {
                    let mut settled = cs.clone();
                    let hold_scope = settled.pan.as_ref().map(|p| p.scope);
                    settled.pan = None;
                    t.lock().unwrap().paint_cfg_pool_content(
                        px,
                        w,
                        h,
                        ps,
                        &settled,
                        0,
                        acc_cfg,
                        crate::report::boot_ms() as u64,
                        hold_scope.filter(|_| pan_upper),
                        true, // BAR-096：选中框由下池光标层合成期提供
                    );
                }
            }
            g.slot_bake(crate::gles_present::ChromeSlot::Config);
        }
        // BAR-096 拆层烘焙（帧饥饿根治）：标签栏层 + 下池光标层——
        // 各持小画布（标签栏 屏宽×TAB_LAYER_H ≈0.65MB / 光标 池内容宽×
        // 行高 ≈0.69MB）；动画期只脏这两层，替代配置槽每次 14MB 全页
        // 重光栅+上传（draw_avg 47ms → 21fps 的三路真凶：光标/游标/池高）
        if cfg_visible {
            if let (Some(ts), Some(t)) = (tab_snap, th) {
                use std::hash::{Hash, Hasher};
                let tw = w;
                let thh = crate::ui::tab_bar::TAB_LAYER_H;
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                for tb in &ts.tabs {
                    tb.hash(&mut hasher);
                }
                for p in &ts.colors {
                    (p.c1, p.c2).hash(&mut hasher);
                }
                let tabs_hash = hasher.finish();
                let ls = ts.line_span.unwrap_or((0, 0));
                let sig = TabBarSig {
                    w: tw,
                    layer_h: thh,
                    sel: ts.selected as u32,
                    scroll: ts.scroll_px as i32,
                    cx_q: (ts.cursor_x * 64.0).round() as i32,
                    c1: acc_cfg.c1,
                    c2: acc_cfg.c2,
                    line_span: (ls.0 as i32, ls.1 as i32),
                    tabs_hash,
                };
                if sigs.tabbar.feed(sig) {
                    g.set_slot_dims(crate::gles_present::ChromeSlot::TabBar, tw, thh);
                    let bx = g.slot_canvas(crate::gles_present::ChromeSlot::TabBar);
                    bx.fill(0);
                    t.lock().unwrap().paint_tab_bar_layer(
                        bx,
                        tw,
                        thh,
                        i64::from(crate::ui::tab_bar::content_origin().1),
                        ts,
                        acc_cfg,
                    );
                    g.slot_bake(crate::gles_present::ChromeSlot::TabBar);
                }
            }
            if let (Some(ps), Some(cs), Some(t)) = (pool_snap, cfg_snap, th) {
                use crate::ui::cfg_page as cp;
                let cw = ps
                    .lower
                    .w
                    .saturating_sub((cp::POOL_CONTENT_INSET * 2) as u32);
                let chh = cp::LOWER_ROW_H;
                let px0 = ps.lower.x + cp::POOL_CONTENT_INSET;
                let stride = cp::LOWER_ROW_H as i64 + cp::ROW_GAP;
                let py0 = ps.lower.y
                    + cp::POOL_CONTENT_INSET
                    + (cs.cursor_row * stride as f32).round() as i64;
                let page_denom =
                    (i64::from(w.saturating_sub(1)) + i64::from(h.saturating_sub(1))).max(1);
                let sig = CursorSig {
                    w: cw,
                    h: chh,
                    c1: acc_cfg.c1,
                    c2: acc_cfg.c2,
                    px: px0 as i32,
                    py: py0 as i32,
                    denom: page_denom,
                    focus: cs.focus as u32,
                    epoch: cs.epoch,
                };
                if sigs.cursor.feed(sig) {
                    g.set_slot_dims(crate::gles_present::ChromeSlot::LowerCursor, cw, chh);
                    let cxp = g.slot_canvas(crate::gles_present::ChromeSlot::LowerCursor);
                    cxp.fill(0);
                    // BAR-107：层内只画框（芯半透明 0x55），选中行文字归
                    // 行层透出——「框动行不动」，平移期不再双画错位
                    t.lock()
                        .unwrap()
                        .paint_lower_cursor_layer(cxp, cw, chh, px0, py0, page_denom, acc_cfg);
                    g.slot_bake(crate::gles_present::ChromeSlot::LowerCursor);
                }
            }
            // 二十四修：下拉面板层烘焙（抽屉开合/细框滑行/宽度伸缩
            // 逐帧只脏这块小画布）。z 序在带内双代之上——并发同拍期
            // 抽屉收起盖在滑动的体行之上
            if cfg_snap
                .as_ref()
                .is_some_and(|cs| cs.dropdown_progress > 0.001)
                && let (Some(ps), Some(cs), Some(t)) = (pool_snap, cfg_snap, th)
                && let Some((pr, ox)) = Self::cfg_dropdown_panel_geom(th, cs, ps, h)
            {
                use crate::ui::cfg_page as cp;
                let canvas_w = ps
                    .upper
                    .w
                    .saturating_sub((cp::POOL_CONTENT_INSET * 2) as u32);
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                for o in &cs.options {
                    o.hash(&mut hasher);
                }
                let sig = DropdownSig {
                    w: canvas_w,
                    full_h: pr.h,
                    x: pr.x as i32,
                    y: pr.y as i32,
                    progress_q: (cs.dropdown_progress * 1000.0).round() as u32,
                    sel_q: (cs.option_sel_f * 64.0).round() as i32,
                    c1: acc_cfg.c1,
                    c2: acc_cfg.c2,
                    opts_hash: hasher.finish(),
                    epoch: cs.epoch,
                };
                if sigs.dropdown.feed(sig) {
                    g.set_slot_dims(
                        crate::gles_present::ChromeSlot::DropdownPanel,
                        canvas_w,
                        pr.h,
                    );
                    let bx = g.slot_canvas(crate::gles_present::ChromeSlot::DropdownPanel);
                    bx.fill(0);
                    t.lock().unwrap().paint_dropdown_panel_layer(
                        bx,
                        canvas_w,
                        pr.h,
                        cs,
                        &pr,
                        (ox, pr.y),
                        acc_cfg,
                    );
                    g.slot_bake(crate::gles_present::ChromeSlot::DropdownPanel);
                }
            }
            // BAR-097 池区拆层烘焙（仅 Upper 平移期上岗，贴死即隐）：
            // ①池框几何层 PoolFx——池高 glide 逐帧只重烘这块池区小画布
            // （LUT 后 ~10ms/帧，替代配置槽 14MB 全页逐帧重烘=ras 28ms
            // 病根）；②下池行层 LowerRowsPan——起步一烘（行内容静止），
            // 合成期 y 位移跟 lower.y glide（渐变锚终点位，贴死帧与
            // 稳态配置槽逐像素一致交接）
            if pan_upper && let (Some(ps), Some(cs), Some(t)) = (pool_snap, cfg_snap, th) {
                let area = crate::ui::dual_pool::pool_area(w, h, bottom_inset);
                let fx_sig = (
                    area.w,
                    area.h,
                    bottom_inset,
                    ps.upper.h,
                    acc_cfg.c1,
                    acc_cfg.c2,
                );
                if sigs.poolfx.feed(fx_sig) {
                    g.set_slot_dims(crate::gles_present::ChromeSlot::PoolFx, area.w, area.h);
                    let px = g.slot_canvas(crate::gles_present::ChromeSlot::PoolFx);
                    // BAR-104：paint_pool_frames_layer 自填页底色，此处不再清 0
                    t.lock()
                        .unwrap()
                        .paint_pool_frames_layer(px, area.w, area.h, area.x, area.y, ps, acc_cfg);
                    g.slot_bake(crate::gles_present::ChromeSlot::PoolFx);
                }
                // 行层终点几何：布局数学同源（下池 = 区 − 上池 − 间距）；
                // 终点高从共享池句柄拿（锁序：本块不持 term/cfg 锁 ✓）。
                // BAR-105：与 PanMove 终点锚共用 final_pool_snap 单一源
                let target_h = crate::ui::dual_pool::dual_pool_handle()
                    .map(|p| p.lock().unwrap().target_upper_h())
                    .unwrap_or(ps.upper.h);
                let lower_final = crate::ui::dual_pool::final_pool_snap(ps, &area, target_h).lower;
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                for r in &cs.rows {
                    r.title.hash(&mut hasher);
                    r.meta.hash(&mut hasher);
                }
                let rows_hash = hasher.finish();
                let lr_sig = (
                    area.w,
                    area.h,
                    bottom_inset,
                    target_h,
                    acc_cfg.c1,
                    acc_cfg.c2,
                    cs.epoch,
                    rows_hash,
                );
                if sigs.lower_rows.feed(lr_sig) && !cs.rows.is_empty() {
                    g.set_slot_dims(
                        crate::gles_present::ChromeSlot::LowerRowsPan,
                        area.w,
                        area.h,
                    );
                    let px = g.slot_canvas(crate::gles_present::ChromeSlot::LowerRowsPan);
                    // BAR-104：paint_lower_rows_layer 自填页底色+自画下池框，此处不再清 0
                    let page_denom =
                        (i64::from(w.saturating_sub(1)) + i64::from(h.saturating_sub(1))).max(1);
                    t.lock().unwrap().paint_lower_rows_layer(
                        px,
                        area.w,
                        area.h,
                        area.x,
                        lower_final.y,
                        &lower_final,
                        &cs.rows,
                        acc_cfg,
                        page_denom,
                    );
                    g.slot_bake(crate::gles_present::ChromeSlot::LowerRowsPan);
                }
            }
        }
        // 文件树槽（§五B 三公民）：同规——画布恒靠泊位（ft_off=0）
        if ft_visible && sigs.filetree.feed((w, h, ime, bar_h, acc_ft.c1, acc_ft.c2)) {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::FileTree);
            px.fill(0);
            crate::termview::paint_ft_page_chrome(px, w, h, bottom_inset, 0, acc_ft);
            g.slot_bake(crate::gles_present::ChromeSlot::FileTree);
        }
        // 解析槽（§五B 四公民·三缘语义）：同规——画布恒靠泊位（pt_off=0）。
        // tmux 插件卡内容随槽同烘焙（2026-09-19 v1）——sig 加插件 epoch
        // （会话表/附着/命名/确认任何变更都必触发重烘焙，漏维 = 鬼影）
        let pt_epoch = parser_snap.map_or(0, |ps| ps.epoch);
        let tunnel_epoch = crate::tunnel::snap().map_or(0, |s| s.lock().unwrap().epoch);
        if pt_visible
            && sigs.parser.feed((
                w,
                h,
                ime,
                bar_h,
                acc_pt.c1,
                acc_pt.c2,
                pt_epoch,
                tunnel_epoch,
            ))
        {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::Parser);
            px.fill(0);
            // 视口化（2026-09-20 用户拍板「卡弹小」）：壳吃 bottom_inset
            // ——键盘在场页环弹小到输入栏带以上，环底 = 页面滚动视口底；
            // 内容布局仍只吃栏带高（BAR-119 只盖不重排），逾视底归键盘
            // 遮盖、逾视顶归页缘裁剪带
            crate::termview::paint_parser_page_chrome(px, w, h, bottom_inset, 0, acc_pt);
            if let Some(psnap) = parser_snap {
                term_arc.lock().unwrap().paint_parser_content(
                    px, w, h,
                    // BAR-119：解析页布局永不吃键盘 inset（只盖不重排）
                    bar_h, // 键盘 inset 只喂滚动窗可视底+页缘裁剪带
                    ime, 0, psnap, acc_pt,
                );
            }
            g.slot_bake(crate::gles_present::ChromeSlot::Parser);
        }
        // AI 文字（每帧实例——消息/滚动/panel_off 逐帧变，永不进烘焙；
        // panel_off 进实例 y=刚体平移，2026-09-05 拍板不变）。别家面板靠泊
        // 在顶时 AI 被整页盖住：零生成零绘制（布局写回暂停，露出后下一帧
        // 自愈——被覆盖面板无手势够得着，眼手同尺不缺这份读数）
        let ai_fully_covered = match z_order[3] {
            Panel::Config => cfg_off == 0 && cfg_draw,
            Panel::FileTree => ft_off == 0 && ft_draw,
            Panel::Parser => pt_off == 0 && pt_draw,
            Panel::Ai => false,
        };
        let (ai_layout, ai_glyphs) = if panel_visible && !ai_fully_covered {
            let term = term_arc.lock().unwrap();
            let (layout, glyphs) = term.ai_page_glyphs(
                w,
                h,
                chat_msgs,
                chat_scroll,
                bottom_inset,
                chat_live,
                panel_off,
            );
            (Some(layout), glyphs)
        } else {
            (None, Vec::new())
        };
        let ras0_us = t_ras.elapsed().as_micros() as u64;

        // 3) AI 文字实例（图集两遍制：misses 补装载 → 重生成；字号类 =
        // GLYPH_SIZE_AI——AI_PAGE_PX/AI_PAGE_LINE_H 常量冻结的代号，
        // off_y 按 AI 行基线折算，ai_text_baseline_off 是唯一尺子）
        let t_gen2 = std::time::Instant::now();
        let mut ai_glyphs_by_page: Vec<Vec<crate::glyph_atlas::GlyphInstance>> = Vec::new();
        if panel_visible && !ai_glyphs.is_empty() {
            let mut inst =
                crate::glyph_atlas::ai_glyphs_to_instances(&ai_glyphs, g.atlas(), |c, font| {
                    Self::ai_slot_of(g.atlas(), c, font)
                });
            if !inst.misses.is_empty() {
                let t = term_arc.lock().unwrap();
                let baseline = t.ai_text_baseline_off();
                for k in &inst.misses {
                    if let Some((fid, m, bmp)) = t.rasterize_for_atlas_px(
                        k.c,
                        crate::termview::AI_PAGE_PX,
                        crate::termview::AI_PAGE_PX,
                    ) {
                        let off_y = crate::termview::ai_glyph_off_y(
                            baseline,
                            m.ymin as f32,
                            m.height as f32,
                        );
                        let key = crate::glyph_atlas::GlyphKey {
                            font: fid,
                            c: k.c,
                            size: crate::glyph_atlas::GLYPH_SIZE_AI,
                        };
                        g.atlas_insert(
                            key,
                            m.width as u32,
                            m.height as u32,
                            &bmp,
                            m.xmin as i16,
                            off_y,
                        );
                    }
                }
                // 闭包内联成临时（调用结束即死）——提升成 let 会横跨
                // atlas_insert 的 &mut g 借用（新工具链 E0502，2026-09-05
                // 手机 chain 咬出；网格路径同款写法）
                let inst2 =
                    crate::glyph_atlas::ai_glyphs_to_instances(&ai_glyphs, g.atlas(), |c, font| {
                        Self::ai_slot_of(g.atlas(), c, font)
                    });
                inst = inst2;
            }
            ai_glyphs_by_page = vec![Vec::new(); g.atlas().pages().len()];
            for gi in inst.glyph {
                let p = gi.page as usize;
                if p < ai_glyphs_by_page.len() {
                    ai_glyphs_by_page[p].push(gi);
                }
            }
        }
        let gen2_us = t_gen2.elapsed().as_micros() as u64;

        // 4) 上层槽（输入栏/光球/放大镜）+ tofu 上报。sig 列全 paint_over
        // 的每个输入（orb_alpha_out 恒 true 不进 sig）；放大镜内容跟终端
        // 网格活（网格变化不进 sig）——拖选期强制重烘焙（与改前每帧
        // 全画布重画等价）
        let mut ras_us = ras0_us;
        let sending = ai_snap.is_some_and(|s| s.ai_running);
        let t_over = std::time::Instant::now();
        let over_dirty = magnifier_at.is_some()
            || sigs.over.feed(OverSig(
                caret_on,
                sending,
                bar_snap.cloned(),
                ai_snap,
                magnifier_at,
                ime,
                w,
                h,
            ));
        if over_dirty {
            let px = g.slot_canvas(crate::gles_present::ChromeSlot::Over);
            px.fill(0);
            let mut term = term_arc.lock().unwrap();
            Self::paint_over(
                &mut **term,
                px,
                w,
                h,
                ime,
                bar_snap,
                sending,
                caret_on,
                ai_snap,
                magnifier_at,
                true,
            );
            g.slot_bake(crate::gles_present::ChromeSlot::Over);
        }
        Self::report_tofu(&mut **term_arc.lock().unwrap());
        ras_us += t_over.elapsed().as_micros() as u64;
        crate::gles_present::STAGE_GEN_US
            .fetch_add(gen_us + gen2_us, std::sync::atomic::Ordering::Relaxed);
        crate::gles_present::STAGE_RAS_US.fetch_add(ras_us, std::sync::atomic::Ordering::Relaxed);

        // 5) 组合呈现（z 序见 present_frame——动者在上裁决两面板上下；
        // panel_off/panel_fade/cfg_off/cfg_fade 只进合成期 placement 与
        // 显影，烘焙物不动。视口推移：基座实例过仿射（恒等早退），
        // 面板 placement 加被压额外位移）
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        crate::glyph_atlas::push_bg_instances(
            &mut bg_inst,
            vpush.dx,
            vpush.dy,
            term_place.2,
            cx,
            cy,
        );
        for page in &mut glyphs_by_page {
            crate::glyph_atlas::push_glyph_instances(
                page,
                vpush.dx,
                vpush.dy,
                term_place.2,
                cx,
                cy,
            );
        }
        // 平移遥测（BAR-092 观测升级：逐帧位置对账，用户终验实报
        // 「新内容不跟随/过冲」——涂装无法自证的合成期，位置先行）
        let pan_comp = Self::pan_composite(cfg_snap, pool_snap, w);
        // BAR-096 拆层位置（合成期唯一位置源）：标签栏层（静止，随
        // cfg_off——十八修语义标签不进平移带）+ 下池光标层（实时缓动位；
        // Page 平移期另给旧代位做带内双代）。越池底/池外不画（与整页
        // 涂装同判据）
        let layered = {
            let mut lp = crate::gles_present::LayeredPlace::default();
            if cfg_visible {
                lp.tabbar = Some((0.0, crate::ui::tab_bar::content_origin().1 as f32));
                if let (Some(ps), Some(cs)) = (pool_snap, cfg_snap) {
                    use crate::ui::cfg_page as cp;
                    // 二十四修：下拉面板层合成位（抽屉有余影即上岗；
                    // 平移期同位——面板不进带，钉在自己的页坐标上）
                    if cs.dropdown_progress > 0.001
                        && let Some((pr, ox)) = Self::cfg_dropdown_panel_geom(th, cs, ps, h)
                    {
                        lp.dropdown = Some((ox as f32, pr.y as f32));
                    }
                    // BAR-097：Upper 平移期池区两层合成位——池框层原点 =
                    // 池区左上（无位移，逐帧烘的就是当前几何）；下池行层
                    // y = 实时 lower.y（烘焙锚在终点，位移量 = 当前−终点
                    // 已含在 draw_y 里），底缘 scissor = 下池内缘底
                    if cs.pan.as_ref().is_some_and(|p| {
                        matches!(p.scope, cp::PanScope::Upper | cp::PanScope::UpperBody)
                    }) {
                        lp.poolfx = Some((ps.upper.x as f32, ps.upper.y as f32));
                        lp.lower_rows = Some((
                            ps.lower.x as f32,
                            ps.lower.y as f32,
                            (ps.lower.y + i64::from(ps.lower.h) - cp::POOL_CONTENT_INSET) as f32,
                        ));
                    }
                    // 跳框模态在 = 光标层不画（BAR 挂账：光标层合成在配置
                    // 槽之上，模态只画进槽画布——不摘会让下池三级框光标
                    // 压盖跳框内容，用户实机目击「下池三级框行叠加在跳框上」）
                    if cs.modal.is_none()
                        && !cs.rows.is_empty()
                        && ps.lower.w > (cp::POOL_CONTENT_INSET * 2) as u32
                    {
                        let stride = (cp::LOWER_ROW_H as i64 + cp::ROW_GAP) as f32;
                        let cx = (ps.lower.x + cp::POOL_CONTENT_INSET) as f32;
                        let cy = ps.lower.y as f32
                            + cp::POOL_CONTENT_INSET as f32
                            + (cs.cursor_row * stride).round();
                        if cy + f32::from(cp::LOWER_ROW_H as u16)
                            <= (ps.lower.y + ps.lower.h as i64) as f32
                        {
                            lp.cursor = Some((cx, cy));
                        }
                        if let Some(pan) = cs.pan.as_ref()
                            && pan.scope == cp::PanScope::Page
                        {
                            let ox = (pan.old.pool.lower.x + cp::POOL_CONTENT_INSET) as f32;
                            let oy = pan.old.pool.lower.y as f32
                                + cp::POOL_CONTENT_INSET as f32
                                + (pan.old.cursor_row * stride).round();
                            lp.cursor_old = Some((ox, oy));
                        }
                    }
                }
            }
            lp
        };
        if let Some(pc) = &pan_comp {
            let (po_v, po_b) = g.slot_flags(crate::gles_present::ChromeSlot::PanOld);
            let (pm_v, pm_b) = g.slot_flags(crate::gles_present::ChromeSlot::PanMove);
            crate::report::report(
                "panc",
                &format!(
                    "t={:.3} old={:+.0} new={:+.0} band=({},{},{},{}) clear={} po_v={} po_b={} pm_v={} pm_b={}",
                    pc.t,
                    pc.old_dx,
                    pc.new_dx,
                    pc.band.0,
                    pc.band.1,
                    pc.band.2,
                    pc.band.3,
                    pc.clear_bg,
                    po_v,
                    po_b,
                    pm_v,
                    pm_b
                ),
            );
        }
        g.present_frame(
            &bg_inst,
            &glyphs_by_page,
            &ai_glyphs_by_page,
            panel_off,
            panel_fade,
            cfg_off,
            cfg_fade,
            ft_off,
            ft_fade,
            pt_off,
            pt_fade,
            z_order,
            term_place,
            ai_extra.dy,
            cfg_extra.dy,
            ft_extra.dy,
            pt_extra.dy,
            pan_comp,
            layered,
        );
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

    /// 配置页首行字段实量宽（十四修动态宽度：触发器/panel 触摸命中
    /// 与涂装同一条 measure_items 尺）。先 snap 取文再量，逐段借还
    /// 不嵌套持锁（锁序 term→cfg_page，倒持 = 死锁）
    fn cfg_row0_text_widths(&self) -> (u32, u32) {
        let Some(page) = crate::ui::cfg_page::cfg_page_handle() else {
            return (0, 0);
        };
        let (lbl, val) = {
            let pg = page.lock().unwrap();
            match pg.snap(crate::report::boot_ms() as u64).upper.first() {
                Some(ur) => (ur.label.clone(), ur.value.clone()),
                None => (String::new(), String::new()),
            }
        };
        match self.term_handle() {
            Some(t) => {
                let t = t.lock().unwrap();
                (t.text_width(&lbl, 36.0), t.text_width(&val, 30.0))
            }
            None => (0, 0),
        }
    }

    /// 下拉 panel 内容最小宽（十七修 BAR-090）：选项最长文实量宽 +
    /// 双侧文内边距——命中与涂装同一条尺（眼手同尺不漏维）。
    /// 锁序同 cfg_row0_text_widths（先 snap 取文再量，不嵌套持锁）
    fn cfg_dropdown_content_w_min(&self) -> u32 {
        let Some(page) = crate::ui::cfg_page::cfg_page_handle() else {
            return 0;
        };
        let opts = {
            let pg = page.lock().unwrap();
            pg.snap(crate::report::boot_ms() as u64).options
        };
        match self.term_handle() {
            Some(t) => {
                let t = t.lock().unwrap();
                opts.iter()
                    .map(|o| t.text_width(o, 36.0))
                    .max()
                    .unwrap_or(0)
                    + crate::ui::cfg_page::FIELD_TEXT_INSET * 2
            }
            None => 0,
        }
    }

    /// 下拉面板层几何（二十四修拆层）：Some((面板全高矩形 pr, 画布
    /// 原点 x = 上池内容左内缘))；None = 行 0 非下拉行/无 term 句柄。
    /// 烘焙与合成 placement 共用本尺（单源）；量宽锁序：cs 是快照
    /// （无页锁），term 锁独立短持。宽度账在播 = 触发器宽吃瞬时值
    /// （面板宽 = max(触发器宽, 内容最小宽) 跟随伸缩）
    fn cfg_dropdown_panel_geom(
        th: &Option<crate::gate::SharedTerm>,
        cs: &crate::ui::cfg_page::CfgPageSnap,
        ps: &crate::ui::dual_pool::DualPoolSnap,
        screen_h: u32,
    ) -> Option<(crate::ui::dual_pool::PoolRect, i64)> {
        use crate::ui::cfg_page as cp;
        let ur = cs.upper.first()?;
        if !ur.is_dropdown {
            return None;
        }
        let t = th.as_ref()?;
        let (lw, vw0, cw) = {
            let tg = t.lock().unwrap();
            let lw = tg.text_width(&ur.label, 36.0);
            let vw0 = tg.text_width(&ur.value, 30.0);
            let cw = cs
                .options
                .iter()
                .map(|o| tg.text_width(o, 36.0))
                .max()
                .unwrap_or(0)
                + cp::FIELD_TEXT_INSET * 2;
            (lw, vw0, cw)
        };
        // 二十四修 §六②：宽度账瞬时值优先（伸缩动画期面板宽跟随）
        let vw = cs.trigger_w.map(|v| v.ceil() as u32).unwrap_or(vw0);
        let tr = cp::trigger_rect(&ps.upper, cs.upper_scroll, true, lw, vw);
        let max_h = screen_h.saturating_sub(tr.y.max(0) as u32 + tr.h + 40);
        let pr = cp::dropdown_panel_rect(
            cs.options.len(),
            &ps.upper,
            max_h,
            cs.upper_scroll,
            true,
            lw,
            vw,
            cw,
        );
        Some((pr, ps.upper.x + cp::POOL_CONTENT_INSET))
    }

    /// 组件池跳框动效预览开着（十四修 §六）：当前 modal 条目是动效
    /// 引擎四件之一 = 帧泵/烘焙 sig 的动画维开关。无 self——
    /// draw_frame_gles（关联函数无 self 接收者）也调
    fn cfg_anim_modal_open() -> bool {
        let Some(page) = crate::ui::cfg_page::cfg_page_handle() else {
            return false;
        };
        let pg = page.lock().unwrap();
        match pg.modal() {
            Some(mi) => crate::ui::comp_registry::COMPONENTS
                .get(mi)
                .is_some_and(|e| crate::ui::comp_registry::preview_is_animated(e.preview)),
            None => false,
        }
    }

    /// 配置页池区/下拉动画活性探针（十五修 §五/§六 帧泵闸）：
    /// 下池光标缓动 / 下拉开合 / 视口平移（BAR-099 状态驱动：账在 =
    /// 活性在，终点帧渲染消费才灭）/ 池高缓动（BAR-095）任一
    /// 未收敛 = true。锁序 term→pool→cfg_page 不倒持嵌套（本探针不
    /// 碰 term，两把短锁先后取，互不嵌套）
    fn cfg_fx_active() -> bool {
        let now = crate::report::boot_ms() as u64;
        if crate::ui::dual_pool::dual_pool_handle()
            .is_some_and(|p| p.lock().unwrap().glide_fx_active(now))
        {
            return true;
        }
        crate::ui::cfg_page::cfg_page_handle().is_some_and(|pg| {
            let g = pg.lock().unwrap();
            g.cursor_fx_active(now)
                || g.dropdown_fx_active(now)
                || g.pan_active(now)
                || g.trig_w_fx_active(now)
        })
    }

    /// 渲染一帧：GLES 双层装配（专线，AI 文字 GPU 化）或 softbuffer 单层
    /// 全 CPU（兜底），非终端模式清紫屏
    fn draw_frame(&mut self) {
        let t0 = std::time::Instant::now(); // 帧耗时画像(自观测第三块)
        // 先拿终端句柄(owned Arc,借用即还),再借 gfx——顺序反了 E0502
        let th = self.term_handle();
        let Some(g) = &mut self.gfx else { return };
        // 配置卡标签栏快照（宪法 §四）：视口宽按真实屏宽逐帧纠（捏合/
        // 旋转后内容带宽度变）；弹簧读数随快照——游标动画帧自带新值
        let mut tab_snap = self.tab_bar.as_ref().map(|b| {
            let mut g2 = b.lock().unwrap();
            if let Some(win) = &self.window {
                g2.set_viewport_w(crate::ui::tab_bar::content_viewport_w(
                    win.inner_size().width,
                ));
            }
            g2.snap(crate::report::boot_ms() as u64)
        });
        // 配置卡双池快照（宪法 §五）：可用区按真实屏尺寸逐帧纠（与标签栏
        // 同规）；底内缘必须含键盘+输入栏带（漏算 = 下池顶穿页环，
        // 2026-09-12 真机实踩）。锁序：先取 term 算栏带高再锁池——gate
        // 倒帧是 term→pool，这里不许反向
        let pool_snap = self.dual_pool.as_ref().map(|p| {
            let view = match (&self.window, &th) {
                (Some(win), Some(t)) => {
                    let sz = win.inner_size();
                    let tg = t.lock().unwrap();
                    let bar_h = Self::current_bar_h(&**tg, self.last_bar_snap.as_ref(), sz.width);
                    (sz.width, sz.height, self.ime_bottom_px + bar_h)
                }
                _ => (720, 1280, 0),
            };
            let mut g3 = p.lock().unwrap();
            g3.set_viewport(view.0, view.1, view.2);
            // 上池内容高（宪法 §五 高度数学钉的输入）：触发器 + 字段行，
            // 三层目录状态核唯一来源（锁序 term→pool→cfg_page 与 gate 同）。
            // BAR-095 分域律：Upper 平移中 = glide 缓动（池高与光标/
            // 平移同钟同曲线——「光标到位池高也到位」）；Page 平移/无
            // 平移 = set 直通（新页池高起步帧就位，贴死零二次动画）。
            // BAR-093 的 pan_freeze 不复辟：glide 是真同步动画，不是
            // 冻结喂入（PanMove 新代行布局与池高无关，带外静物逐帧重
            // 烘跟随，贴死连续）
            if let Some(page) = &self.cfg_page {
                let now = crate::report::boot_ms() as u64;
                // 单次锁内拿两样（锁序 term→pool→cfg_page 不倒持；分
                // 两次锁有挂账竞态窗口）。二十四修：冻结窗口废除——
                // 并发三账同拍起步，glide 恒吃当下起步
                let (h, gliding) = {
                    let pg = page.lock().unwrap();
                    (pg.upper_content_h(), pg.pan_upper_active(now))
                };
                if gliding {
                    g3.glide_upper_content_h(h, now);
                } else {
                    g3.set_upper_content_h(h);
                }
            }
            g3.layout(crate::report::boot_ms() as u64)
        });
        // BAR-096 拆层：标签栏层画布不知屏高/键盘 inset——底线 span——底线 span
        // （池区左右内缘）由壳层补进快照（层画布只画自己那块）
        if let (Some(ts), Some(ps)) = (tab_snap.as_mut(), pool_snap.as_ref()) {
            ts.line_span = Some((ps.upper.x, ps.upper.x + i64::from(ps.upper.w)));
        }
        // 配置页内容快照（三层目录）：涂装/命中同一份（D9；十五修：
        // 吃 now——光标弹簧/下拉进度是时间函数）
        let cfg_now = crate::report::boot_ms() as u64;
        let cfg_snap = self
            .cfg_page
            .as_ref()
            .map(|p| p.lock().unwrap().snap(cfg_now));
        // 解析页 tmux 插件快照（涂装/命中同一份）：锁短——snap 即放
        let parser_snap = self.parser_page.as_ref().map(|p| p.lock().unwrap().snap());
        // BAR-099 终点帧消费（状态驱动帧泵）：本帧若已贴死，渲染的是
        // 钳制后的精确终点态——账随帧消，帧泵下一圈停；活性翻 false
        // 那圈 BAR-098 补帧机制再产一帧回稳态单代（逐像素一致无感）。
        // 与本帧 snap 吃同一 cfg_now：消的账 = 刚画的那帧的账
        if let Some(page) = &self.cfg_page {
            page.lock().unwrap().consume_settled_pan(cfg_now);
        }
        // GLES（2026-09-07 图层槽位版）：网格实例 → 键行槽 → 面板槽
        // （placement 动画）→ AI 文字实例 → 上层槽。槽位置脏烘焙。
        // 关联函数按字段传参，避开 buf 借用 gfx 时动不了 self 的问题
        if let Gfx::Gles(g) = g {
            let mods = self.modifiers.as_ref().map_or(0, |m| m.peek());
            // 光标闪烁相位（聚焦时每半周期翻转要重画——置脏在 poll_input_bar）
            let caret_on = (crate::report::boot_ms() as u64 / crate::input_bar::CARET_BLINK_MS)
                .is_multiple_of(2);
            let chat_msgs = self.ai_chat.as_ref().map(|c| c.snap()).unwrap_or_default();
            let chat_scroll = self.ai_chat.as_ref().map_or(0, |c| c.scroll_offset());
            let chat_live = self.ai_chat.as_ref().is_some_and(|c| c.thinking_live());
            let ai_layout = Self::draw_frame_gles(
                g,
                &th,
                mods,
                caret_on,
                &chat_msgs,
                chat_scroll,
                chat_live,
                self.last_ai_snap,
                self.last_bar_snap.as_ref(),
                self.magnifier_at,
                self.ime_bottom_px,
                &mut self.chrome_inset_px,
                &mut self.layer_sigs,
                tab_snap.as_ref(),
                pool_snap.as_ref(),
                cfg_snap.as_ref(),
                parser_snap.as_ref(),
                self.session_over,
                self.panel_drag.as_ref().and_then(|d| {
                    let off = d.current_offset()?;
                    let p = match d.role()? {
                        crate::ui::panel_drag::DragRole::DismissConfig => {
                            crate::ai_presence::Panel::Config
                        }
                        crate::ui::panel_drag::DragRole::SummonFileTree
                        | crate::ui::panel_drag::DragRole::DismissFileTree => {
                            crate::ai_presence::Panel::FileTree
                        }
                        crate::ui::panel_drag::DragRole::SummonParser
                        | crate::ui::panel_drag::DragRole::DismissParser => {
                            crate::ai_presence::Panel::Parser
                        }
                    };
                    Some((p, off))
                }),
            );
            // 布局写回视口状态机（眼手同尺：手势钳制与渲染同一份布局）
            if let (Some(chat), Some((total, fit))) = (&self.ai_chat, ai_layout) {
                chat.scroll_sync_layout(total, fit);
            }
            // 活性口径（2026-09-16 补 cfg_fx：池区平移/光标/下拉/池高
            // 原不在记账面——anim-cap 点播触发只在 anim_run_start 消费，
            // 池区动画不挂表 = 点播永不消费 = 池区动画无呈现帧仪器。
            // 挂上后池区动画同享节奏账（panel-anim 行）与渲染源采样）
            crate::gles_present::note_anim_frame(
                crate::ui::seam::ai_panel_offset_y_active()
                    || crate::ui::seam::config_panel_offset_x_active()
                    || crate::ui::seam::filetree_panel_offset_x_active()
                    || crate::ui::seam::parser_panel_offset_x_active()
                    || Self::cfg_fx_active(),
                t0.elapsed(),
            );
            crate::gate::note_draw(t0.elapsed()); // 含 present 的全帧耗时
            return;
        }
        // ---- softbuffer 兜底（单层全 CPU，立项书红线永久保留）----
        let (mut buf, w, h) = match g {
            Gfx::Soft { surface, .. } => {
                let b = surface.buffer_mut().expect("取帧缓冲失败");
                let (w, h) = (b.width().get(), b.height().get());
                (b, w, h)
            }
            Gfx::Gles(_) => unreachable!("GLES 已在上面专线处理"),
        };
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
            // 面板栈读数重建（底→顶）+ 配置/文件树/解析 X 偏移过缝（§五B
            // 第三/四/五道缝）：target 只问栈（BAR-084 单源
            // panel_target_and_draw——活性泄漏进 target = 退场回粘）；
            // draw = 在栈或缝/拖拽活跃。无 ui-fx 占槽 = 直通目标值（硬切）。
            // 右缘家屏外 +w（配置/解析）/ 文件树 -w
            use crate::ai_presence::Panel;
            let mut stack_vec: Vec<Panel> = Vec::new();
            if let Some(s) = self.last_ai_snap {
                if let Some(c) = s.covered {
                    stack_vec.push(c);
                }
                if let Some(t) = s.top {
                    stack_vec.push(t);
                }
            }
            let drag = self.panel_drag.as_ref().and_then(|d| {
                let off = d.current_offset()?;
                let p = match d.role()? {
                    crate::ui::panel_drag::DragRole::DismissConfig => Panel::Config,
                    crate::ui::panel_drag::DragRole::SummonFileTree
                    | crate::ui::panel_drag::DragRole::DismissFileTree => Panel::FileTree,
                    crate::ui::panel_drag::DragRole::SummonParser
                    | crate::ui::panel_drag::DragRole::DismissParser => Panel::Parser,
                };
                Some((p, off))
            });
            let drag_cfg = drag.filter(|(p, _)| *p == Panel::Config).map(|(_, o)| o);
            let drag_ft = drag.filter(|(p, _)| *p == Panel::FileTree).map(|(_, o)| o);
            let drag_pt = drag.filter(|(p, _)| *p == Panel::Parser).map(|(_, o)| o);
            let cfg_active = crate::ui::seam::config_panel_offset_x_active() || drag_cfg.is_some();
            let ft_active = crate::ui::seam::filetree_panel_offset_x_active() || drag_ft.is_some();
            let pt_active = crate::ui::seam::parser_panel_offset_x_active() || drag_pt.is_some();
            let (cfg_target, _cfg_draw) = crate::ui::stage::panel_target_and_draw(
                stack_vec.contains(&Panel::Config),
                cfg_active,
                w as f32,
            );
            let (ft_target, _ft_draw) = crate::ui::stage::panel_target_and_draw(
                stack_vec.contains(&Panel::FileTree),
                ft_active,
                -(w as f32),
            );
            let (pt_target, _pt_draw) = crate::ui::stage::panel_target_and_draw(
                stack_vec.contains(&Panel::Parser),
                pt_active,
                w as f32,
            );
            // z 序单源（stage::panel_z_order，BAR-083 动者在上四公民泛化，
            // 同 GLES 路径）
            let z_order = crate::ui::stage::panel_z_order(
                &stack_vec,
                [
                    crate::ui::seam::ai_panel_offset_y_active(),
                    cfg_active,
                    ft_active,
                    pt_active,
                ],
            );
            // 跟手拖拽锁定期旁路缝采样（同 GLES 路径；拖拽偏移=距靠泊
            // 距离，按家折符号）
            let cfg_off = match drag_cfg {
                Some(off) => off as i32,
                None => crate::ui::seam::sample_config_panel_offset_x(
                    cfg_target,
                    crate::report::boot_ms() as u64,
                ) as i32,
            };
            let ft_off = match drag_ft {
                Some(off) => -(off as i32),
                None => crate::ui::seam::sample_filetree_panel_offset_x(
                    ft_target,
                    crate::report::boot_ms() as u64,
                ) as i32,
            };
            let pt_off = match drag_pt {
                Some(off) => off as i32,
                None => crate::ui::seam::sample_parser_panel_offset_x(
                    pt_target,
                    crate::report::boot_ms() as u64,
                ) as i32,
            };
            // 键盘 inset chrome 跟随过缝（ui-base §二 第二道缝）：目标值 =
            // 真实 inset（BAR-006 轮询）；无 ui-fx 占槽 = 直通（硬切）。
            // 采样值写回字段——触摸命中下一拍吃同一份（眼手同尺）。
            // 终端 resize 不在这里：永远吃真实值（pty resize 抖动红线）
            self.chrome_inset_px = crate::ui::seam::sample_chrome_ime_inset(
                self.ime_bottom_px as f32,
                crate::report::boot_ms() as u64,
            )
            .max(0.0) as u32;
            let chat_scroll = self.ai_chat.as_ref().map_or(0, |c| c.scroll_offset());
            let chat_live = self.ai_chat.as_ref().is_some_and(|c| c.thinking_live());
            let ai_layout = Self::rasterize(
                tg.as_deref_mut(),
                mods,
                self.magnifier_at,
                self.chrome_inset_px,
                self.last_ai_snap,
                &chat_msgs,
                chat_scroll,
                chat_live,
                self.last_bar_snap.as_ref(),
                caret_on,
                &mut buf,
                w,
                h,
                panel_off,
                cfg_off,
                ft_off,
                pt_off,
                z_order,
                &mut self.panel_scratch,
                tab_snap.as_ref(),
                pool_snap.as_ref(),
                cfg_snap.as_ref(),
                parser_snap.as_ref(),
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
        // softbuffer 推原生窗（GLES 已在专线内 present_frame + swap）
        buf.present().expect("帧呈现失败");
        crate::gate::note_draw(t0.elapsed()); // 含 present 的全帧耗时
    }
}

/// fx 帧预算同步（BAR-077，2026-09-10 用户拍板：动画节拍跟显示真实刷新
/// 率走——120Hz 屏写死 16ms=60fps 硬钳=「落下拖影」真凶，vsync 账本
/// 实测 110-120Hz 定罪，渲染均耗 3-6ms 证明管线跑得起 120fps）。
/// JNI 直调 MainActivity.displayRefreshHz()（attach 范式同 rec hook）；
/// 读数离谱/查询失败 = 维持旧预算（默认 16ms 保守基线），不许带病进系统。
fn sync_fx_frame_budget(app: &winit::platform::android::activity::AndroidApp) {
    // SAFETY: 同 insets.rs imp——vm_as_ptr/activity_as_ptr 是 android-activity
    // 保证有效的裸指针，attach 回调内即用即弃
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw = app.activity_as_ptr() as jni::sys::jobject;
    let r = vm.attach_current_thread(|env| -> jni::errors::Result<f32> {
        let act = unsafe { jni::objects::JObject::from_raw(env, raw) };
        env.call_method(
            &act,
            jni::jni_str!("displayRefreshHz"),
            jni::jni_sig!(() -> float),
            &[],
        )?
        .f()
    });
    match r {
        // 24Hz 下限：低于此不是正常显示屏（读数串味），不许进系统
        Ok(hz) if (24.0..=480.0).contains(&hz) => {
            let v = crate::ui::fx_spring::set_frame_budget_ms((1000.0 / hz).round() as u64);
            crate::report::report("fx", &format!("帧预算跟随刷新率: {hz:.1}Hz → {v}ms"));
        }
        Ok(hz) => crate::report::report("fx", &format!("刷新率读数离谱({hz})——维持旧预算")),
        Err(e) => crate::report::report("fx", &format!("刷新率查询失败: {e}——维持旧预算")),
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        crate::gate::note_foreground(true); // 看门狗出假(BAR-036)
        // BAR-077：fx 帧预算跟真实刷新率走（每次 resumed 一问，系统设置
        // 切 60/120 档跟手）——写死 16ms 在 120Hz 屏上 = 落下拖影
        if let Some(app) = &self.android_app {
            sync_fx_frame_budget(app);
        }
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("KFM-NA");
        let window = Arc::new(el.create_window(attrs).expect("创建窗口失败"));
        let gfx = Self::init_gfx(&window);
        self.gfx = Some(gfx);
        // 新 Gfx = 新纹理 = 烘焙物全死：判定器全失效，下帧全量重烘焙
        // （漏了这步 = 后台往返后键行/面板/上层消失，BAR 级视觉事故）
        self.layer_sigs.invalidate_all();
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
            // BAR-112：后台往返后强制重算一次几何（幂等——尺寸没变不抖 pty
            // 3049 判等闸；sync_kb_shift 重算零成本）：挂起期漏算的
            // inset/kb_shift 在此归位，系统不发 Resized 事件也不怕
            let sz = window.inner_size();
            self.apply_window_size(sz.width, sz.height);
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
                if let Some(g) = &mut self.gfx {
                    match g {
                        Gfx::Soft { surface, .. } => {
                            if let (Some(w), Some(h)) =
                                (NonZeroU32::new(sz.width), NonZeroU32::new(sz.height))
                            {
                                surface.resize(w, h).expect("surface resize 失败");
                            }
                        }
                        // EGL 窗表面随系统自调，只同步 CPU 帧缓冲尺寸
                        Gfx::Gles(g) => g.set_size(sz.width, sz.height),
                    }
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
                            // 解析页命名态分流（tmux 插件，与 JNI 链同尺）
                            if self
                                .parser_page
                                .as_ref()
                                .is_some_and(|p| p.lock().unwrap().naming_active())
                            {
                                if let Some(p) = &self.parser_page {
                                    p.lock().unwrap().naming_push(&text);
                                }
                                self.dirty = true;
                                return;
                            }
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
            self.check_bar_repeat(); // 方向键长按 → 连发（2026-09-19 拍板）
            self.poll_ai_presence(); // AI 外显快照比对(注入/到期也要画帧)
            // tmux 插件（2026-09-19）：执行排水 + 解析页在栈且从未查询
            // 时的首查（Idle 一次性闸——Loading/Ready 都不会在这反复发）
            let exec_done = self
                .parser_exec
                .as_ref()
                .and_then(|(kind, rx)| match rx.try_recv() {
                    Ok(res) => Some((*kind, res)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Some((*kind, Err("执行线程断线".to_string())))
                    }
                });
            if let Some((kind, res)) = exec_done {
                self.parser_exec = None;
                self.parser_exec_done(kind, res);
            }
            let parser_docked = self
                .last_ai_snap
                .is_some_and(|s| s.top == Some(crate::ai_presence::Panel::Parser));
            // BAR-115：靠泊上升沿 = 开页/重开页 → 重列会话（Idle 一次性闸
            // 只补首查——Ready 后重开页不刷，服务器侧 tmux 增删永远看不见，
            // 「开页自动刷」设计口径的实际破洞；exec 在途闸在 refresh 内）
            let parser_dock_edge = parser_docked && !self.parser_docked_prev;
            self.parser_docked_prev = parser_docked;
            if (parser_dock_edge
                || (parser_docked
                    && self.parser_page.as_ref().is_some_and(|p| {
                        matches!(
                            p.lock().unwrap().status(),
                            crate::ui::parser_page::Status::Idle
                        )
                    })))
                && self.endpoint_exec_ok()
                && self.parser_exec.is_none()
            {
                self.parser_refresh();
            }
            // BAR-117：隧道「不可用→可用」上升沿踢活跃死会话重孵——重孵
            // 链是死亡事件驱动的，末次重孵撞 TCP refused（隧道未起）被 5s
            // 闸压住后再无死亡事件 = 链断 remote_dead 卡死；传输恢复必须
            // 回头踢壳层（裁决纯函数 tunnel::usable_edge_kick，A 档钉）
            self.poll_tunnel_kick();
            // 服务卡数据面（2026-09-20）：可见性喂轮询器（不可见不轮
            // 纪律），快照换代 → 脏帧重烘卡面
            crate::svc_health::set_visible(parser_docked);
            if crate::svc_health::take_dirty() {
                self.dirty = true;
            }
            self.poll_input_bar(); // 输入栏快照比对(注入/分流也要画帧)
            // 采样缝动画帧时钟(ui-base §四 按需启停):缝上有活跃动画
            // 且距上帧 ≥16ms 才置脏——无动画零额外帧,有动画 ≤60fps
            if crate::ui::fx_spring::fx_frame_due(crate::report::boot_ms() as u64) {
                self.dirty = true;
            }
            // 组件池跳框动效预览帧泵(十四修 §六)+池区/下拉动画帧泵
            // (十五修 §五/§六):动画在播且配置页在顶 → ≤60fps 置脏
            // (BAR-097 补咬: 拆层+LUT 后中帧 3-6ms, 33ms 节流从帧贵时代
            // 的自我保护变成帧率天花板——250ms 动画上限 7 帧, 降 16ms);
            // (ConfigSig 时间桶/光标行号/下拉进度三维触发槽重烘焙);
            // 收敛/盖住零帧——无动画零成本纪律同 §四。
            // 收敛补终帧(ui-base §四,2026-09-14 用户实机抓下拉余影):
            // 活性翻 false 那圈必须再脏一帧擦终态,否则屏幕定格在收敛
            // 前最后一帧(进度≈0.02 的一行黑余影)——闸 = prev||curr。
            // BAR-098(2026-09-16 真机残影定案):补终帧不许被 33ms 节流
            // 吃掉——活性期刚画过一帧(<33ms)就被跳过 = 末帧永是 t<1
            // 的平移中帧(带内新代 PanMove@new_dx + 光标层冻在残余偏
            // 移,真机「选中行残字/池框探出」病根),判定收纯函数
            // fx_spring::cfg_fx_frame_due(钉在 fx_spring_spec)
            let cfg_fx = Self::cfg_anim_modal_open() || Self::cfg_fx_active();
            static CFG_FX_PREV: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            let cfg_fx_prev = CFG_FX_PREV.swap(cfg_fx, std::sync::atomic::Ordering::Relaxed);
            if self.last_ai_snap.and_then(|s| s.top) == Some(crate::ai_presence::Panel::Config)
                && (cfg_fx || cfg_fx_prev)
            {
                static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let now = crate::report::boot_ms() as u64;
                if crate::ui::fx_spring::cfg_fx_frame_due(
                    cfg_fx_prev,
                    cfg_fx,
                    now.saturating_sub(LAST.load(std::sync::atomic::Ordering::Relaxed)),
                ) {
                    LAST.store(now, std::sync::atomic::Ordering::Relaxed);
                    self.dirty = true;
                }
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
    // P2 软件内实录钩子（2026-09-08）：gate rec-req-ms → JNI 甩
    // MainActivity.startRecordingFromGate。android-activity 0.6 不露 jni
    // 面（只有 vm_as_ptr/activity_as_ptr 裸指针）——自己 from_raw 拼
    // jni 0.22 类型；GlobalRef 一次建立长期持有；attach 是回调式 API
    // （0.22 无 guard 形态），Java 侧 runOnUiThread 接管（授权弹窗只能
    // Activity 发起）
    {
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
        let gref = vm
            .attach_current_thread(|env| {
                let act = unsafe {
                    jni::objects::JObject::from_raw(env, app.activity_as_ptr() as *mut _)
                };
                env.new_global_ref(&act)
            })
            .expect("MainActivity GlobalRef 建立失败（attach+全局引用）");
        crate::gate::register_rec_hook(Box::new(move |ms: i32| {
            let r = vm.attach_current_thread(|env| {
                env.call_method(
                    &gref,
                    jni::jni_str!("startRecordingFromGate"),
                    jni::jni_sig!((int) -> void),
                    &[jni::objects::JValue::Int(ms)],
                )
                .map(|_| ())
            });
            if let Err(e) = r {
                crate::report::report_sync("rec", &format!("JNI startRecordingFromGate 失败: {e}"));
            }
        }));
        // SPKE-web（2026-09-12）：gate web-req → JNI 甩 MainActivity
        // .startWebViewFromGate(URL)。vm/gref 已被 rec 闭包 move 走，
        // 另建一对（from_raw 不持所有权，GlobalRef 再 new 一枚）
        let vm2 = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
        let gref2 = vm2
            .attach_current_thread(|env| {
                let act = unsafe {
                    jni::objects::JObject::from_raw(env, app.activity_as_ptr() as *mut _)
                };
                env.new_global_ref(&act)
            })
            .expect("MainActivity GlobalRef#2 建立失败");
        crate::gate::register_web_hook(Box::new(move |url: String| {
            let r = vm2.attach_current_thread(|env| {
                let ju = env.new_string(&url)?;
                env.call_method(
                    &gref2,
                    jni::jni_str!("startWebViewFromGate"),
                    jni::jni_sig!((java.lang.String) -> void),
                    &[jni::objects::JValue::Object(&ju)],
                )
                .map(|_| ())
            });
            if let Err(e) = r {
                crate::report::report_sync("web", &format!("JNI startWebViewFromGate 失败: {e}"));
            }
        }));
    }
    let mut app_handler = App {
        android_app: Some(app.clone()),
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
