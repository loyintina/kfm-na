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
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
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
        surface.configure(&device, &config);
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
        let gfx = Self::init_gfx(&window);
        self.gfx = Some(gfx);
        self.window = Some(window);
        crate::report::report("boot", "启动完成——紫屏应已亮");
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
    // 飞鸽传书：panic 直报服务器（手机无 adb 通路，见 report.rs 头注）
    std::panic::set_hook(Box::new(|info| {
        crate::report::report("panic", &info.to_string());
    }));
    crate::report::report("boot", "android_main 进入");
    log::info!("KFM-NA android_main 进入");
    let event_loop = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("创建事件循环失败");
    crate::report::report("boot", "event loop 建成");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("事件循环崩溃");
    crate::report::report("boot", "run_app 返回（正常不该到这）");
}
