//! gles_present.rs — GLES present 后端（期 1 第 1 层：壳内 EGL 基建）
//!
//! 期 0③ 尖刺（spikes/gles，gpu-render.md §九）的骨架移植：同一份 EGL
//! 生命周期与 suspend/resume 拆建纪律。本层还**不是**字形图集——全部
//! 光栅化照旧 CPU 进 `pixels`，本模块只把「present」从 softbuffer 换
//! 成「纹理上传 + 全屏三角 + eglSwapBuffers」。判卷：主 app 在 GLES
//! 上能起、能亮、后台切回不崩，像素与 softbuffer 一致（na-shot 对拍）。
//!
//! B 档（平台胶水）：对错是「系统让不让你活」，冒烟钉防退化。
//! 初始化任何一步失败都走 Result——调用方（init_gfx）回退 softbuffer，
//! 立项书红线「softbuffer 永久保留」在这层兑现。

use std::ffi::c_void;
use std::sync::Arc;

use glow::HasContext;
use khronos_egl as egl;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

type Egl = egl::DynamicInstance<egl::EGL1_4>;

/// 像素格式：CPU 帧缓冲是 XRGB u32（0x00RRGGBB），小端内存布局
/// [BB,GG,RR,00]——按 RGBA8 上传后 texel=(BB,GG,RR,00)，片元里
/// swizzle 成 (b,g,r) 即得正确颜色，零 CPU 转换、零扩展依赖。
pub struct GlesPresent {
    egl: Egl,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    gl: glow::Context,
    prog: glow::NativeProgram,
    tex: glow::NativeTexture,
    _vao: glow::NativeVertexArray,
    /// CPU 侧帧缓冲（rasterize 的画布，present 时整幅上传）
    pixels: Vec<u32>,
    w: u32,
    h: u32,
    /// 纹理已分配的尺寸（变化才 texImage2D 重分配，否则 texSubImage2D）
    tex_size: (u32, u32),
}

impl GlesPresent {
    /// 建 EGL 全栈 + 全屏三角管线——每步一个里程碑（尖刺③同款打法），
    /// 失败即 Err（调用方回退 softbuffer），不 expect 炸进程
    pub fn new(window: &Arc<Window>) -> Result<Self, String> {
        crate::report::report("boot", "GLES: dlopen libEGL.so + EGL1_4 符号加载");
        let egl = unsafe {
            egl::DynamicInstance::<egl::EGL1_4>::load_required_from(
                libloading::Library::new("libEGL.so")
                    .map_err(|e| format!("dlopen libEGL.so 失败: {e}"))?,
            )
            .map_err(|e| format!("EGL1_4 符号加载失败: {e:?}"))?
        };

        let display = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.ok_or("无默认 display")?;
        let (major, minor) = egl
            .initialize(display)
            .map_err(|e| format!("eglInitialize 失败: {e:?}"))?;
        crate::report::report("boot", &format!("GLES: EGL 初始化过了 v{major}.{minor}"));

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
            .map_err(|e| format!("eglChooseConfig 失败: {e:?}"))?
            .ok_or("无可用 EGL config")?;

        egl.bind_api(egl::OPENGL_ES_API)
            .map_err(|e| format!("eglBindAPI 失败: {e:?}"))?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
            )
            .map_err(|e| format!("eglCreateContext 失败: {e:?}"))?;

        let wh = window
            .window_handle()
            .map_err(|e| format!("取窗柄失败: {e:?}"))?
            .as_raw();
        let RawWindowHandle::AndroidNdk(h) = wh else {
            return Err("非 Android 窗柄".into());
        };
        let native_window = h.a_native_window.as_ptr() as egl::NativeWindowType;

        crate::report::report(
            "boot",
            "GLES: eglCreateWindowSurface（尖刺①坟头·裸形态已过）",
        );
        let surface = unsafe { egl.create_window_surface(display, config, native_window, None) }
            .map_err(|e| format!("eglCreateWindowSurface 失败: {e:?}"))?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .map_err(|e| format!("eglMakeCurrent 失败: {e:?}"))?;

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                egl.get_proc_address(s)
                    .map_or(std::ptr::null(), |f| f as *const c_void)
            })
        };

        let (prog, tex, vao) = Self::build_pipeline(&gl)?;

        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        // 不等 vsync（interval 0）：draw_frame 是脏触发的条件帧，swap 堵到
        // 下个垂直同步会反过来卡输入事件派发。撕裂对本负载（网格/面板）
        // 不可感；帧率治理在泵侧（fx_frame_due ≤60fps）
        let _ = egl.swap_interval(display, 0);
        crate::report::report("boot", &format!("GLES: present 后端上线 {w}x{h}"));
        Ok(Self {
            egl,
            display,
            context,
            surface,
            gl,
            prog,
            tex,
            _vao: vao,
            pixels: vec![0; (w * h) as usize],
            w,
            h,
            tex_size: (0, 0),
        })
    }

    /// 全屏三角 + 纹理采样（swizzle 在片元），无顶点缓冲无 VBO
    fn build_pipeline(
        gl: &glow::Context,
    ) -> Result<
        (
            glow::NativeProgram,
            glow::NativeTexture,
            glow::NativeVertexArray,
        ),
        String,
    > {
        unsafe {
            let vs = gl.create_shader(glow::VERTEX_SHADER)?;
            gl.shader_source(
                vs,
                "#version 300 es\n\
             out vec2 v_uv;\n\
             void main(){\n\
             vec2 p=vec2[](vec2(-1.,-3.),vec2(-1.,1.),vec2(3.,1.))[gl_VertexID];\n\
             v_uv=vec2(p.x*.5+.5,.5-p.y*.5);\n\
             gl_Position=vec4(p,0.,1.);}",
            );
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                return Err(format!("VS 编译失败: {}", gl.get_shader_info_log(vs)));
            }
            let fs = gl.create_shader(glow::FRAGMENT_SHADER)?;
            gl.shader_source(
                fs,
                "#version 300 es\nprecision mediump float;\n\
             in vec2 v_uv; out vec4 o;\n\
             uniform sampler2D u_tex;\n\
             void main(){ vec4 t=texture(u_tex,v_uv); o=vec4(t.b,t.g,t.r,1.); }",
            );
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                return Err(format!("FS 编译失败: {}", gl.get_shader_info_log(fs)));
            }
            let prog = gl.create_program()?;
            gl.attach_shader(prog, vs);
            gl.attach_shader(prog, fs);
            gl.link_program(prog);
            if !gl.get_program_link_status(prog) {
                return Err(format!("link 失败: {}", gl.get_program_info_log(prog)));
            }
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            let tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            // NEAREST：1:1 present 逐像素保真（LINEAR 会糊字形/SDF 边）
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.use_program(Some(prog));
            gl.uniform_1_i32(gl.get_uniform_location(prog, "u_tex").as_ref(), 0);
            let vao = gl.create_vertex_array()?;
            Ok((prog, tex, vao))
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }

    /// Resized 事件同步（EGL 窗表面随系统自调，这里只跟帧缓冲尺寸）
    pub fn set_size(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) != (self.w, self.h) {
            self.w = w;
            self.h = h;
            self.pixels.resize((w * h) as usize, 0);
        }
    }

    /// rasterize 的画布（与 softbuffer buffer_mut 同尺的 &mut [u32]）
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    /// 只读半（FrameBuf 的 Deref 用）
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// 上传 + 全屏三角 + swap（期 1 第 1 层的「present」全量）
    pub fn present(&mut self) {
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            let bytes: &[u8] = std::slice::from_raw_parts(
                self.pixels.as_ptr() as *const u8,
                self.pixels.len() * 4,
            );
            if self.tex_size != (self.w, self.h) {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    self.w as i32,
                    self.h as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bytes)),
                );
                self.tex_size = (self.w, self.h);
            } else {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    self.w as i32,
                    self.h as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bytes)),
                );
            }
            gl.viewport(0, 0, self.w as i32, self.h as i32);
            gl.use_program(Some(self.prog));
            gl.bind_vertex_array(Some(self._vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
        self.egl
            .swap_buffers(self.display, self.surface)
            .expect("eglSwapBuffers 失败");
    }
}

impl Drop for GlesPresent {
    /// 尖刺③纪律：挂起即拆（surface/context 随 Gfx 一起 drop）
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        let _ = self.egl.destroy_context(self.display, self.context);
    }
}
