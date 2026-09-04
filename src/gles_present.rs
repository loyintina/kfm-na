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
/// 第 2 层管线物：chrome/bg/glyph 三程序 + 背景与字形 VAO/VBO 对 + chrome 纹理
#[allow(clippy::type_complexity)]
type Layer2 = (
    glow::NativeProgram,
    glow::NativeProgram,
    glow::NativeProgram,
    glow::NativeVertexArray,
    glow::NativeBuffer,
    glow::NativeVertexArray,
    glow::NativeBuffer,
    glow::NativeTexture,
);

pub struct GlesPresent {
    egl: Egl,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    gl: glow::Context,
    prog: glow::NativeProgram,
    _vao: glow::NativeVertexArray,
    /// CPU 侧帧缓冲（chrome 层画布——终端网格已归 GPU 图集/实例，见第 2 层）
    pixels: Vec<u32>,
    w: u32,
    h: u32,
    // ---- 期 1 第 2 层：终端网格 GPU 化 ----
    /// chrome 层纹理（栏带/输入栏/AI 页/放大镜，CPU 画 → RGBA 上传）
    chrome_tex: glow::NativeTexture,
    chrome_size: (u32, u32),
    /// chrome 全屏四边形（RGBA 采样，alpha 混合叠在网格层上）
    chrome_prog: glow::NativeProgram,
    /// 网格背景实色实例
    bg_prog: glow::NativeProgram,
    bg_vao: glow::NativeVertexArray,
    bg_vbo: glow::NativeBuffer,
    /// 网格字形实例（图集 R8 采样 × 前景色 alpha 混合）
    glyph_prog: glow::NativeProgram,
    glyph_vao: glow::NativeVertexArray,
    glyph_vbo: glow::NativeBuffer,
    /// 图集页纹理（页索引对齐 GlyphAtlas.pages()）
    atlas_tex: Vec<glow::NativeTexture>,
    /// 字形图集（数据所有权在此，跨帧缓存——第 2 层性能来源）
    atlas: crate::glyph_atlas::GlyphAtlas,
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

        let (prog, _tex_unused, vao) = Self::build_pipeline(&gl)?;
        let (chrome_prog, bg_prog, glyph_prog, bg_vao, bg_vbo, glyph_vao, glyph_vbo, chrome_tex) =
            Self::build_layer2(&gl)?;

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
            _vao: vao,
            pixels: vec![0; (w * h) as usize],
            w,
            h,
            chrome_tex,
            chrome_size: (0, 0),
            chrome_prog,
            bg_prog,
            glyph_prog,
            bg_vao,
            bg_vbo,
            glyph_vao,
            glyph_vbo,
            atlas_tex: Vec::new(),
            atlas: crate::glyph_atlas::GlyphAtlas::new(2048, 2048),
        })
    }

    /// 期 1 第 2 层管线：chrome 全屏四边形 + 网格背景/字形实例化。
    /// 四边形全用 3 倍超界大三角（角 (0,0),(0,3),(3,0)——目标矩形
    /// (W,H) 落在斜线 x/3W+y/3H=1 内侧 2/3 处，整格全覆盖无半像素缝）。
    fn build_layer2(gl: &glow::Context) -> Result<Layer2, String> {
        unsafe {
            let vs = |src: &str, tag: &str| -> Result<glow::NativeShader, String> {
                let s = gl.create_shader(glow::VERTEX_SHADER)?;
                gl.shader_source(s, src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    return Err(format!("{tag} VS: {}", gl.get_shader_info_log(s)));
                }
                Ok(s)
            };
            let fs = |src: &str, tag: &str| -> Result<glow::NativeShader, String> {
                let s = gl.create_shader(glow::FRAGMENT_SHADER)?;
                gl.shader_source(s, src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    return Err(format!("{tag} FS: {}", gl.get_shader_info_log(s)));
                }
                Ok(s)
            };

            // chrome：全屏三角 + RGBA 纹理，alpha 混合（term 区像素 0 = 透明）
            let chrome_prog = {
                let v = vs(
                    "#version 300 es\n\
                     out vec2 v_uv;\n\
                     void main(){\n\
                     vec2 p=vec2[](vec2(-1.,-3.),vec2(-1.,1.),vec2(3.,1.))[gl_VertexID];\n\
                     v_uv=vec2(p.x*.5+.5,.5-p.y*.5);\n\
                     gl_Position=vec4(p,0.,1.);}",
                    "chrome",
                )?;
                let f = fs(
                    "#version 300 es\nprecision mediump float;\n\
                     in vec2 v_uv; out vec4 o;\n\
                     uniform sampler2D u_tex;\n\
                     void main(){ vec4 t=texture(u_tex,v_uv); o=vec4(t.b,t.g,t.r,t.a); }",
                    "chrome",
                )?;
                link(gl, v, f)?
            };

            // 网格背景实例：rect + XRGB 颜色（归一化 ubyte），不透明
            let bg_prog = {
                let v = vs(
                    "#version 300 es\n\
                     layout(location=0) in vec4 a_rect;\n\
                     layout(location=1) in vec4 a_color;\n\
                     out vec4 v_color;\n\
                     uniform vec2 u_vp;\n\
                     void main(){\n\
                     vec2 c=vec2[](vec2(0.,0.),vec2(0.,3.),vec2(3.,0.))[gl_VertexID];\n\
                     vec2 px=a_rect.xy+c*a_rect.zw;\n\
                     v_color=a_color;\n\
                     gl_Position=vec4(px.x/u_vp.x*2.-1.,1.-px.y/u_vp.y*2.,0.,1.);\n\
                     }",
                    "bg",
                )?;
                let f = fs(
                    "#version 300 es\nprecision mediump float;\n\
                     in vec4 v_color; out vec4 o;\n\
                     void main(){ o=vec4(v_color.bgr,1.); }",
                    "bg",
                )?;
                link(gl, v, f)?
            };

            // 网格字形实例：rect + uv(u0,v0,du,dv) + 前景色；R8 图集 alpha
            let glyph_prog = {
                let v = vs(
                    "#version 300 es\n\
                     layout(location=0) in vec4 a_rect;\n\
                     layout(location=1) in vec4 a_uv;\n\
                     layout(location=2) in vec4 a_fg;\n\
                     out vec2 v_uv;\n\
                     out vec4 v_fg;\n\
                     uniform vec2 u_vp;\n\
                     void main(){\n\
                     vec2 c=vec2[](vec2(0.,0.),vec2(0.,3.),vec2(3.,0.))[gl_VertexID];\n\
                     v_uv=a_uv.xy+c*a_uv.zw;\n\
                     v_fg=a_fg;\n\
                     vec2 px=a_rect.xy+c*a_rect.zw;\n\
                     gl_Position=vec4(px.x/u_vp.x*2.-1.,1.-px.y/u_vp.y*2.,0.,1.);\n\
                     }",
                    "glyph",
                )?;
                let f = fs(
                    "#version 300 es\nprecision mediump float;\n\
                     in vec2 v_uv; in vec4 v_fg; out vec4 o;\n\
                     uniform sampler2D u_tex;\n\
                     void main(){ float cov=texture(u_tex,v_uv).r; o=vec4(v_fg.bgr,cov); }",
                    "glyph",
                )?;
                link(gl, v, f)?
            };

            let chrome_tex = gl.create_texture()?;

            // 实例 VAO/VBO：bg（rect+color = 5×f32 = 20B）/glyph（+uv+fg = 9×f32 + page 对齐 = 40B）
            let bg_vao = gl.create_vertex_array()?;
            let bg_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(bg_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(bg_vbo));
            stride_attrib(gl, 0, 0, 20, true);
            stride_attrib(gl, 1, 16, 20, true);
            let glyph_vao = gl.create_vertex_array()?;
            let glyph_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(glyph_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(glyph_vbo));
            stride_attrib(gl, 0, 0, 40, true);
            stride_attrib(gl, 1, 16, 40, true);
            stride_attrib(gl, 2, 32, 40, true);
            gl.bind_vertex_array(None);

            Ok((
                chrome_prog,
                bg_prog,
                glyph_prog,
                bg_vao,
                bg_vbo,
                glyph_vao,
                glyph_vbo,
                chrome_tex,
            ))
        }
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

    /// 图集只读（grid_to_instances 进料）
    pub fn atlas(&self) -> &crate::glyph_atlas::GlyphAtlas {
        &self.atlas
    }

    /// 图集装载（misses 补墨；同键幂等由图集保证）
    pub fn atlas_insert(
        &mut self,
        key: crate::glyph_atlas::GlyphKey,
        w: u32,
        h: u32,
        bitmap: &[u8],
        off_x: i16,
        off_y: i16,
    ) -> crate::glyph_atlas::GlyphSlot {
        self.atlas.insert(key, w, h, bitmap, off_x, off_y)
    }

    /// 上传 + 全屏三角 + swap（期 1 第 1 层的「present」——保留作
    /// chrome-only 路径的底座，第 2 层 present_frame 接管组合）
    pub fn present(&mut self) {
        self.upload_chrome();
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, self.w as i32, self.h as i32);
            gl.use_program(Some(self.prog));
            gl.bind_vertex_array(Some(self._vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
        self.swap();
    }

    /// chrome 层纹理上传（尺寸变化重分配，否则子更新）
    fn upload_chrome(&mut self) {
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.chrome_tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            let bytes: &[u8] = std::slice::from_raw_parts(
                self.pixels.as_ptr() as *const u8,
                self.pixels.len() * 4,
            );
            if self.chrome_size != (self.w, self.h) {
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
                self.chrome_size = (self.w, self.h);
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
        }
    }

    /// 图集页纹理上传（新增页/首装时调用；coverage 原样 R8）。
    /// 注意借序：先做 self.atlas_tex 的所有权操作，再取 gl。
    pub fn upload_atlas_page(&mut self, page: u32, w: u32, h: u32, coverage: &[u8]) {
        while self.atlas_tex.len() <= page as usize {
            let tex = unsafe { self.gl.create_texture() }.expect("建图集纹理失败");
            self.atlas_tex.push(tex);
        }
        let tex = self.atlas_tex[page as usize];
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
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
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                w as i32,
                h as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(coverage)),
            );
        }
    }

    /// 期 1 第 2 层组合帧：清屏 → 网格背景实例 → 图集字形实例（按页）
    /// → chrome 层（alpha 混合）→ swap。chrome 画布 = pixels（调用方
    /// 已做透明底 + 不透明 chrome 的 |= alpha 标记）。
    pub fn present_frame(
        &mut self,
        bg: &[crate::glyph_atlas::BgInstance],
        glyphs_by_page: &[Vec<crate::glyph_atlas::GlyphInstance>],
    ) {
        // 图集页增量上传（新页出现即补；同页重装由调用方触发全页重传）
        let need: Vec<(u32, u32, u32, Vec<u8>)> = self
            .atlas
            .pages()
            .iter()
            .enumerate()
            .filter(|(i, _p)| self.atlas_tex.len() <= *i)
            .map(|(i, p)| (i as u32, p.w, p.h, p.coverage.clone()))
            .collect();
        for (i, w, h, cov) in need {
            self.upload_atlas_page(i, w, h, &cov);
        }
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, self.w as i32, self.h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            let vp = (self.w as f32, self.h as f32);
            gl.disable(glow::BLEND);

            // 背景
            if !bg.is_empty() {
                gl.use_program(Some(self.bg_prog));
                gl.uniform_2_f32(
                    gl.get_uniform_location(self.bg_prog, "u_vp").as_ref(),
                    vp.0,
                    vp.1,
                );
                gl.bind_vertex_array(Some(self.bg_vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.bg_vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    std::slice::from_raw_parts(bg.as_ptr() as *const u8, bg.len() * 20),
                    glow::DYNAMIC_DRAW,
                );
                gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, bg.len() as i32);
            }

            // 字形（alpha 混合，按图集页分组 draw——每页一次上传+绘制）
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.glyph_prog));
            gl.uniform_2_f32(
                gl.get_uniform_location(self.glyph_prog, "u_vp").as_ref(),
                vp.0,
                vp.1,
            );
            gl.uniform_1_i32(
                gl.get_uniform_location(self.glyph_prog, "u_tex").as_ref(),
                0,
            );
            gl.active_texture(glow::TEXTURE0);
            gl.bind_vertex_array(Some(self.glyph_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.glyph_vbo));
            for (page, insts) in glyphs_by_page.iter().enumerate() {
                if insts.is_empty() || page >= self.atlas_tex.len() {
                    continue;
                }
                gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex[page]));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    std::slice::from_raw_parts(insts.as_ptr() as *const u8, insts.len() * 40),
                    glow::DYNAMIC_DRAW,
                );
                gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, insts.len() as i32);
            }

            // chrome 层（alpha 混合叠上）
            gl.bind_texture(glow::TEXTURE_2D, Some(self.chrome_tex));
            gl.use_program(Some(self.chrome_prog));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.chrome_prog, "u_tex").as_ref(),
                0,
            );
            gl.bind_vertex_array(Some(self._vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.disable(glow::BLEND);
        }
        self.swap();
    }

    fn swap(&mut self) {
        self.egl
            .swap_buffers(self.display, self.surface)
            .expect("eglSwapBuffers 失败");
    }
}

/// 实例属性：归一化 ubyte/浮点通吃，stride 字节，divisor=1（每实例一次）
unsafe fn stride_attrib(gl: &glow::Context, loc: u32, off: usize, stride: usize, instanced: bool) {
    unsafe {
        gl.enable_vertex_attrib_array(loc);
        gl.vertex_attrib_pointer_f32(loc, 4, glow::FLOAT, false, stride as i32, off as i32);
        if instanced {
            gl.vertex_attrib_divisor(loc, 1);
        }
    }
}

/// 编译对 → 程序（attach/link 已由调用方 compile 检查）
unsafe fn link(
    gl: &glow::Context,
    vs: glow::NativeShader,
    fs: glow::NativeShader,
) -> Result<glow::NativeProgram, String> {
    unsafe {
        let prog = gl.create_program()?;
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(format!("link 失败: {}", gl.get_program_info_log(prog)));
        }
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        Ok(prog)
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
