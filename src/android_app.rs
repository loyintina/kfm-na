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
use std::sync::mpsc::{self, Receiver, Sender};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

use crate::conn::TermCmd;
use crate::session::SessionEvent;
use crate::termview::{self, TermView};

/// KFM 紫（softbuffer 像素格式 XRGB）
const KFM_PURPLE: u32 = 0x008B_5CF6;

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

/// ws 线程 → 主事件循环的会话事件桥（inbound 闭包在 ws 线程跑，跨线程走 mpsc）
type EventRx = Receiver<SessionEvent>;

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    reported_first_events: bool,
    // ---- TERMINAL_MODE 状态 ----
    term: Option<TermView>,
    outbound: Option<Sender<TermCmd>>,
    /// RTT 探针：最后一次击键送出的墙钟时刻（下个 output 到达时结算）
    last_input_at: Option<std::time::Instant>,
    event_rx: Option<EventRx>,
    /// 有新输出/尺寸变化待渲染
    dirty: bool,
    /// 会话终了（exited/failed）后定格最后一屏，出向不再发
    session_over: bool,
    /// 真实软键盘底部 inset（px，JNI 轮询得来，BAR-006）。0 = 未弹/未知
    ime_bottom_px: u32,
    /// 上次 JNI 轮询时刻（500ms 节流）
    last_inset_poll: Option<std::time::Instant>,
    /// AndroidApp 句柄（JNI 用；android_main 里 clone 进来）
    android_app: Option<winit::platform::android::activity::AndroidApp>,
}

impl App {
    /// JNI 轮询真实键盘高度（500ms 节流）：winit 的 Ime::Enabled/Disabled 在
    /// 本机从未触发（全日志零条），事件驱动是死路，轮询才是活路（BAR-006）。
    /// 值变了才 resize + 上报——resize 会抖动服务器 pty，不能跟着轮询抖
    fn poll_ime_inset(&mut self) {
        let now = std::time::Instant::now();
        if let Some(t) = self.last_inset_poll {
            if now.duration_since(t) < std::time::Duration::from_millis(500) {
                return;
            }
        }
        self.last_inset_poll = Some(now);
        let Some(app) = &self.android_app else { return };
        // None = 查询失败：维持旧值不抖动
        let Some(px) = crate::insets::query_ime_bottom(app) else {
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
        crate::report::report("boot", "softbuffer 表面建成");
        let size = window.inner_size();
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            surface.resize(w, h).expect("surface resize 失败");
        }
        Gfx {
            _context: context,
            surface,
        }
    }

    /// 终端模式初始化：建 TermView + spawn 常驻会话 + 首发 resize
    fn init_terminal(&mut self, window: &Arc<Window>) {
        // BAR-004 后台往返重开会话的路径：旧会话的死亡标记必须清掉，
        // 否则键盘/IME 输入被 session_over 挡死，新会话成了哑巴
        self.session_over = false;
        let Some((tv, font_path, cjk_path)) =
            termview::build_from_candidates(termview::FONT_CANDIDATES)
        else {
            crate::report::report_sync("term", "字体候选全灭——TermView 建不成");
            return;
        };
        crate::report::report("term", &format!("字体加载自 {font_path}"));
        match &cjk_path {
            Some(p) => crate::report::report("term", &format!("CJK 备用字体: {p}")),
            None => crate::report::report("term", "CJK 备用字体全灭——中文画 tofu"),
        }
        // 候选体检（一次性）：每个主/CJK 候选一行判定结论——「为什么偏偏
        // 选中它」不再靠猜（12:09 实录：DroidSansMono 在目录里却落选，原因未知）
        static DIAGNOSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !DIAGNOSED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            for path in termview::FONT_CANDIDATES {
                crate::report::report("term", &termview::diagnose_candidate(path));
            }
            for path in termview::CJK_FONT_CANDIDATES {
                crate::report::report(
                    "term",
                    &format!("[cjk] {}", termview::diagnose_candidate(path)),
                );
            }
        }
        // 字体探针：加载成功 ≠ 能出字形，西文/中文各探一针（真机判卷「不见字」）
        for c in ['M', '中'] {
            let (w, h, ink) = tv.font_probe(c);
            crate::report::report("term", &format!("字体探针 '{c}': {w}x{h} ink={ink}"));
        }
        // 字体目录普查（一次性）：真机 CJK/等宽候选排查——NotoSansCJK.ttc 空光栅
        // 已实锤（BAR-002），Roboto 比例字体（BAR-003）。335 个字体全量上报必然
        // 截断（11:55 实拍只见 A-B 开头），按关键词过滤出候选相关的
        if let Ok(rd) = std::fs::read_dir("/system/fonts") {
            const KEYS: &[&str] = &[
                "mono", "Mono", "cjk", "CJK", "noto", "Noto", "droid", "Droid", "misans", "MiSans",
                "vivo", "Vivo", "harmony", "Harmony", "SC", "fallback", "Fallback",
            ];
            let mut hits: Vec<String> = rd
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| KEYS.iter().any(|k| n.contains(k)))
                .collect();
            hits.sort();
            let list: String = hits.join(",").chars().take(800).collect();
            crate::report::report("term", &format!("字体候选 {} 个: {list}", hits.len()));
        }
        crate::report::report("term", "TermView 建成");
        self.term = Some(tv);

        // 常驻会话：command=None = 交互 shell；inbound 事件经 mpsc 桥回主循环
        let (event_tx, event_rx) = mpsc::channel::<SessionEvent>();
        let outbound =
            crate::conn::spawn_terminal_session("ws://8.145.46.182/kfmv4/ws", None, move |ev| {
                // 主循环死了发送失败：吞掉——ws 线程绝不为上报陪葬
                let _ = event_tx.send(ev);
            });
        self.event_rx = Some(event_rx);
        self.outbound = Some(outbound);

        // 首发尺寸：Opened 前 outbound 会被 conn 层缓存，绑定后补发
        let size = window.inner_size();
        self.apply_window_size(size.width, size.height);
        self.dirty = true;
    }

    /// 窗口 px 尺寸 → cols/rows → Term resize + terminal-resize 出向。
    /// 可用区域 = 窗口 - 四周边距（BAR-005）- 真实软键盘 inset（BAR-006，
    /// JNI 轮询，insets.rs）
    fn apply_window_size(&mut self, w: u32, h: u32) {
        let Some(term) = &mut self.term else { return };
        let (cw, ch) = term.cell_size();
        let usable_w = w.saturating_sub(2 * termview::MARGIN_X);
        let usable_h =
            h.saturating_sub(termview::MARGIN_TOP + termview::MARGIN_Y + self.ime_bottom_px);
        let (cols, rows) = termview::grid_dims(usable_w, usable_h, cw, ch);
        term.resize_cells(cols, rows);
        if !self.session_over {
            if let Some(tx) = &self.outbound {
                let _ = tx.send(TermCmd::Resize { cols, rows });
            }
        }
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
                    crate::report::report("term", &format!("会话 opened: {session_id}"));
                }
                SessionEvent::Output { data } => {
                    // 首 output 预览：诊断「黑屏等提示符」——提示符何时到、内容是什么
                    static FIRST_OUTPUT: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if !FIRST_OUTPUT.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        crate::report::report_sync(
                            "term",
                            &format!("首 output 到达: {:?}", &data[..data.len().min(120)]),
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

    /// 排干 Java 皮（KfmInputConnection）经 JNI 注入的 IME 文字——
    /// 中文落字从这里进终端（NativeActivity 无 InputConnection 的补丁，
    /// 链路见 ime_queue.rs 文件头）
    fn drain_ime_inject(&mut self) {
        let texts = crate::ime_queue::global().drain();
        if texts.is_empty() || self.session_over {
            return;
        }
        static FIRST_INJECT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !FIRST_INJECT.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::report::report("ime", "首个 JNI IME 文字注入");
        }
        if let Some(tx) = &self.outbound {
            for text in texts {
                let _ = tx.send(TermCmd::Input(text));
            }
            self.last_input_at = Some(std::time::Instant::now());
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
        if let (Some(bytes), Some(tx)) = (bytes, &self.outbound) {
            if !bytes.is_empty() {
                let _ = tx.send(TermCmd::Input(bytes));
                self.last_input_at = Some(std::time::Instant::now());
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
                    crate::report::report("term", "首终端帧渲染完成");
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
        crate::report::report("boot", "resumed——开始建窗口");
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
        crate::report::report("boot", "启动完成");
        log::info!("KFM-NA 壳启动完成");
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                crate::report::report("death", "CloseRequested——窗口被要求关闭");
                el.exit();
            }
            WindowEvent::Resized(sz) => {
                if let Some(g) = &mut self.gfx {
                    if let (Some(w), Some(h)) =
                        (NonZeroU32::new(sz.width), NonZeroU32::new(sz.height))
                    {
                        g.surface.resize(w, h).expect("surface resize 失败");
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
            // 触摸唤出软键盘（winit Android：set_ime_allowed(true) 即 show soft keyboard）
            WindowEvent::Touch(touch) => {
                if TERMINAL_MODE && touch.phase == TouchPhase::Started {
                    if let Some(w) = &self.window {
                        w.set_ime_allowed(true);
                        static IME_REQ: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !IME_REQ.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            crate::report::report("ime", "触摸唤出软键盘");
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
                            if !self.session_over {
                                if let Some(tx) = &self.outbound {
                                    let _ = tx.send(TermCmd::Input(text));
                                    self.last_input_at = Some(std::time::Instant::now());
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
    // 飞鸽传书：先起后台冲洗线程（必须在 report_sync 之前——sync 失败时
    // 入队要有人接，否则第一格静默丢，06:42 实拍已踩），再挂 panic 钩子
    crate::report::start_flusher();
    // 第一格同步直报：早死进程等不到后台线程（有界 2s）。
    // 能收到这行 = 死在 android_main 内部；收不到 = 死在更前（加载/manifest）。
    crate::report::report_sync("boot", "android_main 进入");
    std::panic::set_hook(Box::new(|info| {
        crate::report::report("panic", &info.to_string());
    }));
    // ws 冒烟（尖刺切片 3 对照组）：连服务器 terminal-pty 跑 echo 闭环，
    // 判卷 = field-reports.log 的 [ws] 四格。TERMINAL_MODE=true 时让位给
    // 常驻会话（resumed 里 spawn），冒烟路径保留作回退开关
    if !TERMINAL_MODE {
        crate::conn::spawn_smoke("ws://8.145.46.182/kfmv4/ws", "echo KFM-NA-WS-OK");
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
    crate::report::report("boot", "event loop 建成");
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
