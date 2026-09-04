//! gles-spike — GPU 渲染期 0 复活尖刺③：glow/GLES 直连 × NativeActivity
//! （gpu-render.md §四；①wgpu 30 双后端已判死 configure，见 §八）
//!
//! 不过 wgpu-hal，自己 dlopen libEGL.so 走裸 EGL/GLES。诊断价值：
//! 若裸 EGL 能 configure（eglCreateWindowSurface + make_current）上屏，
//! 病灶即坐实在 wgpu-hal 而非 Mali r44p1 驱动——「绕开 wgpu 的 GPU 化」
//! 这条路就活着；若裸 EGL 也死同一处，GPU 线全灭封档转 CPU 优化线。
//!
//! 判卷证据 = 里程碑 + 每秒心跳经飞鸽传书回传（①同款 JSON 通道）。
//! 验收：15 分钟存活零原生崩溃 + suspend/resume 10 循环。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use glow::HasContext;
use khronos_egl as egl;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

static BOOT_T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static FRAMES: AtomicU64 = AtomicU64::new(0);

/// 飞鸽传书（①同款：JSON {stage,msg}，一线程一报，2s 超时吞错）
fn spike_report(msg: &str) {
    let ms = BOOT_T0.get().map_or(0, |t| t.elapsed().as_millis());
    let esc = msg.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!("{{\"stage\":\"gles-spike\",\"msg\":\"+{ms}ms {esc}\"}}");
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

type Egl = egl::DynamicInstance<egl::EGL1_4>;

struct Gfx {
    egl: Egl,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    gl: glow::Context,
    prog: glow::NativeProgram,
    vao: glow::NativeVertexArray,
}

/// 裸 EGL/GLES 初始化——每步一个里程碑（死亡点定位器，①同款打法）
fn init_gfx(window: &Arc<Window>) -> Gfx {
    spike_report("dlopen libEGL.so + EGL1_4 符号加载");
    let egl = unsafe {
        egl::DynamicInstance::<egl::EGL1_4>::load_required_from(
            libloading::Library::new("libEGL.so").expect("dlopen libEGL.so 失败"),
        )
        .expect("EGL1_4 符号加载失败")
    };

    spike_report("eglGetDisplay + eglInitialize");
    let display = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.expect("无默认 display");
    let (major, minor) = egl.initialize(display).expect("eglInitialize 失败");
    spike_report(&format!("EGL 初始化过了 v{major}.{minor}"));

    let attribs = [
        egl::SURFACE_TYPE,
        egl::WINDOW_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_ES3_BIT,
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        8,
        egl::NONE,
    ];
    let config = egl
        .choose_first_config(display, &attribs)
        .expect("eglChooseConfig 失败")
        .expect("无可用 EGL config");
    spike_report("choose_config 过了（RGB888 ES3 window）");

    egl.bind_api(egl::OPENGL_ES_API).expect("eglBindAPI 失败");
    let context = egl
        .create_context(display, config, None, &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE])
        .expect("eglCreateContext 失败");
    spike_report("GLES3 context 成了");

    let wh = window.window_handle().expect("取窗柄失败").as_raw();
    let RawWindowHandle::AndroidNdk(h) = wh else {
        panic!("非 Android 窗柄");
    };
    let native_window = h.a_native_window.as_ptr() as egl::NativeWindowType;

    spike_report("eglCreateWindowSurface 开始（wgpu 双后端的死亡点·裸形态）");
    let surface = unsafe { egl.create_window_surface(display, config, native_window, None) }
        .expect("eglCreateWindowSurface 失败");
    spike_report("create_window_surface 过了");
    egl.make_current(display, Some(surface), Some(surface), Some(context))
        .expect("eglMakeCurrent 失败");
    spike_report("make_current 过了（wgpu 从未活着走到这）");

    spike_report("glow loader + shader/VAO");
    let gl = unsafe {
        glow::Context::from_loader_function(|s| {
            egl.get_proc_address(s)
                .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
        })
    };
    let (prog, vao) = unsafe {
        let vs = gl.create_shader(glow::VERTEX_SHADER).expect("建 vs 失败");
        gl.shader_source(
            vs,
            "#version 300 es\n\
             void main(){\n\
             vec2 p=vec2[](vec2(0.,.6),vec2(-.6,-.6),vec2(.6,-.6))[gl_VertexID];\n\
             gl_Position=vec4(p,0.,1.);}",
        );
        gl.compile_shader(vs);
        if !gl.get_shader_compile_status(vs) {
            panic!("VS 编译失败: {}", gl.get_shader_info_log(vs));
        }
        let fs = gl.create_shader(glow::FRAGMENT_SHADER).expect("建 fs 失败");
        gl.shader_source(
            fs,
            "#version 300 es\nprecision mediump float;\n\
             out vec4 c;\nvoid main(){c=vec4(1.,.45,.1,1.);}",
        );
        gl.compile_shader(fs);
        if !gl.get_shader_compile_status(fs) {
            panic!("FS 编译失败: {}", gl.get_shader_info_log(fs));
        }
        let prog = gl.create_program().expect("建 program 失败");
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            panic!("link 失败: {}", gl.get_program_info_log(prog));
        }
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        let vao = gl.create_vertex_array().expect("建 VAO 失败");
        (prog, vao)
    };
    spike_report("shader/VAO 成了");
    Gfx {
        egl,
        display,
        context,
        surface,
        gl,
        prog,
        vao,
    }
}

/// 一帧：紫底清屏 + 橙三角（与①同视觉判据，便于肉眼对拍）
fn draw(g: &Gfx) {
    unsafe {
        g.gl.clear_color(0.545, 0.361, 0.965, 1.0);
        g.gl.clear(glow::COLOR_BUFFER_BIT);
        g.gl.use_program(Some(g.prog));
        g.gl.bind_vertex_array(Some(g.vao));
        g.gl.draw_arrays(glow::TRIANGLES, 0, 3);
    }
    g.egl
        .swap_buffers(g.display, g.surface)
        .expect("eglSwapBuffers 失败");
    let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        spike_report("首帧 swap_buffers 过了（判决点：裸 EGL 上屏成功）");
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
        spike_report("resumed：建窗 + 裸 EGL/GLES 初始化");
        let attrs = Window::default_attributes().with_title("GLES尖刺");
        let window = Arc::new(el.create_window(attrs).expect("创建窗口失败"));
        let gfx = init_gfx(&window);
        self.gfx = Some(gfx);
        self.window = Some(window);
        spike_report("resumed 完成");
    }

    /// ①同款：挂起即拆 surface/context + 窗（验收挂 10 循环）
    fn suspended(&mut self, _el: &ActiveEventLoop) {
        spike_report("suspended：拆 surface/context + 窗");
        if let Some(g) = self.gfx.take() {
            let _ = g.egl.make_current(g.display, None, None, None);
            let _ = g.egl.destroy_surface(g.display, g.surface);
            let _ = g.egl.destroy_context(g.display, g.context);
        }
        self.window = None;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => {
                if let Some(g) = &self.gfx {
                    draw(g);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
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
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    BOOT_T0.set(Instant::now()).ok();
    std::panic::set_hook(Box::new(|info| {
        spike_report(&format!("PANIC: {info}"));
    }));
    spike_report("android_main 进入（glow/GLES 直连尖刺③——①wgpu 双后端已葬 configure）");
    let event_loop = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("创建事件循环失败");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("事件循环崩溃");
}
