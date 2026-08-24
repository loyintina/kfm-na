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
//! - 重绘泵是忙轮询（about_to_wait 无条件 request_redraw）：ws 线程事件经
//!   mpsc 送达，Android 上没用 EventLoopProxy 唤醒（可靠性未验证），busy loop
//!   是最朴素的活路。电池不友好，正式版要换 proxy 唤醒
//! - 键盘只翻可打印字符 + Enter/Backspace/Tab/Esc；中文 IME 走 Java 皮
//!   （KfmInputConnection.commitText → JNI → ime_queue → drain_ime_inject，
//!   2026-08-13 定案——winit native-activity 后端零 Ime 事件代码，平台层
//!   补不了，只能 Java 层接 InputConnection）

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

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

/// 终端模式开关：true = 启动即进终端画面；false = 紫屏 + echo 冒烟对照组
const TERMINAL_MODE: bool = true;

/// 开局上机提示（2026-08-20 用户实拍：快捷键是 app 层的，shell 里 help
/// 看不见它们，要「至少一个提示」）。青色标题 + 灰说明，只 feed 视图
/// 不进 PTY；滚屏可回看，每次冷启动印一次
const HELP_BANNER: &str = "\x1b[36m── kfm-na 就绪 ──\x1b[0m\r\n\
\x1b[90m切换会话: CTRL+] 本地⇄远程 · 触摸: 点按唤键盘 / 滑动滚屏 / 双指缩放字号\x1b[0m\r\n\
\x1b[90m长按选词: 拖动扩选 / 按住边界精调(带放大镜) / 单击复制 · HOME/END 跳首尾 · PGUP/PGDN 翻页\x1b[0m\r\n\
\x1b[90m快捷键行: CTRL/ALT/SHIFT 点一下粘住再敲字母\x1b[0m\r\n\
\x1b[90m本地 HOME: Android/data/dev.kfm.na/files(文件管理器可见,随便读写)\x1b[0m\r\n";

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

/// 连接 → 主事件循环的会话事件通道（工厂内部建桥，跨线程走 mpsc；设计页 §6：
/// 服务数据通道，非插件事件）
type EventRx = Receiver<SessionEvent>;

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
    /// 活跃会话的入向事件通道（切换时与待机槽的 rx 互换——
    /// 出向归 router，入向归壳，同一方法内同步换，不许分开动）
    event_rx: Option<EventRx>,
    /// 待机入向槽（L1）：(入向, 名字)。待机期间事件在 mpsc 里积压，
    /// 切入时一口气排干补屏（v1 接受；长时间积压的内存账暂不细算）
    standby: Option<(EventRx, &'static str)>,
    /// 最近一次下发的网格尺寸（切换会话时给新活跃方补发 Resize）
    last_grid: (u32, u32),
    /// 有新输出/尺寸变化待渲染
    dirty: bool,
    /// 会话终了（exited/failed）后定格最后一屏，出向不再发
    session_over: bool,
    /// 会话健康牌 ×2（断线重连）：字段语义见 SessHealth
    health_local: SessHealth,
    health_remote: SessHealth,
    /// 待机输出缓存（每圈抽干待机 rx 攒的 Output;切换时补屏,待机死亡
    /// 换新通道时清掉——旧 shell 遗物不喂新会话）
    standby_buf: std::collections::VecDeque<SessionEvent>,
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
    /// 即 moved 撤 armed；RedrawRequested 每圈查时间戳（忙轮询泵福利，
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
    /// 插件基座（连接 provider 设计页）：持有它 = 插件服务活着
    base: Option<Base>,
    /// input.modifiers 服务句柄（input-ime 插件，方案 A：修饰键状态挂服务键）
    modifiers: Option<Arc<crate::keybar::ModifierState>>,
    /// ime.insets 服务句柄（键盘高度/强弹；生产 = JniInsets）
    ime_insets: Option<Arc<dyn crate::insets::ImeInsets>>,
}

impl App {
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

    /// 长按计时（忙轮询泵福利：RedrawRequested 每圈查时间戳，免定时器）：
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
        // 全量重建 = 生死簿重开（断线重连的健康牌/待机缓存一并归零）
        self.health_local = SessHealth::default();
        self.health_remote = SessHealth::default();
        self.standby_buf.clear();

        // exec 探针(L2/L3 总开关,exec_probe.rs):私有目录 exec 放行与否
        // 决定 busybox/apt 生态路线。冷启动一次,结果走飞鸽传书。
        // 后台线程跑(2026-08-21 实测:同步跑吃 2283ms 占 init 96%,大头是
        // report_sync 阻塞 HTTP;探针结果 v1 只上报不分支,没资格堵启动)
        if let Some(app) = &self.android_app
            && let Some(dir) = app.internal_data_path()
        {
            std::thread::spawn(move || {
                crate::exec_probe::run(&dir);
            });
        }

        // 插件基座：终端模拟器 + 连接 provider（边界手术第一/二刀）——
        // 「用哪个终端芯、连哪、怎么连」都不归主循环；工厂是服务，实例归调用方。
        // 瞬时返回契约预算 50ms 是 harness 政策(G5 归层:cordis-na 默认关,
        // 这里显式开启,规格书 §4.3)
        let base = Base::new(vec![PluginEntry {
            id: crate::plugins::conn_provider_ws::PLUGIN_NAME,
            disabled: false,
            config: Some(Box::new(|| {
                Arc::new(ConnConfig::default()) as Arc<dyn std::any::Any + Send + Sync>
            })),
        }])
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
                self.event_rx = Some(l.events);
                self.standby = Some((r.events, "remote"));
                self.install_router(router);
            }
            // 兜底：本地挂了远程顶上（单会话退化，行为同 L1 前）
            (None, Some(r)) => {
                crate::report::report_sync("term", "本地会话断裂——退化纯远程模式");
                self.event_rx = Some(r.events);
                self.install_router(crate::session_router::SessionRouter::new(
                    r.outbound, "remote",
                ));
            }
            (Some(l), None) => {
                crate::report::report_sync("term", "远程连接断裂——纯本地模式");
                self.event_rx = Some(l.events);
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

        // 上机提示(L1 实拍后用户要「至少一个提示」):app 级快捷键 shell
        // 看不见,开局直接印在网格上(只 feed 视图,不进 PTY 不污染会话)。
        // 每次冷启动印一次;滚屏可回看
        if let Some(t) = self.term_handle() {
            t.lock().unwrap().feed(HELP_BANNER.as_bytes());
        }

        // 首发尺寸：Opened 前 outbound 会被 conn 层缓存，绑定后补发
        let size = window.inner_size();
        self.apply_window_size(size.width, size.height);
        self.dirty = true;
    }

    /// 窗口 px 尺寸 → cols/rows → Term resize + terminal-resize 出向。
    /// 可用区域 = 窗口 - 四周边距（BAR-005）- 真实软键盘 inset（BAR-006，
    /// JNI 轮询，insets.rs）- 快捷键行高（BAR-017，Rust 自绘常驻让位）。
    /// 顶带跟当前格高走（margin_top：捏合缩放后格高可变，2026-08-21）
    fn apply_window_size(&mut self, w: u32, h: u32) {
        let Some(term) = self.term_handle() else {
            return;
        };
        let (cw, ch) = term.lock().unwrap().cell_size();
        let usable_w = w.saturating_sub(2 * termview::MARGIN_X);
        let usable_h = h.saturating_sub(
            termview::margin_top(ch)
                + termview::MARGIN_Y
                + self.ime_bottom_px
                + crate::keybar::HEIGHT_PX,
        );
        let (cols, rows) = termview::grid_dims(usable_w, usable_h, cw, ch);
        term.lock().unwrap().resize_cells(cols, rows);
        self.last_grid = (cols, rows);
        if !self.session_over
            && let Some(r) = self.router_handle()
        {
            r.lock().unwrap().send(TermCmd::Resize { cols, rows });
        }
        self.dirty = true;
    }

    /// 会话切换（L1）：Ctrl-] 触达——router 换出向活跃槽，壳同步换入向
    /// rx（同一方法内完成），给新活跃方补发当前网格尺寸，横幅直接喂进
    /// 终端网格（不走对端）。待机期缓存的输出先补屏（standby_buf，
    /// 待机 rx 每圈已被 drain_terminal_events 抽干）。切入死会话 →
    /// 立即重连（用户在场，断线重连 2026-08-21）
    fn switch_session(&mut self) {
        // 锁即取即还——后面补屏循环要借 self 别处
        let Some((name_a, name_s)) = self
            .router_handle()
            .and_then(|r| r.lock().unwrap().switch())
        else {
            return; // 没待机方：装作没发生(或没路由装配)
        };
        let (Some(rx_a), Some((rx_s, _))) = (self.event_rx.take(), self.standby.take()) else {
            crate::report::report_sync("term", "切换时入向槽残缺——装配 bug");
            return;
        };
        self.standby = Some((rx_a, name_a));
        self.event_rx = Some(rx_s);
        // 待机期缓存的输出补屏：死会话的遗屏也喂——用户看得到「死前最后
        // 画面」,比重连后的白屏亲切;活的会话更必须(输出连续)
        let buf = std::mem::take(&mut self.standby_buf);
        for ev in buf {
            if let SessionEvent::Output { data } = ev
                && let Some(t) = self.term_handle()
            {
                t.lock().unwrap().feed(data.as_bytes());
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
    /// 静默吞输入）：给死会话 spawn 新实例，router 换心脏 + 壳换入向通道。
    /// 服务器侧 PTY 随 WS 断即杀（kfmv4 ws-server killAll），重连必然是
    /// 新 shell——横幅明示，旧现场引导 tmux attach。本地 PTY 死亡（shell
    /// exit）同路重孵
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
            self.event_rx = Some(h.events);
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
            self.standby = Some((h.events, name));
            self.standby_buf.clear(); // 旧 shell 遗物不喂新会话
        }
        crate::report::report("term", &format!("会话重连: {name} 重孵"));
        self.dirty = true;
    }

    /// 抽干会话事件（about_to_wait 每圈调）：活跃槽全量处理；待机槽也每圈
    /// 抽——Output 进缓存（切换时补屏），生死事件即时登记（不抽的话死讯
    /// 压到切换才爆，重连晚一整拍；2026-08-21 实拍 WS 退后台被掐的坑）
    fn drain_terminal_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.event_rx {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        let active = self
            .router_handle()
            .map_or("", |r| r.lock().unwrap().active_name());
        for ev in events {
            self.on_session_event(active, ev, true);
        }
        let mut sbuf = Vec::new();
        if let Some((rx, _)) = &self.standby {
            while let Ok(ev) = rx.try_recv() {
                sbuf.push(ev);
            }
        }
        let sname = self.standby.as_ref().map_or("", |s| s.1);
        for ev in sbuf {
            if matches!(ev, SessionEvent::Output { .. }) {
                self.standby_buf.push_back(ev);
            } else {
                self.on_session_event(sname, ev, false);
            }
        }
    }

    /// 单事件分派：name = 来源槽位名（健康牌按名记账，空名 = 无路由装配,
    /// 只动 session_over 不记账），is_active = 是否当前可见方
    fn on_session_event(&mut self, name: &'static str, ev: SessionEvent, is_active: bool) {
        match ev {
            SessionEvent::Opened { session_id } => {
                if !name.is_empty() {
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
            SessionEvent::Output { data } => {
                if let Some(term) = self.term_handle() {
                    term.lock().unwrap().feed(data.as_bytes());
                    self.dirty = true;
                }
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
        if name.is_empty() {
            return;
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

    /// 光栅化一帧的内容（终端网格 + 快捷键行 + 放大镜 + tofu 上报）进
    /// 任意像素缓冲。关联函数按字段传参，避开 buf 借用 self.gfx 时动
    /// 不了 self 的问题。后台离屏倒帧不走这里——值守线程(screendump)
    /// 只画终端网格本体，快捷键行/放大镜是 UI 装帧，不在后台视野里
    fn rasterize(
        term: Option<&mut Box<dyn TermEmu>>,
        mods: u8,
        magnifier_at: Option<(f64, f64)>,
        ime_bottom_px: u32,
        buf: &mut [u32],
        w: u32,
        h: u32,
    ) {
        if let Some(term) = term {
            term.render_into(buf, w, h);
            // 快捷键行（BAR-017：Rust 自绘覆盖层，画在终端网格之上；
            // 键盘 inset 之上——键盘弹起时行跟着上浮）。
            // 修饰键位读 input.modifiers 服务（input-ime 方案 A）
            term.render_keybar(buf, w, h, ime_bottom_px, mods);
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
        }
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
        // 先拿终端句柄(owned Arc,借用即还),再借 gfx——顺序反了 E0502
        let th = self.term_handle();
        let Some(g) = &mut self.gfx else { return };
        let mut buf = g.surface.buffer_mut().expect("取帧缓冲失败");
        let (w, h) = (buf.width().get(), buf.height().get());
        if TERMINAL_MODE {
            crate::gate::note_frame_size(w, h); // 给后台倒帧值守记账
            let mods = self.modifiers.as_ref().map_or(0, |m| m.peek());
            let mut tg = th.as_ref().map(|a| a.lock().unwrap());
            Self::rasterize(
                tg.as_mut().map(|t| &mut **t),
                mods,
                self.magnifier_at,
                self.ime_bottom_px,
                &mut buf,
                w,
                h,
            );
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
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
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
                if !TERMINAL_MODE {
                    return;
                }
                match touch.phase {
                    TouchPhase::Started => {
                        static FIRST_TOUCH: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !FIRST_TOUCH.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            crate::report::report("ime", "首个触摸进 handler（派发活着）");
                        }
                        // 起点在快捷键行带上 → 这手势归行（不滚屏不唤键盘）
                        // BAR-018：判定尺与渲染/hit 一致——减去键盘 inset，
                        // 否则键盘弹起时行带浮在 inset 上方，这里却认屏底
                        let in_bar = self.window.as_ref().is_some_and(|w| {
                            crate::keybar::in_bar(
                                touch.location.y,
                                w.inner_size().height,
                                self.ime_bottom_px,
                            )
                        });
                        if in_bar {
                            self.bar_touch = Some((touch.location.x, touch.location.y));
                            return;
                        }
                        // 终端区指头登记（keybar 带上的不进来）
                        self.touches
                            .push((touch.id, touch.location.x, touch.location.y));
                        // 双指都在终端区 → 捏合缩放：挂起滚动/点按/长按
                        // （touch_scroll/press 清掉，残余指头抬手前不接管任何手势）
                        if self.touches.len() == 2
                            && self.bar_touch.is_none()
                            && self.pinch.is_none()
                        {
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
                            && let Some(end) = self.term_handle().and_then(|t| {
                                t.lock()
                                    .unwrap()
                                    .hit_boundary(touch.location.x, touch.location.y)
                            })
                        {
                            self.sel_drag = Some(end);
                            self.magnifier_at = Some((touch.location.x, touch.location.y));
                            crate::report::report("ime", &format!("边界按住: {end:?}"));
                            return;
                        }
                        // 单指：记按压（长按计时，RedrawRequested 里查）；
                        // 选择态下不建滚动机——拖动 = 扩选
                        self.press = Some(Press {
                            at: std::time::Instant::now(),
                            x: touch.location.x,
                            y: touch.location.y,
                            moved: false,
                            long_fired: false,
                        });
                        if !selecting {
                            let cell_h = self
                                .term_handle()
                                .map(|t| t.lock().unwrap().cell_size().1)
                                .unwrap_or(crate::termview::CELL_H);
                            self.touch_scroll = Some(crate::scroll::TouchScroll::new(
                                touch.location.y,
                                f64::from(cell_h),
                            ));
                        }
                    }
                    TouchPhase::Moved => {
                        // 指头坐标跟新（捏合测距用）
                        for t in &mut self.touches {
                            if t.0 == touch.id {
                                t.1 = touch.location.x;
                                t.2 = touch.location.y;
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
                        if self.bar_touch.is_some() {
                            return; // 快捷键行手势：不支持拖动
                        }
                        // 边界拖动：端点跟手指走（跨行/历史区换算在
                        // move_selection_end），放大镜跟着触点浮
                        if let Some(end) = self.sel_drag {
                            if let Some(t) = self.term_handle() {
                                t.lock().unwrap().move_selection_end(
                                    end,
                                    touch.location.x,
                                    touch.location.y,
                                );
                            }
                            self.magnifier_at = Some((touch.location.x, touch.location.y));
                            self.dirty = true;
                            return;
                        }
                        // 过阈值撤长按 armed（选择态/滚动态同一把尺）
                        if let Some(p) = &mut self.press
                            && ((touch.location.x - p.x).abs() >= crate::scroll::TAP_SLOP_PX
                                || (touch.location.y - p.y).abs() >= crate::scroll::TAP_SLOP_PX)
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
                                t.lock()
                                    .unwrap()
                                    .extend_selection(touch.location.x, touch.location.y);
                            }
                            self.dirty = true;
                            return;
                        }
                        let Some(tracker) = &mut self.touch_scroll else {
                            return;
                        };
                        let lines = tracker.moved(touch.location.y);
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
                            let col = (touch.location.x as u32 / cw + 1).max(1);
                            let row = (touch.location.y as u32 / ch + 1).max(1);
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
                        self.touches.retain(|t| t.0 != touch.id);
                        // 捏合收尾：任一指抬起即结束，缩放比写盘 + [zoom] 上报。
                        // 残余指头不接管滚动/点按（touch_scroll/press 进捏合时已清）
                        if self.pinch.take().is_some() {
                            self.persist_zoom();
                            return;
                        }
                        // 快捷键行手势：抬手命中发键（Cancelled 不发）
                        if self.bar_touch.take().is_some() {
                            // BAR-018 诊断：进得了这个分支 = Started 的 in_bar
                            // 判定活着；hit 落空也会留痕（坐标+inset 三数）
                            crate::report::report(
                                "ime",
                                &format!(
                                    "快捷键行抬手 ({},{}), inset={}",
                                    touch.location.x, touch.location.y, self.ime_bottom_px
                                ),
                            );
                            if touch.phase != TouchPhase::Ended {
                                return;
                            }
                            let Some(w) = &self.window else { return };
                            let s = w.inner_size();
                            let Some(kd) = crate::keybar::hit(
                                touch.location.x,
                                touch.location.y,
                                s.width,
                                s.height,
                                self.ime_bottom_px,
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
                            if tap && touch.phase == TouchPhase::Ended {
                                self.copy_selection();
                            }
                            return;
                        }
                        let was_tap = self.touch_scroll.take().is_some_and(|t| t.was_tap());
                        if was_tap && let Some(w) = &self.window {
                            w.set_ime_allowed(true);
                            if let Some(insets) = &self.ime_insets {
                                insets.force_show();
                            }
                            crate::report::report("ime", "点按唤出软键盘");
                        }
                    }
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
                if TERMINAL_MODE {
                    // 长按计时：忙轮询泵下每圈查时间戳（≥500ms 未动 → 选词）
                    self.check_long_press();
                    if !self.dirty {
                        return; // 忙轮询泵下的空圈：不重绘（省电的最后底线）
                    }
                }
                self.dirty = false;
                self.draw_frame();
            }
            _ => {}
        }
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
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

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if TERMINAL_MODE {
            self.drain_terminal_events();
            self.drain_ime_inject();
            self.poll_ime_inset();
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
        // 事件循环心跳（10s 节流）：忙轮询泵下它在跳 = 循环活着，
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
        if let Some(w) = &self.window {
            w.request_redraw();
        }
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
    std::panic::set_hook(Box::new(|info| {
        crate::report::report("panic", &info.to_string());
    }));
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
