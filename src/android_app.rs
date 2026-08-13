//! android_app.rs — Android 壳（B 档：平台胶水，冒烟钉防退化）
//!
//! 渲染路线定案（2026-08-13，用户拍板）：**软渲染 softbuffer**。
//! 背景：本机 GPU 驱动栈（Mali-G720 Immortalis r44p1 + OriginOS）与 wgpu
//! 双后端（Vulkan/GLES）随机原生暴毙——六次实拍，死亡点在 adapter/surface/
//! configure 间漂移、零 Rust panic，非代码逻辑病；裸 winit 窗对照组稳定。
//! 终端负载（字符网格 + 光标）本就是 CPU 教科书级场景，softbuffer 零驱动
//! 依赖、行为确定。GPU 路线留档后查（git 历史 ENABLE_GFX/wgpu 时代）。

use std::num::NonZeroU32;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

/// KFM 紫（softbuffer 像素格式 XRGB）
const KFM_PURPLE: u32 = 0x008B_5CF6;

type SoftContext = softbuffer::Context<Arc<Window>>;
type SoftSurface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

struct Gfx {
    _context: SoftContext,
    surface: SoftSurface,
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    reported_first_events: bool,
}

impl App {
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

    /// 清屏一帧（KFM 紫）
    fn draw_purple(g: &mut Gfx) {
        let mut buf = g.surface.buffer_mut().expect("取帧缓冲失败");
        buf.fill(KFM_PURPLE);
        buf.present().expect("帧呈现失败");
        // 首帧呈现里程碑：紫屏真亮了才算雷 1 排除
        static FIRST_PRESENT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !FIRST_PRESENT.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::report::report("boot", "首帧 present 完成——紫屏应已亮");
        }
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
        self.window = Some(window);
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
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &mut self.gfx {
                    Self::draw_purple(g);
                }
            }
            _ => {}
        }
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        crate::report::report("death", "suspended——Activity 被挂起（退后台/被销毁前奏）");
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        crate::report::report_sync("death", "exiting——事件循环即将退出");
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
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
    // 心跳：进程存活的客观判决——心跳停 = 进程真死（精确到秒）；
    // 心跳在跳但用户看到「闪退」= Activity 被系统杀、进程活着（病根完全不同）
    std::thread::spawn(|| {
        let mut n = 0u32;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            n += 1;
            crate::report::report("alive", &format!("心跳 {}", n));
        }
    });
    log::info!("KFM-NA android_main 进入");
    let event_loop = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("创建事件循环失败");
    crate::report::report("boot", "event loop 建成");
    let mut app = App::default();
    let result = event_loop.run_app(&mut app);
    // 同步直报：async 入队后立刻 exit(0) 会吃掉这行（此前历次「静默消失」
    // 的嫌疑——死亡现场被自己的 exit(0) 毁尸灭迹）
    crate::report::report_sync("death", &format!("run_app 返回: {:?}", result));
    // 事件循环一生只能建一次（winit RecreationAttempt）。NativeActivity 销毁后
    // 进程常被 ROM 保留，不自杀则下次点开 android_main 重跑必 panic
    // （2026-08-13 实拍「白退」次生病灶）。活动结束 = 进程跟着死，重来即全新。
    std::process::exit(0);
}
