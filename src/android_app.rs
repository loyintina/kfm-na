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
use std::sync::mpsc::{Receiver, Sender};

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

/// 帧缓冲探针状态：0=等首个 output，1=探针已上膛（下一帧数非背景像素），2=已报
static FRAME_PROBE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// 终端模式开关：true = 启动即进终端画面；false = 紫屏 + echo 冒烟对照组
const TERMINAL_MODE: bool = true;

type SoftContext = softbuffer::Context<Arc<Window>>;
type SoftSurface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

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
    reported_first_events: bool,
    // ---- TERMINAL_MODE 状态 ----
    /// 终端实例：插件工厂产出（term-alacritty），调用方持有的长寿命 mutable
    /// 状态（设计页 §7）——含 scrollback，跨插件生命周期存活
    term: Option<Box<dyn TermEmu>>,
    /// 活跃会话的出向/入向通道（L1 双会话：默认本地 PTY，ws 远程在待机槽）
    outbound: Option<Sender<TermCmd>>,
    /// RTT 探针：最后一次击键送出的墙钟时刻（下个 output 到达时结算）
    last_input_at: Option<std::time::Instant>,
    event_rx: Option<EventRx>,
    /// 待机会话槽（L1）：(出向, 入向, 名字)。待机期间事件在 mpsc 里积压，
    /// 切入时一口气排干补屏（v1 接受；长时间积压的内存账暂不细算）
    standby: Option<(Sender<TermCmd>, EventRx, &'static str)>,
    /// 活跃会话名（"local" / "remote"；诊断与切换横幅用）
    active_name: &'static str,
    /// 最近一次下发的网格尺寸（切换会话时给新活跃方补发 Resize）
    last_grid: (u32, u32),
    /// 有新输出/尺寸变化待渲染
    dirty: bool,
    /// 会话终了（exited/failed）后定格最后一屏，出向不再发
    session_over: bool,
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
                self.event_rx = Some(l.events);
                self.outbound = Some(l.outbound);
                self.active_name = "local";
                self.standby = Some((r.outbound, r.events, "remote"));
            }
            // 兜底：本地挂了远程顶上（单会话退化，行为同 L1 前）
            (None, Some(r)) => {
                crate::report::report_sync("term", "本地会话断裂——退化纯远程模式");
                self.event_rx = Some(r.events);
                self.outbound = Some(r.outbound);
                self.active_name = "remote";
            }
            (Some(l), None) => {
                crate::report::report_sync("term", "远程连接断裂——纯本地模式");
                self.event_rx = Some(l.events);
                self.outbound = Some(l.outbound);
                self.active_name = "local";
            }
            (None, None) => {
                crate::report::report_sync("term", "双会话全灭——本屏无会话");
            }
        }

        // 建终端：经基座取终端工厂；build 失败 = 字体全灭走 Err（裁决 3，非插件失败）
        crate::report::report("boot", &format!("基座+插件装载完成 +{}ms", boot_ms()));
        let Some((tv, font_path, cjk_path)) = (match base.ctx().get::<dyn TermEmuFactory>() {
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
        crate::report::report("term", &format!("字体加载自 {font_path} +{}ms", boot_ms()));
        match &cjk_path {
            Some(p) => crate::report::report("term", &format!("CJK 备用字体: {p}")),
            None => crate::report::report("term", "CJK 备用字体全灭——中文画 tofu"),
        }
        // （BAR-021：诊断脚手架已拆——候选体检/目录普查每个冷启动全量解析
        // 44MB×2+32MB 巨物，是启动慢的最大单块成本；探测链本身也已退役，
        // 生产字体编译期内嵌。需要排查时从 git 历史恢复）
        // 字体探针：加载成功 ≠ 能出字形，西文/中文各探一针（真机判卷「不见字」）
        for c in ['M', '中'] {
            let (w, h, ink) = tv.font_probe(c);
            crate::report::report("term", &format!("字体探针 '{c}': {w}x{h} ink={ink}"));
        }
        crate::report::report("term", &format!("TermView 建成 +{}ms", boot_ms()));
        self.term = Some(tv);
        self.base = Some(base);

        // 首发尺寸：Opened 前 outbound 会被 conn 层缓存，绑定后补发
        let size = window.inner_size();
        self.apply_window_size(size.width, size.height);
        self.dirty = true;
    }

    /// 窗口 px 尺寸 → cols/rows → Term resize + terminal-resize 出向。
    /// 可用区域 = 窗口 - 四周边距（BAR-005）- 真实软键盘 inset（BAR-006，
    /// JNI 轮询，insets.rs）- 快捷键行高（BAR-017，Rust 自绘常驻让位）
    fn apply_window_size(&mut self, w: u32, h: u32) {
        let Some(term) = &mut self.term else { return };
        let (cw, ch) = term.cell_size();
        let usable_w = w.saturating_sub(2 * termview::MARGIN_X);
        let usable_h = h.saturating_sub(
            termview::MARGIN_TOP
                + termview::MARGIN_Y
                + self.ime_bottom_px
                + crate::keybar::HEIGHT_PX,
        );
        let (cols, rows) = termview::grid_dims(usable_w, usable_h, cw, ch);
        term.resize_cells(cols, rows);
        self.last_grid = (cols, rows);
        if !self.session_over
            && let Some(tx) = &self.outbound
        {
            let _ = tx.send(TermCmd::Resize { cols, rows });
        }
        self.dirty = true;
    }

    /// 会话切换（L1）：Ctrl-] 触达——活跃槽与待机槽互换，给新活跃方补发
    /// 当前网格尺寸，横幅直接喂进终端网格（不走对端）。待机期积压的事件
    /// 由下一圈 drain_terminal_events 一口气排干补屏
    fn switch_session(&mut self) {
        let (Some(tx_a), Some(rx_a)) = (self.outbound.take(), self.event_rx.take()) else {
            return;
        };
        let Some((tx_s, rx_s, name_s)) = self.standby.take() else {
            // 没待机方：把槽位放回去，装作没发生
            self.outbound = Some(tx_a);
            self.event_rx = Some(rx_a);
            return;
        };
        let name_a = self.active_name;
        self.standby = Some((tx_a, rx_a, name_a));
        self.outbound = Some(tx_s.clone());
        self.event_rx = Some(rx_s);
        self.active_name = name_s;
        let (cols, rows) = self.last_grid;
        let _ = tx_s.send(TermCmd::Resize { cols, rows });
        if let Some(t) = &mut self.term {
            let banner =
                format!("\r\n\x1b[36m[kfm-na → {name_s} 会话（Ctrl-] 切回 {name_a}）]\x1b[0m\r\n");
            t.feed(banner.as_bytes());
        }
        self.session_over = false; // 新活跃方生死未知,先复活输出面
        crate::report::report("term", &format!("会话切换: {name_a} → {name_s}"));
        self.dirty = true;
    }

    /// 抽干 ws 线程送来的会话事件（about_to_wait 每圈调）
    fn drain_terminal_events(&mut self) {
        let Some(rx) = &self.event_rx else { return };
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                SessionEvent::Opened { session_id } => {
                    crate::report::report(
                        "term",
                        &format!("会话 opened: {session_id} +{}ms", boot_ms()),
                    );
                }
                SessionEvent::Output { data } => {
                    // 首 output 预览：诊断「黑屏等提示符」——提示符何时到、内容是什么
                    static FIRST_OUTPUT: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if !FIRST_OUTPUT.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        crate::report::report_sync(
                            "term",
                            &format!(
                                "首 output 到达 +{}ms: {:?}",
                                boot_ms(),
                                &data[..data.len().min(120)]
                            ),
                        );
                        // 上膛帧缓冲探针（一次性）：下一帧数非背景像素（见 draw_frame）。
                        // 注意只能在首 output 上膛——此前写成每个 output 都上膛，
                        // 探针变成常驻连发，报告通道被刷屏（11:31 实拍事故）
                        FRAME_PROBE.store(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    // RTT 探针（输入延迟判卷）：击键→回显 output 的墙钟耗时，采样 5 发。
                    // 数字说话：延迟到底在网络往返还是自家管线（2026-08-13 用户实拍问）
                    if let Some(t) = self.last_input_at.take() {
                        static RTT_N: std::sync::atomic::AtomicU8 =
                            std::sync::atomic::AtomicU8::new(0);
                        let n = RTT_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if n < 5 {
                            crate::report::report_sync(
                                "rtt",
                                &format!("击键→回显: {}ms", t.elapsed().as_millis()),
                            );
                        }
                    }
                    if let Some(term) = &mut self.term {
                        term.feed(data.as_bytes());
                        self.dirty = true;
                    }
                }
                SessionEvent::Exited { code } => {
                    crate::report::report_sync("term", &format!("会话 exited: code={code}"));
                    self.session_over = true;
                }
                SessionEvent::Failed { message } => {
                    crate::report::report_sync("term", &format!("会话 failed: {message}"));
                    self.session_over = true;
                }
            }
        }
    }

    /// 排干 Java 皮（KfmInputConnection/快捷键行）经 JNI 注入的输入——
    /// 中文落字从这里进终端（NativeActivity 无 InputConnection 的补丁，
    /// 链路见 ime_queue.rs 文件头）。键码在排干侧按当下光标模式翻序列
    /// （模式位只有这里的 Term 知道，keymap.rs 吃 app_cursor 参数）
    fn drain_ime_inject(&mut self) {
        let items = crate::ime_queue::global().drain();
        if items.is_empty() || self.session_over {
            return;
        }
        static FIRST_INJECT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !FIRST_INJECT.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::report::report("ime", "首个 JNI IME 文字注入");
        }
        let app_cursor = self.term.as_ref().is_some_and(|t| t.app_cursor_mode());
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
            if let Some(tx) = &self.outbound {
                let _ = tx.send(TermCmd::Input(bytes));
                sent = true;
            }
        }
        if sent {
            self.last_input_at = Some(std::time::Instant::now());
            // IME 落字 = 用户输入：滚回底部贴最新输出
            if let Some(t) = &mut self.term {
                t.scroll_to_bottom();
            }
        }
    }

    /// 键盘事件 → 终端输入字节（尖刺极简映射，IME 见文件头留白）
    fn handle_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed || self.session_over {
            return;
        }
        let bytes: Option<String> = match &event.logical_key {
            Key::Named(NamedKey::Enter) => Some("\r".into()),
            Key::Named(NamedKey::Backspace) => Some("\x7f".into()),
            Key::Named(NamedKey::Tab) => Some("\t".into()),
            Key::Named(NamedKey::Escape) => Some("\x1b".into()),
            _ => event.text.as_ref().map(|t| t.to_string()),
        };
        if let (Some(bytes), Some(tx)) = (bytes, &self.outbound)
            && !bytes.is_empty()
        {
            let _ = tx.send(TermCmd::Input(bytes));
            self.last_input_at = Some(std::time::Instant::now());
            // 打字了就是要看现在——滚回底部贴最新输出
            if let Some(t) = &mut self.term {
                t.scroll_to_bottom();
            }
        }
    }

    /// 渲染一帧：终端模式画网格，非终端模式清紫屏
    fn draw_frame(&mut self) {
        let Some(g) = &mut self.gfx else { return };
        let mut buf = g.surface.buffer_mut().expect("取帧缓冲失败");
        if TERMINAL_MODE {
            if let Some(term) = &mut self.term {
                let (w, h) = (buf.width().get(), buf.height().get());
                term.render_into(&mut buf, w, h);
                // 快捷键行（BAR-017：Rust 自绘覆盖层，画在终端网格之上；
                // 键盘 inset 之上——键盘弹起时行跟着上浮）。
                // 修饰键位读 input.modifiers 服务（input-ime 方案 A）
                let mods = self.modifiers.as_ref().map_or(0, |m| m.peek());
                term.render_keybar(&mut buf, w, h, self.ime_bottom_px, mods);
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
                static FIRST_TERM_FRAME: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !FIRST_TERM_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::report::report("term", &format!("首终端帧渲染完成 +{}ms", boot_ms()));
                }
                // 帧缓冲探针：首个 output 后的那一帧，数非背景像素传回——
                // 光标块独占 ≈288px，提示符字形真画上则数千。真机判卷「不见字」
                // 的最后一环：字形到底进没进帧缓冲（2026-08-13）
                if FRAME_PROBE
                    .compare_exchange(
                        1,
                        2,
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    let non_bg = buf.iter().filter(|&&p| p != termview::DEFAULT_BG).count();
                    crate::report::report_sync(
                        "term",
                        &format!("output 后首帧非背景像素: {non_bg}"),
                    );
                }
            } else {
                buf.fill(KFM_PURPLE); // 字体全灭的降级画面：紫屏 + 已有上报
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
        buf.present().expect("帧呈现失败");
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _el: &ActiveEventLoop, cause: winit::event::StartCause) {
        if !self.reported_first_events {
            self.reported_first_events = true;
            crate::report::report("boot", &format!("new_events 首次: {:?}", cause));
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        crate::report::report("boot", &format!("resumed——开始建窗口 +{}ms", boot_ms()));
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
        crate::report::report("boot", &format!("启动完成 +{}ms", boot_ms()));
        log::info!("KFM-NA 壳启动完成");
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
                        let cell_h = self
                            .term
                            .as_ref()
                            .map(|t| t.cell_size().1)
                            .unwrap_or(crate::termview::CELL_H);
                        self.touch_scroll = Some(crate::scroll::TouchScroll::new(
                            touch.location.y,
                            f64::from(cell_h),
                        ));
                    }
                    TouchPhase::Moved => {
                        if self.bar_touch.is_some() {
                            return; // 快捷键行手势：不支持拖动
                        }
                        let Some(tracker) = &mut self.touch_scroll else {
                            return;
                        };
                        let lines = tracker.moved(touch.location.y);
                        if lines == 0 {
                            return;
                        }
                        let Some(t) = &mut self.term else { return };
                        if t.mouse_report_active() {
                            // BAR-016②：对端开了鼠标上报（tmux/kimicode 等全屏
                            // TUI）——alt screen 没有本地历史可滚，翻成 SGR 滚轮
                            // 事件发 PTY，让对方滚自己的视图
                            let (cw, ch) = t.cell_size();
                            let col = (touch.location.x as u32 / cw + 1).max(1);
                            let row = (touch.location.y as u32 / ch + 1).max(1);
                            if let Some(tx) = &self.outbound {
                                // 每次事件按行数发滚轮 tick，封顶防一次猛拖雪崩
                                for _ in 0..lines.unsigned_abs().min(10) {
                                    let _ = tx.send(TermCmd::Input(crate::scroll::wheel_seq(
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
                        // Preedit（拼音候选中）尖刺期不上屏，只留痕一次
                        Ime::Preedit(_, _) => {
                            static PREEDIT_SEEN: std::sync::atomic::AtomicBool =
                                std::sync::atomic::AtomicBool::new(false);
                            if !PREEDIT_SEEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                                crate::report::report("ime", "首个 Preedit（候选中）");
                            }
                        }
                        Ime::Commit(text) => {
                            if !self.session_over
                                && let Some(tx) = &self.outbound
                            {
                                let _ = tx.send(TermCmd::Input(text));
                                self.last_input_at = Some(std::time::Instant::now());
                                // IME 落字 = 用户输入：滚回底部贴最新输出
                                if let Some(t) = &mut self.term {
                                    t.scroll_to_bottom();
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if TERMINAL_MODE && !self.dirty {
                    return; // 忙轮询泵下的空圈：不重绘（省电的最后底线）
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
    // BAR-022 归因钉：report_sync 是同步 HTTP（连 2s 读 3s × 至多 3 次重试），
    // 若此行 ~3000ms 则首格直报是窃贼；若 ~0ms 则 EventLoop::build 是窃贼
    crate::report::report("boot", &format!("event loop 开工 +{}ms", boot_ms()));
    let event_loop = EventLoop::builder()
        .with_android_app(app.clone())
        .build()
        .expect("创建事件循环失败");
    crate::report::report("boot", &format!("event loop 建成 +{}ms", boot_ms()));
    let mut app_handler = App {
        android_app: Some(app),
        ..Default::default()
    };
    let result = event_loop.run_app(&mut app_handler);
    // 同步直报：async 入队后立刻 exit(0) 会吃掉这行（此前历次「静默消失」
    // 的嫌疑——死亡现场被自己的 exit(0) 毁尸灭迹）
    crate::report::report_sync("death", &format!("run_app 返回: {:?}", result));
    // 事件循环一生只能建一次（winit RecreationAttempt）。NativeActivity 销毁后
    // 进程常被 ROM 保留，不自杀则下次点开 android_main 重跑必 panic
    // （2026-08-13 实拍「白退」次生病灶）。活动结束 = 进程跟着死，重来即全新。
    std::process::exit(0);
}
