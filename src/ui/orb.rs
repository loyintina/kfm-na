//! ui/orb.rs — 光球控件（控件库第 3 层首个成员，2026-09-01 自 termview
//! 物理搬移立形：状态核 = ai_presence.rs，视图 = 本文件，注入通道 =
//! orb-inject，考题 = ai_presence_spec 逐像素钉，档案 = 插件档案体系。
//! 搬移为零逻辑变化——逐字节原样，只有模块路径变了）。
//!
//! 雾状光球 sprite（D8 拟合定稿 2026-08-30；同日压字反馈后改加法合成）：
//! 程序化预渲染 sprite 一次（build_orb_sprite 三层 Lambert 配方的光贡献
//! 量），运行时纯贴图饱和加——球只加光不遮光（压字反馈修复）。四态增益
//! 硬切（调用方传 ai_presence::orb_gain 的读数）：gain = 整 sprite 增益，
//! halo_gain > 1.0 选光晕加大变体（运行态）。

/// 雾状光球 sprite 配方常量（D8 拟合 2026-08-30）：三层参数化模型，常量
/// 来自 scripts/orb-fit.py 坐标下降拟合（RMSE 4.66/255，验收基准
/// docs/assets/orb-fit-generated.png；改动须重跑拟合器并同步
/// tests/ai_presence_spec.rs 逐像素钉）。长度量均以球半径 Rs 归一，任意
/// 尺寸可缩放
const ORB_BG: (f64, f64, f64) = (11.0, 10.0, 15.0); // 参考图底（加值零点）
const ORB_DARK: (f64, f64, f64) = (9.0, 8.0, 13.0); // 球体暗面色
const ORB_C_LIT: (f64, f64, f64) = (99.0, 50.0, 198.0); // 受光色 = (100,50,200)*bri 0.99
const ORB_HALO_RG: f64 = 2.93; // 光晕 (1-r/Rg)^p 的 Rg（Rs 倍数）
const ORB_HALO_P: f64 = 2.05;
const ORB_HALO_T_AMP: f64 = 0.12; // 光晕尾部平台幅度
const ORB_HALO_T_SIG: f64 = 1.02; // 尾部 exp(-r/tsig) 的 tsig（Rs 倍数）
const ORB_LIGHT: (f64, f64) = (0.37, 0.45); // 光向（左上）
const ORB_LAMBERT_K: f64 = 2.24;
const ORB_SPHERE_ALPHA: f64 = 0.77; // 球体整盘 alpha（拟合层词序用，见下）
const ORB_SPEC_AMP: f64 = 0.22; // 高光点幅度
const ORB_SPEC_SIGMA: f64 = 0.10; // 高光高斯 sigma（Rs 倍数）
const ORB_SPEC_OFF: f64 = 0.55; // 高光心沿光向偏移（Rs 倍数）
/// sprite 截断半径（Rs 倍数）：光晕尾部在 3.5Rs 处已 <1/255
const ORB_HALO_CUT: f64 = 3.5;

/// 预渲染光球（加法 sprite）：每像素 = 三层公式在参考图底 BG=(11,10,15)
/// 上的合成结果**减 BG 裁剪 ≥0** 的光贡献量（0x00RRGGBB 加值）。绘制 =
/// 饱和加——球只加光不遮光（2026-08-30 压字反馈：alpha 混合球内笔画
/// −32%，参考图 orb-on-white-ref.jpg 是 +90% 提亮）。样式参考图暗面 ≈
/// BG，故黑底上 底+加值 精确复现拟合图，文字底上文字全亮透过+球加光
pub struct OrbSprite {
    pub size: u32,
    pub px: Vec<u32>,
}

/// 按 D8 配方建 sprite（rs = 球半径 px；halo_gain = 光晕增益，运行态加大）。
/// 考题与生产同源：逐像素钉（rs=64.25 对拍 orb-fit-generated.png）与
/// render（rs=ORB_RADIUS_PX）走的都是它
pub fn build_orb_sprite(rs: f64, halo_gain: f64) -> OrbSprite {
    let half = (ORB_HALO_CUT * rs).ceil() as i64;
    let size = (half * 2 + 1) as u32;
    let (lx, ly) = ORB_LIGHT;
    let lz = (1.0 - lx * lx - ly * ly).max(0.0).sqrt();
    let spec_sigma = ORB_SPEC_SIGMA * rs;
    let (hx, hy) = (-lx * rs * ORB_SPEC_OFF, -ly * rs * ORB_SPEC_OFF); // 高光心（沿光向）
    let mix = |a: f64, b: f64, t: f64| a * (1.0 - t) + b * t;
    let mut px = Vec::with_capacity((size * size) as usize);
    for iy in -half..=half {
        for ix in -half..=half {
            let (dx, dy) = (ix as f64, iy as f64);
            let rn = dx.hypot(dy) / rs;
            // ① 光晕层（底）：a(r) = clip(clip(1-r/Rg)^p + tamp*exp(-r/tsig))，
            //    色 = C_lit；halo_gain 烘焙在此（运行态光晕 +20%）。
            //    合成 = BG*(1-a) + C_lit*a（orb-fit.py 词序）
            let a_h = ((1.0 - rn / ORB_HALO_RG).max(0.0).powf(ORB_HALO_P)
                + ORB_HALO_T_AMP * (-rn / ORB_HALO_T_SIG).exp())
            .clamp(0.0, 1.0)
                * halo_gain;
            let a_h = a_h.clamp(0.0, 1.0);
            let halo = (
                mix(ORB_BG.0, ORB_C_LIT.0, a_h),
                mix(ORB_BG.1, ORB_C_LIT.1, a_h),
                mix(ORB_BG.2, ORB_C_LIT.2, a_h),
            );
            let comp = if rn <= 1.0 {
                // ② 球体层：Lambert 明暗 I = max(0, -lx*nx - ly*ny + lz*nz)^k，
                //    色 = mix(DARK, C_lit, I)；③ 高光点 = 沿光向小高斯，过曝往白
                let (nx, ny) = (dx / rs, dy / rs);
                let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
                let lam = (-lx * nx - ly * ny + lz * nz).max(0.0).powf(ORB_LAMBERT_K);
                let d2 = (dx - hx).powi(2) + (dy - hy).powi(2);
                let spec = ORB_SPEC_AMP * (-d2 / (2.0 * spec_sigma * spec_sigma)).exp();
                let i2 = (lam + spec).clamp(0.0, 1.6);
                let t = i2.clamp(0.0, 1.0);
                let over = (i2 - 1.0).max(0.0);
                let sph = (
                    mix(ORB_DARK.0, ORB_C_LIT.0, t) + over * 60.0,
                    mix(ORB_DARK.1, ORB_C_LIT.1, t) + over * 40.0,
                    mix(ORB_DARK.2, ORB_C_LIT.2, t) + over * 80.0,
                );
                // 球体 over 光晕（整盘 alpha As）
                (
                    mix(halo.0, sph.0, ORB_SPHERE_ALPHA),
                    mix(halo.1, sph.1, ORB_SPHERE_ALPHA),
                    mix(halo.2, sph.2, ORB_SPHERE_ALPHA),
                )
            } else {
                halo
            };
            // 加值 = 合成结果 − BG（裁剪 ≥0：暗面低于底 = 不贡献，只加光）
            let add = (
                (comp.0 - ORB_BG.0).max(0.0).round() as u32,
                (comp.1 - ORB_BG.1).max(0.0).round() as u32,
                (comp.2 - ORB_BG.2).max(0.0).round() as u32,
            );
            px.push((add.0 << 16) | (add.1 << 8) | add.2);
        }
    }
    OrbSprite { size, px }
}

/// 把 sprite 以 (cx,cy) 为心贴进帧缓冲（加法合成）：每通道饱和加
/// add*gain，越界裁剪。render 与逐像素钉共用的同源绘制入口
pub fn blit_orb_sprite(
    buf: &mut [u32],
    w: u32,
    h: u32,
    sprite: &OrbSprite,
    cx: f64,
    cy: f64,
    gain: f32,
) {
    if gain <= 0.0 {
        return;
    }
    let s = i64::from(sprite.size);
    let (ox, oy) = (cx as i64 - s / 2, cy as i64 - s / 2);
    for sy in 0..s {
        let py = oy + sy;
        if py < 0 || py >= i64::from(h) {
            continue;
        }
        for sx in 0..s {
            let px = ox + sx;
            if px < 0 || px >= i64::from(w) {
                continue;
            }
            let add = sprite.px[(sy * s + sx) as usize];
            if add == 0 {
                continue;
            }
            let idx = (py * i64::from(w) + px) as usize;
            let dst = buf[idx];
            let ch = |shift: u32| {
                let a = ((add >> shift) & 0xFF) as f32 * gain;
                (((dst >> shift) & 0xFF) + a.round() as u32).min(0xFF)
            };
            buf[idx] = (ch(16) << 16) | (ch(8) << 8) | ch(0);
        }
    }
}

/// 生产 sprite 双缓存（rs = ai_presence::ORB_RADIUS_PX）：halo_boost=false
/// → 光晕增益 1.0；true → 运行态光晕增益（HALO_GAIN_RUNNING）
fn orb_sprite(halo_boost: bool) -> &'static OrbSprite {
    static NORMAL: std::sync::OnceLock<OrbSprite> = std::sync::OnceLock::new();
    static BOOST: std::sync::OnceLock<OrbSprite> = std::sync::OnceLock::new();
    let slot = if halo_boost { &BOOST } else { &NORMAL };
    slot.get_or_init(|| {
        build_orb_sprite(
            f64::from(crate::ai_presence::ORB_RADIUS_PX),
            if halo_boost {
                f64::from(crate::ai_presence::HALO_GAIN_RUNNING)
            } else {
                1.0
            },
        )
    })
}

/// 绘制光球（TermEmu::render_orb 的实现本体）：gain ≤ 0 早退；halo_gain
/// > 1.0 选运行态光晕变体。绘制在终端网格之后（调用方顺序保证）
pub fn render(buf: &mut [u32], buf_w: u32, buf_h: u32, x: f64, y: f64, gain: f32, halo_gain: f32) {
    if gain <= 0.0 || buf_w == 0 || buf_h == 0 {
        return;
    }
    let sprite = orb_sprite(halo_gain > 1.0);
    blit_orb_sprite(buf, buf_w, buf_h, sprite, x, y, gain);
}

/// 把 sprite 以 (cx,cy) 为心写进 chrome 层画布（BAR-066 半透出版，
/// 2026-09-05 双层合成专用）：加法 sprite 在独立透明画布上没有真背景
/// 可加，条件 alpha（纯黑=空白）又把暗色增量强转成不透明像素——实机
/// 表现 = 光球背后一整块黑。这里改写 (α, E) 对：α = 加量最大通道，
/// E = 去预乘满亮色相。GPU 标准混合的加亮项 α·E ≡ add·gain（逐像素
/// 守恒），与加法合成的差异只剩雾尾 (1-α)·dst 的轻微压暗——球心
/// α→1 逐像素等价，雾尾视觉即普通 UI 辉光。softbuffer 单层路径仍走
/// blit_orb_sprite（真背景饱和加，不受此病）
pub fn blit_orb_sprite_alpha(
    buf: &mut [u32],
    w: u32,
    h: u32,
    sprite: &OrbSprite,
    cx: f64,
    cy: f64,
    gain: f32,
) {
    if gain <= 0.0 {
        return;
    }
    let s = i64::from(sprite.size);
    let (ox, oy) = (cx as i64 - s / 2, cy as i64 - s / 2);
    for sy in 0..s {
        let py = oy + sy;
        if py < 0 || py >= i64::from(h) {
            continue;
        }
        for sx in 0..s {
            let px = ox + sx;
            if px < 0 || px >= i64::from(w) {
                continue;
            }
            let add = sprite.px[(sy * s + sx) as usize];
            if add == 0 {
                continue;
            }
            let ch = |shift: u32| {
                let a = ((add >> shift) & 0xFF) as f32 * gain;
                (a.round() as u32).min(0xFF)
            };
            let (r, g, b) = (ch(16), ch(8), ch(0));
            let alpha = r.max(g).max(b);
            if alpha == 0 {
                continue;
            }
            let e = |v: u32| v * 255 / alpha;
            let idx = (py * i64::from(w) + px) as usize;
            buf[idx] = (alpha << 24) | (e(r) << 16) | (e(g) << 8) | e(b);
        }
    }
}

/// chrome 层半透渲染入口（render 的 over 层版，见 blit_orb_sprite_alpha）
pub fn render_alpha(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    x: f64,
    y: f64,
    gain: f32,
    halo_gain: f32,
) {
    if gain <= 0.0 || buf_w == 0 || buf_h == 0 {
        return;
    }
    let sprite = orb_sprite(halo_gain > 1.0);
    blit_orb_sprite_alpha(buf, buf_w, buf_h, sprite, x, y, gain);
}
