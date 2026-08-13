//! android_app.rs — Android 壳（B 档：平台胶水，冒烟钉防退化）
//!
//! 尖刺 1 第一步：winit（NativeActivity）+ wgpu 空窗，紫屏（KFM 紫 #8B5CF6）。
//! 目的：一次踩掉两颗已知雷——wgpu 的 Android Vulkan 兼容性、APK 包体基线。
//! 验收见 docs/active/立项.md 尖刺五条（包体 <10MB / 冷启动 <1s 手机实拍）。

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

/// 对照实验开关（2026-08-13 下午）：false = wgpu 全摘裸窗组，见 resumed 注释
const ENABLE_GFX: bool = false;

struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    reported_first_events: bool,
}

impl App {
    /// 初始化 wgpu（实例/表面/适配器/设备），配置表面为当前窗口尺寸
    fn init_gfx(window: &Arc<Window>) -> Gfx {
        // 后端锁 GLES（2026-08-13 实拍六次定案）：本机 Mali-G720 Immortalis
        // Vulkan 驱动（r44p1）与 wgpu 25 相冲——死亡点在 instance→present 间
        // 随机漂移（adapter/configure/present 各死过）、挂起与原生崩交替、
        // 无 Rust panic，非代码逻辑病。Mali 的 GLES 驱动是另一套成熟栈，
        // 清屏/终端渲染绰绰有余。Vulkan 留作后查（换机或驱动升级再试）。
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..Default::default()
        });
        crate::report::report("boot", "wgpu instance 建成");
        let surface = instance
            .create_surface(window.clone())
            .expect("创建 wgpu surface 失败");
        crate::report::report("boot", "wgpu surface 建成");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("无可用 Vulkan/GLES 适配器（雷 1 爆点）");
        log::info!("wgpu 适配器: {:?}", adapter.get_info());
        crate::report::report("boot", &format!("wgpu 适配器: {:?}", adapter.get_info()));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("kfm-na"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("请求 wgpu 设备失败");
        crate::report::report("boot", "wgpu device 到手");
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        crate::report::report(
            "boot",
            &format!("caps 到手: formats={}", caps.formats.len()),
        );
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        // configure 死锁看门狗（2026-08-13 实拍：caps 到手后 26s 静默，进程活着
        // ——疑 configure/present 挂起而非崩溃）。3s 未置旗即回传死锁警报。
        static CONFIG_DONE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if !CONFIG_DONE.load(std::sync::atomic::Ordering::Relaxed) {
                crate::report::report("hang", "configure 3 秒未返回——疑 Mali 驱动死锁");
            }
        });
        surface.configure(&device, &config);
        CONFIG_DONE.store(true, std::sync::atomic::Ordering::Relaxed);
        crate::report::report("boot", "surface 配置完——开始渲染");
        Gfx {
            surface,
            device,
            queue,
            config,
        }
    }

    /// 清屏一帧（KFM 紫）
    fn draw_clear(g: &Gfx) {
        let frame = match g.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                // 表面丢失（如息屏回来）：重配，本帧跳过
                g.surface.configure(&g.device, &g.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = g
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear"),
            });
        {
            let _rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.545, // #8B5CF6 KFM 紫
                            g: 0.361,
                            b: 0.965,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        }
        g.queue.submit([enc.finish()]);
        frame.present();
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
        // 对照实验（2026-08-13 下午，ENABLE_GFX=false）：wgpu 全摘。
        // 六次实拍死亡点横跨 event loop 构建/adapter/surface/configure、
        // Vulkan 与 GLES 两后端都死——根本不是图形问题。裸 winit 窗 + 心跳
        // 判决：心跳停 = 进程真死（精确到秒）；心跳在跳但用户看到「闪退」
        // = Activity 被系统杀、进程活着，病根在 ROM/manifest 层。
        if ENABLE_GFX {
            let gfx = Self::init_gfx(&window);
            self.gfx = Some(gfx);
        }
        self.window = Some(window);
        crate::report::report("boot", "启动完成（裸窗对照组）");
        log::info!("KFM-NA 壳启动完成");
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) => {
                if let Some(g) = &mut self.gfx {
                    g.config.width = sz.width.max(1);
                    g.config.height = sz.height.max(1);
                    g.surface.configure(&g.device, &g.config);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &self.gfx {
                    Self::draw_clear(g);
                }
            }
            _ => {}
        }
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
    crate::report::report("boot", &format!("run_app 返回: {:?}", result));
    // 事件循环一生只能建一次（winit RecreationAttempt）。NativeActivity 销毁后
    // 进程常被 ROM 保留，不自杀则下次点开 android_main 重跑必 panic
    // （2026-08-13 实拍「白退」次生病灶）。活动结束 = 进程跟着死，重来即全新。
    std::process::exit(0);
}
