//! gpu-spike — GPU 渲染期 0 复活尖刺①：wgpu 30 × NativeActivity
//! （gpu-render.md §四；对照 2026-08-13 判词 a44f936 逐项复跑）
//!
//! 复跑的死亡点：instance / surface / adapter / device / configure /
//! present——当年 wgpu 25 在这段随机原生暴毙（零 Rust panic），裸
//! winit 窗对照组稳定。本尖刺外加当年没有的两件事：
//! - **suspend/resume 循环**：suspended 拆 surface+窗，resumed 重建
//!   （老尖刺不拆——surface 生命周期正是嫌疑区）；
//! - **三角渲染**：不止清屏，shader/pipeline/draw 全链路压上。
//!
//! 判卷证据 = 里程碑 + 每秒心跳经飞鸽传书回传（report.rs 同通道，
//! POST /kfmv4/api/na-report → 服务器 field-reports.log）——原生
//! 暴毙零 panic 时，里程碑断点即死亡行。验收：15 分钟存活零原生
//! 崩溃 + suspend/resume 10 循环。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

static BOOT_T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static FRAMES: AtomicU64 = AtomicU64::new(0);

/// 飞鸽传书（report.rs 同款通道，spike 简化版：一线程一报，2s 超时，
/// best-effort 吞错——上报通道自己绝不能炸成二次事故）
/// 协议：JSON {stage, msg}（files.ts /na-report 只认这两个字段——
/// 2026-09-04 教训：text/plain 裸发会落成空 [?] 行，内容全丢）
fn spike_report(msg: &str) {
    let ms = BOOT_T0.get().map_or(0, |t| t.elapsed().as_millis());
    let esc = msg.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!("{{\"stage\":\"gpu-spike\",\"msg\":\"+{ms}ms {esc}\"}}");
    std::thread::spawn(move || {
        use std::io::Write;
        let Ok(mut s) = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], 8021)),
            std::time::Duration::from_secs(2),
        ) else {
            return;
        };
        let req = format!(
            "POST /kfmv4/api/na-report HTTP/1.1\r\nHost: 127.0.0.1:8021\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = s.write_all(req.as_bytes());
    });
}

struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
}

/// wgpu 初始化——每步一个里程碑（死亡点定位器，2026-08-13 同款打法）
fn init_gfx(window: &Arc<Window>) -> Gfx {
    spike_report("wgpu instance 开始（GL 后端验尸轮——Vulkan 已判死 configure）");
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::GL;
    let instance = wgpu::Instance::new(desc);

    spike_report("surface 开始");
    let surface = instance
        .create_surface(window.clone())
        .expect("创建 wgpu surface 失败");

    spike_report("adapter 开始");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("无可用适配器（2026-08-13 雷 1 爆点）");
    spike_report(&format!("adapter 成了: {:?}", adapter.get_info()));

    spike_report("device 开始");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("gpu-spike"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("请求 wgpu 设备失败");

    let size = window.inner_size();
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        color_space: wgpu::SurfaceColorSpace::default(),
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    spike_report("configure 开始（当年死亡点）");
    surface.configure(&device, &config);
    spike_report("configure 过了");

    // 三角管线（WGSL，无顶点缓冲——vertex_index 出三个点）
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tri"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.6), vec2<f32>(-0.6, -0.6), vec2<f32>(0.6, -0.6));
    return vec4<f32>(pos[i], 0.0, 1.0);
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.45, 0.1, 1.0);
}
"#
            .into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tri"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    spike_report("pipeline 成了");
    Gfx {
        surface,
        device,
        queue,
        config,
        pipeline,
    }
}

/// 一帧：紫底清屏 + 橙三角（KFM 紫 #8B5CF6 垫底，三角证 draw 链路）
fn draw(g: &Gfx) {
    // wgpu 30：get_current_texture 去 Result 化，改返 CurrentSurfaceTexture
    // 枚举——Lost/Outdated 重配，Timeout/Occluded/Validation 丢帧跳过
    let frame = match g.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            g.surface.configure(&g.device, &g.config); // 表面丢失：重配跳过本帧
            return;
        }
        _ => return,
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("f") });
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("f"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.545,
                        g: 0.361,
                        b: 0.965,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        rp.set_pipeline(&g.pipeline);
        rp.draw(0..3, 0..1);
    }
    g.queue.submit([enc.finish()]);
    g.queue.present(frame); // wgpu 30：present 从 SurfaceTexture 挪到 Queue
    let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        spike_report("首帧 present 过了（当年死亡段全程通关）");
    }
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    last_beat: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        spike_report("resumed：建窗 + wgpu 初始化");
        let attrs = Window::default_attributes().with_title("GPU尖刺");
        let window = Arc::new(el.create_window(attrs).expect("创建窗口失败"));
        let gfx = init_gfx(&window);
        self.gfx = Some(gfx);
        self.window = Some(window);
        spike_report("resumed 完成");
    }

    /// 当年老尖刺不拆 surface——surface 生命周期正是嫌疑区，本版补上：
    /// 挂起即拆窗拆 surface，resume 重建（验收挂 10 循环）
    fn suspended(&mut self, _el: &ActiveEventLoop) {
        spike_report("suspended：拆 surface + 窗");
        self.gfx = None;
        self.window = None;
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
                    draw(g);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        // 心跳：每秒一条（存活判卷的计时器——断点即死亡行）
        let beat_due = self.last_beat.is_none_or(|t| t.elapsed().as_secs() >= 1);
        if beat_due {
            self.last_beat = Some(Instant::now());
            spike_report(&format!(
                "心跳 frames={} 存活 {}s",
                FRAMES.load(Ordering::Relaxed),
                BOOT_T0.get().map_or(0, |t| t.elapsed().as_secs())
            ));
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// NativeActivity 入口（android-activity 约定符号名）
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    BOOT_T0.set(Instant::now()).ok();
    std::panic::set_hook(Box::new(|info| {
        spike_report(&format!("PANIC: {info}"));
    }));
    spike_report("android_main 进入（wgpu 30 × NativeActivity 复活尖刺①）");
    let event_loop = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("创建事件循环失败");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("事件循环崩溃");
}
