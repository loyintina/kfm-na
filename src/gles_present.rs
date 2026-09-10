//! gles_present.rs — GLES present 后端（期 1 第 1 层：壳内 EGL 基建）
//!
//! 期 0③ 尖刺（spikes/gles，gpu-render.md §九）的骨架移植：同一份 EGL
//! 生命周期与 suspend/resume 拆建纪律。2026-09-07 图层槽位化（ui-base
//! §八 渲染成本模型）：CPU 光栅只发生在置脏帧（slot_bake），动画帧
//! 只动 placement（panel_off 进实例 rect）——终端网格/AI 文字照旧 GPU
//! 图集实例。判卷：主 app 在 GLES 上能起、能亮、面板动画视觉与双层
//! 合成时代等价、帧率实测对齐（panel-anim 仪表）。
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
/// 第 2 层管线物：layer/bg/glyph 三程序 + 三组实例 VAO/VBO 对
/// （chrome 全屏纹理已被图层槽位取代——ui-base §八 渲染成本模型）
#[allow(clippy::type_complexity)]
type Layer2 = (
    glow::NativeProgram,
    glow::NativeProgram,
    glow::NativeProgram,
    glow::NativeVertexArray,
    glow::NativeBuffer,
    glow::NativeVertexArray,
    glow::NativeBuffer,
    glow::NativeVertexArray,
    glow::NativeBuffer,
);

/// 图层槽位（ui-base §八）：每槽 = 独立画布 + 纹理 + 可见性。动画
/// （placement 变化）不触碰槽内容——合成期只挪矩形；内容变化由调用方
/// 置脏重烘焙（slot_bake）。z 序由 present_frame 按面板栈动态排：
/// 键行恒在网格之上，两面板的上下关系跟 snap.top 走（被覆盖者在下，
/// placement 不动、遮盖撤走零动画露出），Over 恒在一切之上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeSlot {
    /// 快捷键行带（终端页）
    Keybar = 0,
    /// AI 面板底装修（紫底+边框环；烘焙画布恒为靠泊位，panel_off 是
    /// 合成期 placement 不进画布）
    Panel = 1,
    /// 上层 chrome（输入栏/光球/放大镜，浮在 AI 文字之上）
    Over = 2,
    /// 配置面板底装修（青底+边框环；placement.x 跟 cfg_off，面板栈
    /// §五B 左滑抽屉——被覆盖时仍烘焙仍可见，AI 面板盖在它上面）
    Config = 3,
}

/// 单槽烘焙物。baked=false 的槽不许上屏——采样未上传过的纹理得到
/// 不完整纹理恒黑（黑屏案 2026-09-05 教训的图层版）
struct ChromeLayer {
    canvas: Vec<u32>,
    tex: glow::NativeTexture,
    /// 已上传尺寸（与画布尺寸不符 → 下次 bake 重分配）
    size: (u32, u32),
    visible: bool,
    baked: bool,
}

/// 回读探针开关（黑屏案 2026-09-05）：判卷仪器已收队，翻 true 可再开
/// （五横行回读/品红实例/T3 三连/缩略图回传全套基础设施保留）
const GLS_READBACK_PROBE: bool = false;

// ---- 阶段耗时累计（微秒。性能优化判卷仪表：每 300 帧上报一次均值）----
pub static STAGE_RAS_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static STAGE_ALPHA_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static STAGE_UPLOAD_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static STAGE_DRAW_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static STAGE_GEN_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static STAGE_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// ---- 面板动画期帧率仪表（2026-09-06）：缝活性为真时逐帧记账，
// 动画收尾（活性转假）一次性上报 N 帧/均值/最大——stage 计数是 300
// 帧滚动平均，动画帧被空闲帧稀释，判不了动画手感 ----
static ANIM_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ANIM_TOTAL_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ANIM_MAX_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 动画期逐帧记账 + 收尾上报（android_app draw_frame GLES 每帧调用方）：
/// active=true 记账；active=false 且有在记的账 = 动画刚结束 → 上报清账。
/// 2026-09-07 扩展（用户报「拖影变多」的观测基建，三路径判卷）：
/// ①swap 间隔（送帧节奏）②Choreographer vsync 对表（显示真实刷新率 +
/// swap 相位）③动画期回读抽帧（渲染源真相，偶数轮采样奇数轮净跑——
/// readPixels 有停顿会污染节奏数据）。撕裂的最终裁决在系统录屏（P2，
/// 人工路径）——送帧齐 + 显示侧撕 = 呈现侧病（vsync 实验）
static SWAP_LAST_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SWAP_GAP_MIN_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SWAP_GAP_MAX_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SWAP_GAP_TOTAL_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SWAP_GAP_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ANIM_WAS_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CAPTURE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CAPTURE_TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// (w, h, rgb) 缩略帧仓——run 收尾统一外发
static CAPTURE_FRAMES: std::sync::Mutex<Vec<(u32, u32, Vec<u8>)>> =
    std::sync::Mutex::new(Vec::new());

// vsync 对表（dlsym libandroid.so 的 NDK API29+ 符号——targetSdk 28 不便
// 静态链接，运行期 dlsym，libEGL dlopen 先例；句柄存静态保符号有效）。
// BAR-072：账本（ARMED/last_ns/gap 五原子）归核心层 vsync_book 册——
// host 可判卷；本壳只管 dlsym/回调 ABI/相位。
static VSYNC_PHASE_MIN_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static VSYNC_PHASE_MAX_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static VSYNC_PHASE_TOTAL_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static VSYNC_PHASE_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[repr(C)]
struct AChoreographer {
    _opaque: [u8; 0],
}
// BAR-072（2026-09-09 面板闪退案真凶）：NDK 契约 AChoreographer_frameCallback64
// 是**两参** (int64_t frameTimeNanos, void* data)——本类型曾凭空多画了
// 一个首位 *const AChoreographer 形参，于是回调里读到的 "c" 实际是帧时间戳，
// 自续挂表把它当 Choreographer 指针回 post → postFrameCallbackDelayed 对
// 时间戳+0x58 做 mutex::lock → SIGSEGV（三案故障地址低 48 位=同段开机时长
// 纳秒数，铁证）。cb 体内要回 post 就重新 getInstance，不许缓存/伪造 this。
type ChoreoCallback = unsafe extern "C" fn(i64, *mut std::ffi::c_void);
#[repr(C)]
struct ChoreoFns {
    get_instance: unsafe extern "C" fn() -> *const AChoreographer,
    post_cb64: unsafe extern "C" fn(*const AChoreographer, ChoreoCallback, *mut std::ffi::c_void),
}
static CHOREO: std::sync::Mutex<Option<(libloading::Library, ChoreoFns)>> =
    std::sync::Mutex::new(None);

/// 单调时钟（ns，OnceLock 基点——swap/vsync 间隔只吃差值，起点无所谓）
fn now_ns() -> u64 {
    static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let base = BASE.get_or_init(std::time::Instant::now);
    base.elapsed().as_nanos() as u64
}

/// 动画轮开表：节奏/相位记账清零 + 采样点播 + vsync 挂表
fn anim_run_start() {
    use std::sync::atomic::Ordering;
    for a in [
        &SWAP_GAP_N,
        &SWAP_GAP_TOTAL_US,
        &VSYNC_PHASE_N,
        &VSYNC_PHASE_TOTAL_US,
    ] {
        a.store(0, Ordering::Relaxed);
    }
    SWAP_GAP_MIN_US.store(u64::MAX, Ordering::Relaxed);
    SWAP_GAP_MAX_US.store(0, Ordering::Relaxed);
    VSYNC_PHASE_MIN_US.store(u64::MAX, Ordering::Relaxed);
    VSYNC_PHASE_MAX_US.store(0, Ordering::Relaxed);
    crate::vsync_book::reset_run(); // gap 账+基线归 vsync_book（BAR-072）
    // BAR-076：采样改点播——奇偶轮播时代每两轮动画就有一轮被 readPixels
    // 压到 16fps（61ms/帧实测），仪器噪音成了用户体验税。要采样先投
    // anim-cap-req 触发（na-anim-cap.sh），不投 = 零开销
    CAPTURE_ON.store(
        crate::gate::take_anim_cap_req(crate::gate::DUMP_DIR),
        Ordering::Relaxed,
    );
    CAPTURE_TICK.store(0, Ordering::Relaxed);
    vsync_arm();
}

/// Choreographer 挂表（懒 dlsym；回调链自续，vsync_disarm 收表——
/// 零空转纪律：非动画期零回调零唤醒）
fn vsync_arm() {
    let mut guard = CHOREO.lock().unwrap();
    if guard.is_none() {
        let lib = match unsafe { libloading::Library::new("libandroid.so") } {
            Ok(l) => l,
            Err(_) => {
                crate::report::report("boot", "vsync对表: libandroid.so 打不开，降级纯 swap 间隔");
                return;
            }
        };
        // 先解出函数指针再 move lib（Symbol 借用与所有权的借序——E0505）
        let gi: unsafe extern "C" fn() -> *const AChoreographer = {
            match unsafe { lib.get(b"AChoreographer_getInstance\0") } {
                Ok(s) => *s,
                Err(_) => {
                    crate::report::report("boot", "vsync对表: AChoreographer_getInstance 缺席");
                    return;
                }
            }
        };
        let pc: unsafe extern "C" fn(*const AChoreographer, ChoreoCallback, *mut std::ffi::c_void) = {
            match unsafe { lib.get(b"AChoreographer_postFrameCallback64\0") } {
                Ok(s) => *s,
                Err(_) => {
                    crate::report::report("boot", "vsync对表: postFrameCallback64 缺席");
                    return;
                }
            }
        };
        *guard = Some((
            lib,
            ChoreoFns {
                get_instance: gi,
                post_cb64: pc,
            },
        ));
    }
    crate::vsync_book::arm();
    if let Some((_, fns)) = guard.as_ref() {
        unsafe {
            let c = (fns.get_instance)();
            // 当前线程无 ALooper 时 getInstance 返回空（getForThread 判空实锤）——
            // 空指针进 post = 0x58 处暴毙，拦在门外
            if !c.is_null() {
                (fns.post_cb64)(c, vsync_cb, std::ptr::null_mut());
            }
        }
    }
}

/// vsync 跳记账回调：逐跳记周期；仍武装则自续（回调链）。
/// NDK 两参契约 (frameTimeNanos, data)——签名钉见 ChoreoCallback（BAR-072）；
/// 回 post 的 this 现场 getInstance 重取，不吃任何外来指针
unsafe extern "C" fn vsync_cb(ts: i64, _d: *mut std::ffi::c_void) {
    unsafe {
        crate::vsync_book::note_tick(ts as u64);
        if crate::vsync_book::armed()
            && let Some((_, fns)) = CHOREO.lock().unwrap().as_ref()
        {
            let c = (fns.get_instance)();
            if !c.is_null() {
                (fns.post_cb64)(c, vsync_cb, std::ptr::null_mut());
            }
        }
    }
}

/// BAR-072 ABI 钉：以 NDK 契约型导出真回调。考题拿这枚指针按
/// (frameTimeNanos, data) 直接喂——谁把 vsync_cb 改回三参「带
/// Choreographer 指针」的幻觉版本，本函数类型不匹配当场编译红
#[doc(hidden)]
pub fn vsync_cb_for_ndk() -> ChoreoCallback {
    vsync_cb
}

/// swap 间隔小账（present_frame 每帧喂：now=本帧 swap 时刻）
pub fn note_swap(now: u64) {
    use std::sync::atomic::Ordering;
    let last = SWAP_LAST_NS.swap(now, Ordering::Relaxed);
    if last == 0 || !ANIM_WAS_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let gap = now.saturating_sub(last) / 1000;
    SWAP_GAP_N.fetch_add(1, Ordering::Relaxed);
    SWAP_GAP_TOTAL_US.fetch_add(gap, Ordering::Relaxed);
    SWAP_GAP_MIN_US.fetch_min(gap, Ordering::Relaxed);
    SWAP_GAP_MAX_US.fetch_max(gap, Ordering::Relaxed);
    // 相位：本帧 swap 落在 vsync 周期内的位置（对齐质量——稳=齐，飘=抖）
    let vlast = crate::vsync_book::last_ns();
    if crate::vsync_book::armed() && vlast != 0 && now > vlast {
        let phase = (now - vlast) / 1000;
        VSYNC_PHASE_N.fetch_add(1, Ordering::Relaxed);
        VSYNC_PHASE_TOTAL_US.fetch_add(phase, Ordering::Relaxed);
        VSYNC_PHASE_MIN_US.fetch_min(phase, Ordering::Relaxed);
        VSYNC_PHASE_MAX_US.fetch_max(phase, Ordering::Relaxed);
    }
}

fn swap_gap_report() -> String {
    let n = SWAP_GAP_N.load(std::sync::atomic::Ordering::Relaxed);
    if n == 0 {
        return String::new();
    }
    use std::sync::atomic::Ordering;
    let total = SWAP_GAP_TOTAL_US.swap(0, Ordering::Relaxed);
    let min = SWAP_GAP_MIN_US.swap(0, Ordering::Relaxed);
    let max = SWAP_GAP_MAX_US.swap(0, Ordering::Relaxed);
    SWAP_GAP_N.store(0, Ordering::Relaxed);
    let avg = total.checked_div(n).unwrap_or(0);
    format!(
        " swap间隔 min/avg/max {}ms/{}ms/{}ms",
        min / 1000,
        avg / 1000,
        max / 1000
    )
}

fn vsync_report() -> String {
    let Some((n, vmin, vmax, total)) = crate::vsync_book::take_gap() else {
        return " vsync无样本".into();
    };
    use std::sync::atomic::Ordering;
    let avg = total.checked_div(n).unwrap_or(1).max(1);
    // Hz ×10（整数域一位小数）：10_000_000us / 周期us
    let hz10 = 10_000_000_u64.checked_div(avg).unwrap_or(0);
    let pn = VSYNC_PHASE_N.swap(0, Ordering::Relaxed);
    let phase = if pn > 0 {
        let pmin = VSYNC_PHASE_MIN_US.swap(0, Ordering::Relaxed);
        let pmax = VSYNC_PHASE_MAX_US.swap(0, Ordering::Relaxed);
        let pavg = VSYNC_PHASE_TOTAL_US
            .swap(0, Ordering::Relaxed)
            .checked_div(pn)
            .unwrap_or(0);
        format!(
            " 相位 min/avg/max {}ms/{}ms/{}ms",
            pmin / 1000,
            pavg / 1000,
            pmax / 1000
        )
    } else {
        String::new()
    };
    format!(
        " vsync {}.{n2}Hz 样本{n} 周期 min/avg/max {}ms/{}ms/{}ms{phase}",
        hz10 / 10,
        vmin / 1000,
        avg / 1000,
        vmax / 1000,
        n2 = hz10 % 10,
    )
}

/// 采样帧外发（run 收尾：hex 分块飞鸽传书，服务器拼 PNG——渲染源真相）
fn capture_report() -> String {
    let frames: Vec<(u32, u32, Vec<u8>)> = CAPTURE_FRAMES.lock().unwrap().drain(..).collect();
    if frames.is_empty() {
        return " 采样0".into();
    }
    let label = format!(" 采样{}帧", frames.len());
    std::thread::spawn(move || {
        use std::io::Write;
        for (i, (w, h, rgb)) in frames.iter().enumerate() {
            let hex: String = rgb.iter().map(|b| format!("{b:02x}")).collect();
            let total = hex.len().div_ceil(1400);
            for (ci, chunk) in hex.as_bytes().chunks(1400).enumerate() {
                let body = format!(
                    "{{\"stage\":\"anim-strip\",\"msg\":\"{i}|{w}|{h}|{ci}|{total}|{}\"}}",
                    String::from_utf8_lossy(chunk)
                );
                let _ = std::net::TcpStream::connect_timeout(
                    &std::net::SocketAddr::from(([127, 0, 0, 1], 8021)),
                    std::time::Duration::from_secs(2),
                )
                .and_then(|mut s| {
                    s.write_all(
                        format!(
                            "POST /kfmv4/api/na-report HTTP/1.1\r\nHost: 127.0.0.1:8021\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                });
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    });
    label
}

pub fn note_anim_frame(active: bool, elapsed: std::time::Duration) {
    use std::sync::atomic::Ordering;
    let us = elapsed.as_micros() as u64;
    let was = ANIM_WAS_ACTIVE.swap(active, Ordering::Relaxed);
    if active {
        if !was {
            anim_run_start();
        }
        ANIM_FRAMES.fetch_add(1, Ordering::Relaxed);
        ANIM_TOTAL_US.fetch_add(us, Ordering::Relaxed);
        ANIM_MAX_US.fetch_max(us, Ordering::Relaxed);
        return;
    }
    let n = ANIM_FRAMES.swap(0, Ordering::Relaxed);
    if n > 0 {
        let total = ANIM_TOTAL_US.swap(0, Ordering::Relaxed);
        let max = ANIM_MAX_US.swap(0, Ordering::Relaxed);
        let avg_us = total / n.max(1);
        let avg_ms = avg_us / 1000;
        let max_ms = max / 1000;
        let fps = 1_000_000_u64.checked_div(avg_us).unwrap_or(0);
        let mut line = format!("{n}帧 均值{avg_us}us({avg_ms}ms) 最大{max_ms}ms 推算fps={fps}");
        line.push_str(&swap_gap_report());
        line.push_str(&vsync_report());
        line.push_str(&capture_report());
        vsync_disarm();
        CAPTURE_ON.store(false, Ordering::Relaxed);
        crate::report::report("panel-anim", &line);
    }
}

fn vsync_disarm() {
    crate::vsync_book::disarm();
}

fn stage_report() {
    let n = STAGE_N.swap(0, std::sync::atomic::Ordering::Relaxed);
    if n == 0 {
        return;
    }
    let avg = |c: &std::sync::atomic::AtomicU64| {
        c.swap(0, std::sync::atomic::Ordering::Relaxed) / n.max(1) / 1000
    };
    crate::report::report(
        "gles-stage",
        &format!(
            "ras={}ms gen={}ms alpha={}ms upload={}ms draw={}ms /{n}帧",
            avg(&STAGE_RAS_US),
            avg(&STAGE_GEN_US),
            avg(&STAGE_ALPHA_US),
            avg(&STAGE_UPLOAD_US),
            avg(&STAGE_DRAW_US),
        ),
    );
}

pub struct GlesPresent {
    egl: Egl,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    gl: glow::Context,
    w: u32,
    h: u32,
    // ---- 期 1 第 2 层：终端网格 GPU 化 ----
    /// 图层槽位（ui-base §八 渲染成本模型）：键行/AI面板/上层/配置四槽，
    /// 置脏烘焙 + placement 合成——动画帧零光栅零上传
    layers: [ChromeLayer; 4],
    /// 图层实例程序（rect+uv+tint 四边形；placement 逐槽进实例数据）
    layer_prog: glow::NativeProgram,
    layer_vao: glow::NativeVertexArray,
    layer_vbo: glow::NativeBuffer,
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
    /// 已呈现帧数（回读探针判卷用）
    frames_presented: u64,
    /// 已上传的图集版本（图集 revision 变化 → 全页重传——首帧上传后
    /// 新字形只进 CPU coverage 不重传 = 字形全隐形的黑屏案 2026-09-05）
    atlas_rev: u64,
    /// 缩略图已拍标记（墙钟触发，一次性）
    thumb_sent: bool,
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

        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        let (
            layer_prog,
            bg_prog,
            glyph_prog,
            bg_vao,
            bg_vbo,
            glyph_vao,
            glyph_vbo,
            layer_vao,
            layer_vbo,
        ) = Self::build_layer2(&gl, w, h)?;

        // 不等 vsync（interval 0）：draw_frame 是脏触发的条件帧，swap 堵到
        // 下个垂直同步会反过来卡输入事件派发。撕裂对本负载（网格/面板）
        // 不可感；帧率治理在泵侧（fx_frame_due ≤60fps）
        let _ = egl.swap_interval(display, 0);
        crate::report::report("boot", &format!("GLES: present 后端上线 {w}x{h}"));
        let mk_layer = |gl: &glow::Context| -> ChromeLayer {
            let tex = unsafe { gl.create_texture() }.expect("建图层纹理失败");
            ChromeLayer {
                canvas: vec![0; (w * h) as usize],
                tex,
                size: (0, 0),
                visible: false,
                baked: false,
            }
        };
        // 先建槽数组再 move gl 进结构体（E0382：字段初始化按书写序移动）
        let layers = [mk_layer(&gl), mk_layer(&gl), mk_layer(&gl), mk_layer(&gl)];
        Ok(Self {
            egl,
            display,
            context,
            surface,
            gl,
            w,
            h,
            layers,
            layer_prog,
            layer_vao,
            layer_vbo,
            bg_prog,
            glyph_prog,
            bg_vao,
            bg_vbo,
            glyph_vao,
            glyph_vbo,
            atlas_tex: Vec::new(),
            frames_presented: 0,
            thumb_sent: false,
            atlas_rev: 0,
            atlas: crate::glyph_atlas::GlyphAtlas::new(2048, 2048),
        })
    }

    /// 期 1 第 2 层管线：图层/bg/glyph 三程序 + 实例 VAO/VBO。
    /// 四边形全用 3 倍超界大三角（角 (0,0),(0,3),(3,0)——目标矩形
    /// (W,H) 落在斜线 x/3W+y/3H=1 内侧 2/3 处，整格全覆盖无半像素缝）。
    fn build_layer2(gl: &glow::Context, w: u32, h: u32) -> Result<Layer2, String> {
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

            // 图层槽：实例化四边形（rect px + uv + tint，placement 进
            // rect）+ RGBA 纹理采样，alpha 混合（槽画布 0 = 透明）。
            // u_vp 链接期一次写死（黑屏案 2026-09-05：每帧 uniform 疑似
            // 静默失效——本程序只设 u_tex=0 同样在链接期写）
            let layer_prog = {
                let v = vs(
                    "#version 300 es\n\
                     layout(location=0) in vec4 a_rect;\n\
                     layout(location=1) in vec4 a_uv;\n\
                     layout(location=2) in vec4 a_tint;\n\
                     out vec2 v_uv;\n\
                     out vec2 v_local;\n\
                     out float v_alpha;\n\
                     uniform vec2 u_vp;\n\
                     void main(){\n\
                     vec2 c=vec2[](vec2(0.,0.),vec2(0.,3.),vec2(3.,0.))[gl_VertexID];\n\
                     v_uv=a_uv.xy+c*a_uv.zw;\n\
                     v_local=c;\n\
                     v_alpha=a_tint.a;\n\
                     vec2 px=a_rect.xy+c*a_rect.zw;\n\
                     gl_Position=vec4(px.x/u_vp.x*2.-1.,1.-px.y/u_vp.y*2.,0.,1.);\n\
                     }",
                    "layer",
                )?;
                let f = fs(
                    "#version 300 es\nprecision mediump float;\n\
                     in vec2 v_uv; in vec2 v_local; in float v_alpha; out vec4 o;\n\
                     uniform sampler2D u_tex;\n\
                     void main(){ if(v_local.x<0.||v_local.y<0.||v_local.x>1.||v_local.y>1.) discard;\n\
                     vec4 t=texture(u_tex,v_uv); o=vec4(t.b,t.g,t.r,t.a*v_alpha); }",
                    "layer",
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
                     out vec2 v_local;\n\
                     uniform vec2 u_vp;\n\
                     void main(){\n\
                     vec2 c=vec2[](vec2(0.,0.),vec2(0.,3.),vec2(3.,0.))[gl_VertexID];\n\
                     vec2 px=a_rect.xy+c*a_rect.zw;\n\
                     v_local=c;\n\
                     v_color=a_color;\n\
                     gl_Position=vec4(px.x/u_vp.x*2.-1.,1.-px.y/u_vp.y*2.,0.,1.);\n\
                     }",
                    "bg",
                )?;
                let f = fs(
                    "#version 300 es\nprecision mediump float;\n\
                     in vec4 v_color; in vec2 v_local; out vec4 o;\n\
                     void main(){ if(v_local.x<0.||v_local.y<0.||v_local.x>1.||v_local.y>1.) discard; o=vec4(v_color.bgr,1.); }",
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
                     out vec2 v_local;\n\
                     out vec4 v_fg;\n\
                     uniform vec2 u_vp;\n\
                     void main(){\n\
                     vec2 c=vec2[](vec2(0.,0.),vec2(0.,3.),vec2(3.,0.))[gl_VertexID];\n\
                     v_uv=a_uv.xy+c*a_uv.zw;\n\
                     v_local=c;\n\
                     v_fg=a_fg;\n\
                     vec2 px=a_rect.xy+c*a_rect.zw;\n\
                     gl_Position=vec4(px.x/u_vp.x*2.-1.,1.-px.y/u_vp.y*2.,0.,1.);\n\
                     }",
                    "glyph",
                )?;
                let f = fs(
                    "#version 300 es\nprecision mediump float;\n\
                     in vec2 v_uv; in vec2 v_local; in vec4 v_fg; out vec4 o;\n\
                     uniform sampler2D u_tex;\n\
                     uniform float u_alpha;\n\
                     void main(){ if(v_local.x<0.||v_local.y<0.||v_local.x>1.||v_local.y>1.) discard; float cov=texture(u_tex,v_uv).r; o=vec4(v_fg.bgr,cov*u_alpha); }",
                    "glyph",
                )?;
                link(gl, v, f)?
            };

            // u_vp 就地写死（Gfx 生命周期 = 窗口尺寸生命周期，resize 即
            // 重建管线）——黑屏案 2026-09-05：每帧 uniform 设置疑似静默
            // 失效，改链接期一次写入
            gl.use_program(Some(bg_prog));
            let loc = gl.get_uniform_location(bg_prog, "u_vp");
            crate::report::report("boot", &format!("GLES: bg u_vp loc={loc:?}"));
            gl.uniform_2_f32(loc.as_ref(), w as f32, h as f32);
            gl.use_program(Some(glyph_prog));
            let loc2 = gl.get_uniform_location(glyph_prog, "u_vp");
            crate::report::report("boot", &format!("GLES: glyph u_vp loc={loc2:?}"));
            gl.uniform_2_f32(loc2.as_ref(), w as f32, h as f32);
            gl.use_program(Some(layer_prog));
            let loc3 = gl.get_uniform_location(layer_prog, "u_vp");
            crate::report::report("boot", &format!("GLES: layer u_vp loc={loc3:?}"));
            gl.uniform_2_f32(loc3.as_ref(), w as f32, h as f32);
            gl.uniform_1_i32(gl.get_uniform_location(layer_prog, "u_tex").as_ref(), 0);

            // 实例 VAO/VBO：bg（rect+color = 5×f32 = 20B）/glyph（+uv+fg = 9×f32 = 40B）
            let bg_vao = gl.create_vertex_array()?;
            let bg_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(bg_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(bg_vbo));
            stride_attrib(gl, 0, 0, 20, true, true);
            stride_attrib(gl, 1, 16, 20, false, true); // 颜色 = 归一化 ubyte
            let glyph_vao = gl.create_vertex_array()?;
            let glyph_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(glyph_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(glyph_vbo));
            stride_attrib(gl, 0, 0, 40, true, true);
            stride_attrib(gl, 1, 16, 40, true, true);
            stride_attrib(gl, 2, 32, 40, false, true); // 前景色 = 归一化 ubyte
            gl.bind_vertex_array(None);

            let layer_vao = gl.create_vertex_array()?;
            let layer_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(layer_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(layer_vbo));
            // 图层实例：rect(4f32)+uv(4f32)+tint(4f32) = 48B，全 f32
            stride_attrib(gl, 0, 0, 48, true, true);
            stride_attrib(gl, 1, 16, 48, true, true);
            stride_attrib(gl, 2, 32, 48, true, true);
            gl.bind_vertex_array(None);

            Ok((
                layer_prog, bg_prog, glyph_prog, bg_vao, bg_vbo, glyph_vao, glyph_vbo, layer_vao,
                layer_vbo,
            ))
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
            for l in &mut self.layers {
                l.canvas.resize((w * h) as usize, 0);
                // 尺寸缓存作废（下次 bake 重分配；sig 侧含 w/h 必然重烘焙）
                l.size = (0, 0);
            }
        }
    }

    /// 槽位画布（供调用方 paint；不置脏不上传——bake 才算数）
    pub fn slot_canvas(&mut self, s: ChromeSlot) -> &mut [u32] {
        &mut self.layers[s as usize].canvas
    }

    /// 槽位可见性（合成期跳过不画；烘焙物保留，重现身零成本）
    pub fn set_slot_visible(&mut self, s: ChromeSlot, v: bool) {
        self.layers[s as usize].visible = v;
    }

    /// 烘焙一槽：mark_chrome_alpha（「纯黑=空白」约定）+ 全画布上传。
    /// 只在置脏帧调用——这是图层引擎的成本闸门（动画帧不进这里）
    pub fn slot_bake(&mut self, s: ChromeSlot) {
        let idx = s as usize;
        let t0_up = std::time::Instant::now();
        crate::termview::mark_chrome_alpha(&mut self.layers[idx].canvas);
        let target = (self.w, self.h);
        let realloc = self.layers[idx].size != target;
        let tex = self.layers[idx].tex;
        let px: &[u32] = &self.layers[idx].canvas;
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            // 黑屏案终凶（2026-09-05）：默认 MIN_FILTER = NEAREST_MIPMAP_LINEAR
            // 而本纹理无 mipmap → 纹理不完整 → 采样恒 (0,0,0,1) 黑不透明。
            // NEAREST + CLAMP 对每槽同样生效
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
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            let bytes: &[u8] = std::slice::from_raw_parts(px.as_ptr() as *const u8, px.len() * 4);
            if realloc {
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
            STAGE_UPLOAD_US.fetch_add(
                t0_up.elapsed().as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        self.layers[idx].size = target;
        self.layers[idx].baked = true;
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

    /// 期 1 第 2 层组合帧（2026-09-07 图层槽位版，09-10 面板栈双面板化）：
    /// 清屏 → 网格背景实例 → 网格字形实例（按页）→ 键行槽 → 被覆盖面板槽
    /// → 在顶面板槽（placement 跟缝采样、alpha 随落程显影——动画帧唯二
    /// 变的东西，零上传）→ AI 文字字形实例（按页，u_alpha = panel_alpha
    /// 随 AI 面板显影）→ 上层槽（输入栏/光球/放大镜）→ swap。
    /// 两面板 z 序由 cfg_on_top 裁决（= snap.top，§五B：被覆盖者在下，
    /// placement 不动、遮盖撤走零动画露出）；AI 文字是 AI 面板的墨，
    /// 必须紧跟 AI 面板槽画（Config 在顶时压在 AI 文字上）。槽画布由
    /// 调用方置脏烘焙（slot_bake），未烘焙的槽不上屏（不完整纹理=黑屏案）
    #[allow(clippy::too_many_arguments)]
    pub fn present_frame(
        &mut self,
        bg: &[crate::glyph_atlas::BgInstance],
        glyphs_by_page: &[Vec<crate::glyph_atlas::GlyphInstance>],
        ai_glyphs_by_page: &[Vec<crate::glyph_atlas::GlyphInstance>],
        panel_off: i32,
        panel_alpha: f32,
        cfg_off: i32,
        cfg_alpha: f32,
        cfg_on_top: bool,
    ) {
        let t0_draw = std::time::Instant::now();
        // CPU 画布直接测量（rgb 非零计数 + 样本原值）——「画没画」的铁证
        if GLS_READBACK_PROBE {
            let kb = &self.layers[ChromeSlot::Keybar as usize].canvas;
            let rgb_nz = kb.iter().filter(|p| *p & 0x00FF_FFFF != 0).count();
            let mid = kb[(self.h / 2) as usize * self.w as usize + (self.w / 2) as usize];
            let kbar = kb[((self.h - 400) as usize) * self.w as usize + (self.w / 2) as usize];
            crate::report::report(
                "gles-dbg",
                &format!("canvas rgb非零={rgb_nz} mid={mid:#010x} keybar={kbar:#010x}"),
            );
        }
        // 图集纹理同步：版本变化（新字形装载）→ 全页重传（4MB/页 R8，
        // 仅新字形帧发生）；新页出现即补
        let rev = self.atlas.revision();
        if rev != self.atlas_rev {
            let pages: Vec<(u32, u32, u32, Vec<u8>)> = self
                .atlas
                .pages()
                .iter()
                .enumerate()
                .map(|(i, p)| (i as u32, p.w, p.h, p.coverage.clone()))
                .collect();
            for (i, w, h, cov) in pages {
                self.upload_atlas_page(i, w, h, &cov);
            }
            self.atlas_rev = rev;
        }
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, self.w as i32, self.h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::BLEND);

            // 背景
            if !bg.is_empty() {
                gl.use_program(Some(self.bg_prog));
                gl.bind_vertex_array(Some(self.bg_vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.bg_vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    std::slice::from_raw_parts(bg.as_ptr() as *const u8, bg.len() * 20),
                    glow::DYNAMIC_DRAW,
                );
                gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, bg.len() as i32);
            }

            // 字形（alpha 混合，按图集页分组 draw——每页一次上传+绘制；
            // 终端网格显影恒 1.0——「终端格子内容永不动画」红线）
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            draw_glyph_pages(
                gl,
                self.glyph_prog,
                self.glyph_vao,
                self.glyph_vbo,
                &self.atlas_tex,
                glyphs_by_page,
                1.0,
            );

            // 键行槽（面板未靠泊时可见；烘焙物常驻纹理，重现身零成本）
            let kb = &self.layers[ChromeSlot::Keybar as usize];
            if kb.visible && kb.baked {
                draw_slot_layer(
                    gl,
                    self.layer_prog,
                    self.layer_vao,
                    self.layer_vbo,
                    kb.tex,
                    0.0,
                    0.0,
                    self.w as f32,
                    self.h as f32,
                    1.0,
                );
            }

            // 两面板槽 + AI 文字：z 序跟面板栈顶走（§五B）。AI 文字是
            // AI 面板的墨——必须紧跟 AI 面板槽画，Config 在顶时它被
            // 配置页连墨带底一起盖住
            let pn = &self.layers[ChromeSlot::Panel as usize];
            let cf = &self.layers[ChromeSlot::Config as usize];
            if !cfg_on_top {
                // 配置页在下（被覆盖或不在栈）：先画它
                if cf.visible && cf.baked {
                    draw_slot_layer(
                        gl,
                        self.layer_prog,
                        self.layer_vao,
                        self.layer_vbo,
                        cf.tex,
                        cfg_off as f32,
                        0.0,
                        self.w as f32,
                        self.h as f32,
                        cfg_alpha,
                    );
                }
            }
            // AI 面板槽（placement.y = panel_off + tint.α = panel_alpha——
            // 动画帧唯二变化的输入，零光栅零上传；屏外部分 viewport
            // 裁剪零成本）
            if pn.visible && pn.baked {
                draw_slot_layer(
                    gl,
                    self.layer_prog,
                    self.layer_vao,
                    self.layer_vbo,
                    pn.tex,
                    0.0,
                    panel_off as f32,
                    self.w as f32,
                    self.h as f32,
                    panel_alpha,
                );
            }
            // AI 文字实例（面板刚体的墨——z 序紧跟面板底之上；
            // u_alpha 随面板显影，墨不游离于底）
            draw_glyph_pages(
                gl,
                self.glyph_prog,
                self.glyph_vao,
                self.glyph_vbo,
                &self.atlas_tex,
                ai_glyphs_by_page,
                panel_alpha,
            );
            if cfg_on_top {
                // 配置页在顶：压在 AI 面板与 AI 文字之上
                if cf.visible && cf.baked {
                    draw_slot_layer(
                        gl,
                        self.layer_prog,
                        self.layer_vao,
                        self.layer_vbo,
                        cf.tex,
                        cfg_off as f32,
                        0.0,
                        self.w as f32,
                        self.h as f32,
                        cfg_alpha,
                    );
                }
            }

            // 上层槽（输入栏/光球/放大镜——浮在一切内容之上）
            let ov = &self.layers[ChromeSlot::Over as usize];
            if ov.visible && ov.baked {
                draw_slot_layer(
                    gl,
                    self.layer_prog,
                    self.layer_vao,
                    self.layer_vbo,
                    ov.tex,
                    0.0,
                    0.0,
                    self.w as f32,
                    self.h as f32,
                    1.0,
                );
            }
            gl.disable(glow::BLEND);
            STAGE_DRAW_US.fetch_add(
                t0_draw.elapsed().as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            let n = STAGE_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n.is_multiple_of(300) {
                stage_report();
            }

            // GPU 合成缩略图回传（黑屏案判卷仪器）：整帧逐行回读 →
            // 1/10 抽样 → hex 分块飞鸽传书 → 服务器拼图转 PNG 亲眼看。
            // 仅第 3 帧拍一次（内容已稳定）
            if GLS_READBACK_PROBE && !self.thumb_sent && crate::report::boot_ms() > 20_000 {
                self.thumb_sent = true;
                let tw = (self.w / 10) as usize;
                let th = (self.h / 10) as usize;
                let mut thumb = vec![0u8; tw * th * 3];
                for ty in 0..th {
                    let mut row = vec![0u8; (self.w as usize) * 4];
                    gl.read_pixels(
                        0,
                        (ty * 10) as i32,
                        self.w as i32,
                        1,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut row)),
                    );
                    for tx in 0..tw {
                        let s = tx * 10 * 4;
                        let d = (ty * tw + tx) * 3;
                        thumb[d] = row[s];
                        thumb[d + 1] = row[s + 1];
                        thumb[d + 2] = row[s + 2];
                    }
                }
                let hex: String = thumb.iter().map(|b| format!("{b:02x}")).collect();
                let total = hex.len().div_ceil(1400);
                std::thread::spawn(move || {
                    use std::io::Write;
                    for (i, chunk) in hex.as_bytes().chunks(1400).enumerate() {
                        let _ = std::net::TcpStream::connect_timeout(
                            &std::net::SocketAddr::from(([127, 0, 0, 1], 8021)),
                            std::time::Duration::from_secs(2),
                        )
                        .and_then(|mut s| {
                            let body =
                                format!("{{\"stage\":\"gles-thumb\",\"msg\":\"{}|{}|{}\"}}",
                                    i, total, String::from_utf8_lossy(chunk));
                            s.write_all(
                                format!(
                                    "POST /kfmv4/api/na-report HTTP/1.1\r\nHost: 127.0.0.1:8021\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    body.len(),
                                    body
                                )
                                .as_bytes(),
                            )
                        });
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                });
            }

            // 终审实验：品红实例挪到所有层之后重画——它若现身，
            // 实例化/属性/u_vp 全部无罪，凶手是绘制顺序/覆盖；仍黑 =
            // 实例化路径本身有病（属性指针/instanced 调用）
            if GLS_READBACK_PROBE {
                gl.use_program(Some(self.bg_prog));
                gl.bind_vertex_array(Some(self.bg_vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.bg_vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    &{
                        let mut b = [0u8; 20];
                        b[0..4].copy_from_slice(&300.0f32.to_ne_bytes());
                        b[4..8].copy_from_slice(&1400.0f32.to_ne_bytes());
                        b[8..12].copy_from_slice(&300.0f32.to_ne_bytes());
                        b[12..16].copy_from_slice(&300.0f32.to_ne_bytes());
                        b[16..20].copy_from_slice(&0x00FF_00FFu32.to_ne_bytes());
                        b
                    },
                    glow::DYNAMIC_DRAW,
                );
                gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, 1);
            }

            // 回读探针（黑屏案 2026-09-05）：swap 前采样三屏点 + GL 错误
            // 全扫——值直接飞鸽传书，GPU 真实输出不再靠肉眼转述
            if GLS_READBACK_PROBE {
                let errs: Vec<u32> = [gl.get_error(), gl.get_error()]
                    .into_iter()
                    .filter(|e| *e != glow::NO_ERROR)
                    .collect();
                // 五横行回读：横幅/终端上/终端中/快捷键行/输入栏
                // （单点采样会落在合法黑区——整行非黑计数才判得准）
                let rows = [2674i32, 2500, 1400, 450, 100]; // GL 坐标（y 从底部）：屏 126/300/1400/2350/2700
                // 探针点：品红方块中心。glReadPixels y 从底部起算——
                // 屏幕 (200,1100) = GL (200,1700)。上轮读 (150,1050) =
                // 屏幕 y1750，在方块外——探针自身坐标翻转教训
                // 绿三角中心：clip(-0.667,-0.267) → px(210, GL y1026)
                let mut probe_px = [0u8; 4];
                gl.read_pixels(
                    200,
                    1700,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut probe_px)),
                );
                let mut green_px = [0u8; 4];
                gl.read_pixels(
                    210,
                    1026,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut green_px)),
                );
                let mut stats = Vec::new();
                for y in rows {
                    let mut row = vec![0u8; (self.w as usize) * 4];
                    gl.read_pixels(
                        0,
                        y,
                        self.w as i32,
                        1,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut row)),
                    );
                    let nb = row.chunks(4).filter(|p| p[0] + p[1] + p[2] > 24).count();
                    stats.push(format!("y{y}={nb}/{}", self.w));
                }
                crate::report::report(
                    "gles-dbg",
                    &format!(
                        "品红={probe_px:?} 绿={green_px:?} {} errs={errs:?} n={}",
                        stats.join(" "),
                        self.frames_presented
                    ),
                );
            }
        }
        // A 软件内截屏（shot-gles-req，2026-09-07）：倒的是真·GLES 合成
        // 帧（pre-swap readPixels 全分辨率，含图层槽位/AI 文字最终 z 序）
        // ——CPU 重画通道的真相升级。静态屏无帧可消费时 na-shot 回退
        // shot.rgb（画面没动过，内容等价）
        if crate::gate::shot_gl_requested(crate::gate::DUMP_DIR) {
            let buf = self.capture_full();
            crate::gate::write_shot_gl(crate::gate::DUMP_DIR, &buf, self.w, self.h);
        }
        // P3 渲染源采样（点播武装，BAR-076：anim-cap-req 触发才开，
        // 不投 = 零 readPixels 开销）：readPixels 有停顿只落采样轮；
        // 每 5 帧一拍（21 帧动画取 ~4 帧），1/14 缩略
        if CAPTURE_ON.load(std::sync::atomic::Ordering::Relaxed)
            && CAPTURE_TICK
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .is_multiple_of(5)
            && let Some(t) = self.capture_thumb()
        {
            CAPTURE_FRAMES.lock().unwrap().push(t);
        }
        // P1 送帧节奏记账（swap 间隔 + vsync 相位）
        crate::gles_present::note_swap(now_ns());
        self.swap();
        self.frames_presented += 1;
    }

    /// 纯色帧（降级路径：无终端/字体全灭——紫屏）。单实例 bg 全屏，
    /// 不触碰图层槽（这些路径没有 chrome 可言）
    pub fn present_solid(&mut self, color: u32) {
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, self.w as i32, self.h as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::BLEND);
            gl.use_program(Some(self.bg_prog));
            gl.bind_vertex_array(Some(self.bg_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.bg_vbo));
            let inst = crate::glyph_atlas::BgInstance {
                x: 0.0,
                y: 0.0,
                w: self.w as f32,
                h: self.h as f32,
                color: color | 0xFF00_0000,
            };
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                std::slice::from_raw_parts(
                    &inst as *const crate::glyph_atlas::BgInstance as *const u8,
                    20,
                ),
                glow::DYNAMIC_DRAW,
            );
            gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, 1);
        }
        self.swap();
        self.frames_presented += 1;
    }

    /// 1/14 缩略回读（P3 渲染源真相：动画期抽帧——读的是本帧 GL 合成
    /// 结果，swap 前；1260/14≈90 宽，hex 分块走报表通道，服务器拼 PNG）
    fn capture_thumb(&self) -> Option<(u32, u32, Vec<u8>)> {
        const SS: usize = 14;
        let tw = (self.w as usize / SS).max(1);
        let th = (self.h as usize / SS).max(1);
        let mut thumb = vec![0u8; tw * th * 3];
        let gl = &self.gl;
        unsafe {
            for ty in 0..th {
                let mut row = vec![0u8; (self.w as usize) * 4];
                gl.read_pixels(
                    0,
                    (ty * SS) as i32,
                    self.w as i32,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut row)),
                );
                for tx in 0..tw {
                    let s = tx * SS * 4;
                    let d = (ty * tw + tx) * 3;
                    thumb[d] = row[s];
                    thumb[d + 1] = row[s + 1];
                    thumb[d + 2] = row[s + 2];
                }
            }
        }
        Some((tw as u32, th as u32, thumb))
    }

    /// 全分辨率合成帧回读（shot-gles-req 消费方，swap 前调用——读的
    /// 是本帧 GL 合成结果；1260×2800 RGBA ≈ 14MB，仅触发帧付此代价）
    fn capture_full(&self) -> Vec<u32> {
        let n = (self.w * self.h) as usize;
        let mut rgba = vec![0u8; n * 4];
        let gl = &self.gl;
        unsafe {
            gl.read_pixels(
                0,
                0,
                self.w as i32,
                self.h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut rgba)),
            );
        }
        crate::gate::rgba_bytes_to_xrgb(&rgba)
    }

    fn swap(&mut self) {
        self.egl
            .swap_buffers(self.display, self.surface)
            .expect("eglSwapBuffers 失败");
    }
}

/// 字形实例按图集页绘制（网格层与 AI 文字层共用——同一 prog/vao/vbo，
/// 只差调用点的 z 序与实例内容）。每页绑纹理 + 传实例 + instanced draw
/// 按图集页分组 draw（每页一次上传+绘制）。alpha = 整批显影系数
/// （滑动淡入：AI 文字随面板 placement 显影；终端网格恒 1.0——
/// 「终端格子内容永不动画」红线，ui-base §五）
unsafe fn draw_glyph_pages(
    gl: &glow::Context,
    prog: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    atlas_tex: &[glow::NativeTexture],
    pages: &[Vec<crate::glyph_atlas::GlyphInstance>],
    alpha: f32,
) {
    unsafe {
        gl.use_program(Some(prog));
        gl.uniform_1_i32(gl.get_uniform_location(prog, "u_tex").as_ref(), 0);
        gl.uniform_1_f32(gl.get_uniform_location(prog, "u_alpha").as_ref(), alpha);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        for (page, insts) in pages.iter().enumerate() {
            if insts.is_empty() || page >= atlas_tex.len() {
                continue;
            }
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex[page]));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                std::slice::from_raw_parts(insts.as_ptr() as *const u8, insts.len() * 40),
                glow::DYNAMIC_DRAW,
            );
            gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, insts.len() as i32);
        }
    }
}

/// 单槽图层四边形（可见+烘焙过才画；placement 进实例 rect，
/// uv 恒全幅——v1 槽画布=全屏尺寸）。alpha = 整槽显影系数（滑动
/// 淡入：面板槽随 placement 显影；其余槽恒 1.0）
#[allow(clippy::too_many_arguments)]
unsafe fn draw_slot_layer(
    gl: &glow::Context,
    prog: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    tex: glow::NativeTexture,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    alpha: f32,
) {
    unsafe {
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.use_program(Some(prog));
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        // rect(4)+uv(4)+tint(4) = 12×f32 = 48B，与 layer_vao 配置咬合
        let inst = [
            x, y, w, h, // a_rect（px，placement 在这）
            0.0, 0.0, 1.0, 1.0, // a_uv
            1.0, 1.0, 1.0, alpha, // a_tint（α=整槽显影——转场/主题槽位）
        ];
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            std::slice::from_raw_parts(inst.as_ptr() as *const u8, 48),
            glow::DYNAMIC_DRAW,
        );
        gl.draw_arrays_instanced(glow::TRIANGLES, 0, 3, 1);
    }
}

/// 实例属性：float=true 读 4×f32；false 读 4×ubyte 归一化（颜色）
/// stride 字节，divisor=1（每实例一次）。BAR-0xx 教训（2026-09-05 黑屏
/// 案）：颜色槽按 FLOAT 配指针会读穿结构体边界——rect 16B + color 4B，
/// FLOAT 版多吃的 12B 全是下一实例的垃圾。
unsafe fn stride_attrib(
    gl: &glow::Context,
    loc: u32,
    off: usize,
    stride: usize,
    float: bool,
    instanced: bool,
) {
    unsafe {
        gl.enable_vertex_attrib_array(loc);
        let ty = if float {
            glow::FLOAT
        } else {
            glow::UNSIGNED_BYTE
        };
        gl.vertex_attrib_pointer_f32(loc, 4, ty, !float, stride as i32, off as i32);
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
