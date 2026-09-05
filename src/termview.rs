//! termview.rs — 终端视图：alacritty_terminal 网格 + fontdue 光栅 + softbuffer 直推
//!
//! 职责：包装 Term（转义序列/网格/滚屏全交给它），把当前可见网格软渲染进
//! u32 帧缓冲（XRGB）。零 I/O、零平台依赖——host 单测与 Android 壳共用一份。
//!
//! 判卷方式：
//! - A 档考题 tests/termview_spec.rs：布局数学纯函数（grid_dims / cell_origin）
//!   与颜色映射（ANSI 表 / indexed 256 色 / 反色交换）钉死，含变异抽检
//! - B 档冒烟钉（同文件）：feed 字节进真 Term，render_into 后断言帧缓冲
//!   出现非背景像素（字形真画出来了）、红色转义真出红像素、光标格真反色
//! - C 档实拍：手机终端画面（立项.md 尖刺验收 2/3）
//!
//! 已知留白（尖刺期不处理）：
//! - fallback 只有一节（主字体 + 一个 CJK 备用，prefer_cjk 按字形覆盖挑）；
//!   备用也缺的画 tofu（.notdef 方框），不 panic。多级链等实拍再议
//! - 每次 render_into 全量重绘，无 damage 增量（alacritty_terminal 自带
//!   damage 追踪，性能成为问题再接）

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};

/// 单元格像素尺寸基准（捏合缩放的 1.0 锚点 + 无持久化时的冷启动默认）。
/// 2026-08-13 实拍「字太小」：12x24 → 15x30；2026-08-21 用户两次抱怨
/// 「太小」：15x30 → 18x36（1080 屏净宽 1056px ≈ 58 列）
pub const CELL_W: u32 = 18;
pub const CELL_H: u32 = 36;

/// 起手网格几何（BAR-035）：开机横幅在首个 resize 到达前落笔,折行点
/// 由它定。真机(build_vendored)与 host 回放器(na-replay)必须同胚——
/// 各写一份数字迟早漂走,2026-08-25 终验实拍回放恒差 1 行折行
pub const BOOT_COLS: u32 = 80;
pub const BOOT_ROWS: u32 = 24;

/// 开局上机提示（2026-08-20 用户实拍：快捷键是 app 层的，shell 里 help
/// 看不见它们，要「至少一个提示」）。青色标题 + 灰说明，只 feed 视图
/// 不进 PTY；滚屏可回看，每次冷启动印一次。
/// **BAR-040 契约：必须在首个真实几何 resize 之后再印**（80 列印、61 列
/// 重排会折行 +2，标题两行被顶进 scrollback——2026-08-27 用户实拍）。
/// 住 termview 不住 android_app：横幅折行点由 BOOT_COLS 决定，与几何
/// 常量同文件同审（且 android_app 是 android feature 门控，host 考题
/// 够不着）
pub const HELP_BANNER: &str = "\x1b[36m── kfm-na 就绪 ──\x1b[0m\r\n\
\x1b[90m切换会话: CTRL+] 本地⇄远程 · 触摸: 点按唤键盘 / 滑动滚屏 / 双指缩放字号\x1b[0m\r\n\
\x1b[90m长按选词: 拖动扩选 / 按住边界精调(带放大镜) / 单击复制 · HOME/END 跳首尾 · PGUP/PGDN 翻页\x1b[0m\r\n\
\x1b[90m快捷键行: CTRL/ALT/SHIFT 点一下粘住再敲字母\x1b[0m\r\n\
\x1b[90m本地 HOME: Android/data/dev.kfm.na/files(文件管理器可见,随便读写)\x1b[0m\r\n";

/// 画面边距（BAR-005）：网格不贴边，边缘字符不再被屏幕圆角/曲面切半。
/// 纯黑带，不画框——框是装饰，等中央页面定稿再议
pub const MARGIN_X: u32 = 12;

/// AI 对话页排版尺（期 0④ 提升为模块级：手势 px→行换算与渲染同尺）
pub const AI_PAGE_MARGIN_X: u32 = 60;
pub const AI_PAGE_TOP: u32 = 48;
pub const AI_PAGE_BOTTOM: u32 = 48;
pub const AI_PAGE_LINE_H: u32 = 64;
pub const AI_PAGE_PX: f32 = 40.0;
/// 思考块文字色（期 0④½）：比正文暗的灰紫——能读到思考在流，但不抢戏
pub const AI_THINK_FG: u32 = 0x007E_7A9E;
/// 收流后思考块的折叠占位行（2026-09-04 用户拍板：输出完自动折叠——
/// 思考往往不重要但必须存在；全文随消息存档，展开查看是未来的活）
pub const AI_THINK_COLLAPSED: &str = "· 已思考 ·";
pub const MARGIN_Y: u32 = 12;

/// 顶边距（BAR-010）：圆角屏吃掉首行首字符（2026-08-13 实拍）——
/// 顶部在常规边距之上再下探一整行。这是基准格高（CELL_H）下的常量值；
/// 格高随捏合缩放变后必须走 margin_top(cell_h) 动态版
pub const MARGIN_TOP: u32 = MARGIN_Y + CELL_H;

/// 顶边距动态版（A 档考题钉死）：跟随当前格高——缩放任一档下顶带都是
/// 「常规边距 + 一整行」，圆角屏语义不随缩放漂移
pub const fn margin_top(cell_h: u32) -> u32 {
    MARGIN_Y + cell_h
}

/// 捏合缩放格尺寸钳制区间（A 档考题钉死）：10x20 = 还能认出字的下限，
/// 45x90 = 一屏 24 列 26 行的上限（再大打不了字）
pub const CELL_W_MIN: u32 = 10;
pub const CELL_W_MAX: u32 = 45;
pub const CELL_H_MIN: u32 = 20;
pub const CELL_H_MAX: u32 = 90;

/// 捏合比例 → 格尺寸（A 档考题钉死）：基准 × 比例四舍五入取整，钳到
/// 可读区间。非法比例（NaN/0/负/无穷）不落钳制结果而落基准本身——
/// 坏输入不许把字号打飞
pub fn pinch_cell_size(base_w: u32, base_h: u32, ratio: f64) -> (u32, u32) {
    if !ratio.is_finite() || ratio <= 0.0 {
        return (
            base_w.clamp(CELL_W_MIN, CELL_W_MAX),
            base_h.clamp(CELL_H_MIN, CELL_H_MAX),
        );
    }
    let w = (f64::from(base_w) * ratio).round() as u32;
    let h = (f64::from(base_h) * ratio).round() as u32;
    (
        w.clamp(CELL_W_MIN, CELL_W_MAX),
        h.clamp(CELL_H_MIN, CELL_H_MAX),
    )
}

/// 按字形覆盖挑备用字体（A 档考题钉死）：主字体缺该字（glyph_index=0）
/// 且备用字体有才换。字形存在性问 lookup_glyph_index——光栅有没有墨
/// 靠不住（DejaVu 缺字也画 tofu，有墨但不是对的字，host 实测 '中'
/// idx=0 ink=150）。盲文圆点（U+2800 盲文块，kimi code 转动点同款）
/// 就是这条链救的：DejaVuSansMono 没盲文，BBK fallback 顶班
pub fn prefer_cjk(primary: &fontdue::Font, cjk: &fontdue::Font, c: char) -> bool {
    primary.lookup_glyph_index(c) == 0 && cjk.lookup_glyph_index(c) != 0
}

/// AI 面板整页移位压盖（采样缝过渡帧专用，2026-09-04 弹簧落下）。
/// src = 整页面板渲染产物，y_off ∈ [-h, 0]：面板顶在屏上 y_off 行处
/// （负 = 屏外上方）——dst 的 [0, h+y_off) 行从 src **底部**对应行整行
/// 拷贝（面板底边从屏顶一路落下来）；y_off=0 即原样全盖（与直接渲染
/// 像素等价），y_off=-h 即完全屏外不动 dst。
/// BAR-062：初版把 src 顶部拷进 dst 底部（方向写反），真机实看是「从
/// 下往上升」——考题同谋钉了反方向，C 档实看才逮住。
pub fn blit_panel_shifted(dst: &mut [u32], src: &[u32], w: u32, h: u32, y_off: i32) {
    if w == 0 || h == 0 {
        return;
    }
    let (w, h) = (w as usize, h as usize);
    let y_off = y_off.clamp(-(h as i32), 0);
    let skip = (-y_off) as usize; // 屏外行数（面板顶被推到屏上多少行）
    let rows = h - skip; // 可见行数
    if rows == 0 {
        return;
    }
    dst[..rows * w].copy_from_slice(&src[skip * w..(skip + rows) * w]);
}

/// AI 面板分层判定（唯一裁决处，2026-09-05 GLES 双层合成立此为据）：
/// 三分支语义从 rasterize 的 if/else 链抽成真值表——
/// - 网格+快捷键行（下层可见）：panel_off != 0（原分支一/分支三）；
/// - 面板可见：panel_off > -h（原分支二/分支三）；
/// - 两者都真 = 过渡帧（终端在下、面板移位压上）；都不真不可能
///   （off <= -h 时必有键行，off == 0 时必有面板）。
///
/// softbuffer 路径与 GLES 路径都从这里取判定——分支语义漂移 = 眼手
/// 两张皮，BAR-063 级事故的温床
pub fn panel_split(panel_off: i32, h: u32) -> (bool, bool) {
    (panel_off != 0, panel_off > -(h as i32))
}

/// AI 页视口一屏行数（布局尺：render_ai_page / ai_page_glyphs / 底装修
/// 共用——原是 render_ai_page 里的一行算式，chrome 路径空态也要给
/// scroll_sync_layout 同尺读数，抽出来单源）
pub fn ai_page_fit(buf_h: u32, bottom_inset: u32) -> u32 {
    buf_h.saturating_sub(AI_PAGE_TOP + AI_PAGE_BOTTOM + bottom_inset) / AI_PAGE_LINE_H
}

/// AI 页底装修（2026-09-05 GLES 双层合成）：整页紫底 + 边框环，文字
/// 不在这层——GPU 路径的 z 序是 终端网格 → 下层（键行 + 本层）→
/// AI 文字实例 → 上层（输入栏/光球）。panel_off = 面板刚体平移（过渡
/// 帧整体移位，与 scratch+blit 时代像素等价；屏外部分裁剪零成本）。
/// 返回 fit（空态也要给 scroll_sync_layout 同尺读数）
pub fn paint_ai_page_chrome(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    bottom_inset: u32,
    panel_off: i32,
) -> u32 {
    if buf_w == 0 || buf_h == 0 {
        return 0;
    }
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    // 整页紫底 = 面板刚体矩形（全屏）与屏求交后画。fill 一定盖住环的
    // 全部行（环下缘之上还有 bottom_inset+margin 的紫底），环/发光在其
    // 上按原配方叠加——混合底色与 scratch 时代一致
    let py0 = panel_off.clamp(0, buf_h as i32) as u32;
    let py1 = (buf_h as i32 + panel_off).clamp(0, buf_h as i32) as u32;
    if py1 > py0 {
        frame.fill_rect(0, py0, buf_w, py1 - py0, AI_PAGE_BG);
    }
    paint_ai_frame_ring(&mut frame, buf_w, buf_h, bottom_inset, panel_off);
    ai_page_fit(buf_h, bottom_inset)
}

/// AI 页边框环（2026-09-04 装修配方的唯一实体，2026-09-05 平移参数化）：
/// 先紫外发光，再 135° 渐变外环，最后页面底色 punch 内芯（左缘让 9 =
/// 3 倍粗，其余让 3）。panel_off 整体平移（render_ai_page 传 0，
/// paint_ai_page_chrome 传面板偏移）——环是面板装修，跟面板一起动。
/// 空态也画：框是页面装修不是内容
fn paint_ai_frame_ring(
    frame: &mut Frame<'_>,
    buf_w: u32,
    buf_h: u32,
    bottom_inset: u32,
    panel_off: i32,
) {
    let fx0 = AI_PAGE_FRAME_MARGIN as i64;
    let fy0 = AI_PAGE_FRAME_MARGIN as i64 + i64::from(panel_off);
    let fx1 = (buf_w - AI_PAGE_FRAME_MARGIN) as i64;
    let fy1 = (buf_h - bottom_inset - AI_PAGE_FRAME_MARGIN) as i64 + i64::from(panel_off);
    if fx1 <= fx0 + 2 * i64::from(AI_PAGE_FRAME_R) || fy1 <= fy0 + 2 * i64::from(AI_PAGE_FRAME_R) {
        return;
    }
    let (fw, fh) = ((fx1 - fx0) as u32, (fy1 - fy0) as u32);
    // 发光（矩形外部，沿 SDF 向外二次衰减）——glow_round_rect 同款配方
    // （r 不钳、内部 d<=0 归主体、alpha*t² 衰减），只换裁剪 iteration
    let r = AI_PAGE_FRAME_R;
    let spread = 14u32;
    let (gc, ga) = (AI_PAGE_FRAME_C2, 64u32);
    paint_rr_clipped(frame, fx0, fy0, fw, fh, r, |cov, lx, ly| {
        let _ = cov;
        let d = rr_sdf(lx as f32 + 0.5, ly as f32 + 0.5, fw, fh, r);
        if d <= 0.0 {
            return None; // 内部归主体画
        }
        let t = (1.0 - d / spread as f32).max(0.0);
        let a = (ga as f32 * t * t) as u32;
        if a > 0 { Some((gc, a)) } else { None }
    });
    // 135° 渐变外环（fill_round_rect_grad diag 同式：t = lx+ly 归一）
    let denom = ((fw - 1) + (fh - 1)).max(1);
    let (c1, c2) = (AI_PAGE_FRAME_C1, AI_PAGE_FRAME_C2);
    paint_rr_clipped(frame, fx0, fy0, fw, fh, r, |cov, lx, ly| {
        if cov == 0 {
            return None;
        }
        let color = lerp_rgb(c1, c2, ((lx + ly) * 255 / denom).min(255));
        Some((color, cov))
    });
    // 内芯 punch（左缘 3 倍粗：x 让 3W，其余让 W）——fill_round_rect
    // 同款（cov==255 直写底色，弧边 blend）
    let w = AI_PAGE_FRAME_W;
    let ix = fx0 + i64::from(w) * 3;
    let iy = fy0 + i64::from(w);
    let iw = ((fx1 - i64::from(w)) - ix).max(0) as u32;
    let ih = ((fy1 - i64::from(w)) - iy).max(0) as u32;
    let punch_r = r - w;
    paint_rr_clipped(frame, ix, iy, iw, ih, punch_r, |cov, _lx, _ly| {
        if cov == 0 {
            None
        } else {
            Some((AI_PAGE_BG, cov))
        }
    });
}

/// i64 原点裁剪版圆角矩形墨刷（面板过渡帧专用）：ink(cov, lx, ly) →
/// Some((颜色, alpha))；cov==255 && alpha==255 直写，其余 blend_px
/// （与 Frame 同规）。相位保持——局部坐标 (lx, ly) 相对真实原点折算，
/// 只迭代屏内行；直接钳原点调 Frame 系列会把圆角弧与渐变相位一起
/// 错位（用户逐帧看动画，不许）
fn paint_rr_clipped(
    frame: &mut Frame<'_>,
    rx: i64,
    ry: i64,
    rw: u32,
    rh: u32,
    r_cover: u32,
    ink: impl Fn(u32, u32, u32) -> Option<(u32, u32)>,
) {
    let r = r_cover.min(rw / 2).min(rh / 2);
    let x0 = rx.max(0);
    let y0 = ry.max(0);
    let x1 = (rx + i64::from(rw)).min(i64::from(frame.w));
    let y1 = (ry + i64::from(rh)).min(i64::from(frame.h));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    for ay in y0..y1 {
        let ly = (ay - ry) as u32;
        for ax in x0..x1 {
            let lx = (ax - rx) as u32;
            if let Some((color, a)) = ink(rr_cover(lx, ly, rw, rh, r), lx, ly)
                && a > 0
            {
                if a == 255 {
                    frame.buf[ay as usize * frame.w as usize + ax as usize] = color;
                } else {
                    frame.blend_px(ax as u32, ay as u32, color, a);
                }
            }
        }
    }
}

/// 默认前景白 / 背景黑（softbuffer XRGB：高字节不用）
pub const DEFAULT_FG: u32 = 0x00FF_FFFF;
pub const DEFAULT_BG: u32 = 0x0000_0000;

/// ANSI 前 16 色表（XRGB）：0-7 普通，8-15 高亮。主体 VGA 经典配色；
/// **蓝系例外（2026-08-23 实拍）**:VGA #0000AA/#5555FF 在纯黑底上不可读
/// （ssh 远端 ls 目录名、help 标题看不清）——换 kfmv4 品牌蓝系
pub const ANSI_16: [u32; 16] = [
    0x0000_0000, // 黑
    0x00AA_0000, // 红
    0x0000_AA00, // 绿
    0x00AA_5500, // 黄（VGA 棕）
    0x003B_82F6, // 蓝 → kfmv4 品牌正蓝(原 VGA #0000AA 黑底不可读)
    0x00AA_00AA, // 品红
    0x0000_AAAA, // 青
    0x00AA_AAAA, // 白
    0x0055_5555, // 亮黑（灰）
    0x00FF_5555, // 亮红
    0x0055_FF55, // 亮绿
    0x00FF_FF55, // 亮黄
    0x0060_A5FA, // 亮蓝 → 品牌蓝亮一档(原 VGA #5555FF)
    0x00FF_55FF, // 亮品红
    0x0055_FFFF, // 亮青
    0x00FF_FFFF, // 亮白
];

/// 字体加载候选（按序取第一个及格的）：设备 CJK 优先，host 测试用 DejaVu
/// （12:09 真机普查补充：DroidSansFallbackBBK = vivo 的 fallback 字体，
/// DroidSansMono = 设备自带等宽——usable/monospaced 双判定会把关，
/// 不及格的自动跳过，最后落内嵌 DejaVuSansMono）
/// 2026-08-18 启动提速：DroidSansMono 提首（真机实证它就是胜者，
/// 108KB 秒杀）；NotoSansCJK.ttc/DroidSansFallback* 是几十 MB 巨物,
/// 反正过不了探针,留表尾靠 MAX_MAIN_FONT_BYTES 体积闸廉价跳过
pub const FONT_CANDIDATES: &[&str] = &[
    "/system/fonts/DroidSansMono.ttf",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/DroidSansFallbackBBK.ttf",
    "/system/fonts/Roboto-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];

/// 编译期内嵌的等宽兜底字体（BAR-003）：真机字体三连坑——NotoSansCJK.ttc
/// 空光栅（BAR-002）、Roboto 比例字体间距错乱、DroidSansFallbackFull 不存在。
/// 嵌一份及格的等宽字体进包，任何设备都有下限（选型/许可见 assets/fonts/README.md）
pub static VENDORED_MONO_FONT: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// 生产内嵌字体（BAR-021，build.rs 编译期选择：assets/fonts/local/ 覆盖 >
/// 开源占位，规则见 build.rs 头注）。启动零探测——不读 /system/fonts，
/// 不解析 44MB 巨物，TermView 毫秒级建成（启动慢病灶连根拔，BAR-020 终章）
pub static VENDORED_MAIN_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fonts/main.ttf"));
/// 生产内嵌 CJK 备用字体（同 build.rs 选择；全角双宽，覆盖 GB2312 全字库）
pub static VENDORED_CJK_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fonts/cjk.ttf"));

/// 字体可用性判定（A 档考题钉死）：光栅化探针字符，空字形（尺寸 0 或
/// 位图零覆盖）判不合格。背景：2026-08-13 真机实拍「只见光标不见字」——
/// NotoSansCJK-Regular.ttc from_bytes 成功却疑似光栅全空：能载 ≠ 能画
pub fn font_usable(font: &fontdue::Font, probe: char) -> bool {
    let (m, bmp) = font.rasterize(probe, CELL_H as f32);
    m.width > 0 && m.height > 0 && bmp.iter().any(|&a| a > 0)
}

/// 等宽判定（A 档考题钉死，BAR-003）：终端网格按定宽格摆字形，比例字体
/// （i 窄 m 宽）摆进去间距忽近忽远。'i' 与 'M' 步进宽相等才算终端可用
pub fn font_monospaced(font: &fontdue::Font) -> bool {
    let (mi, _) = font.rasterize('i', CELL_H as f32);
    let (mm, _) = font.rasterize('M', CELL_H as f32);
    (mi.advance_width - mm.advance_width).abs() < 0.5
}

/// 主字体体积闸（2026-08-18 启动慢实测：表面建成→TermView 建成 6 秒,
/// 病灶=每次启动全量解析 NotoSansCJK.ttc 32MB + DroidSansFallbackBBK
/// 44MB 再被探针扔掉)。等宽 Latin 主字体不可能是几十 MB 的巨物——
/// 超闸直接不解析,行为不变(它们本来就过不了 usable/mono 探针),
/// CJK 备用表不受此闸(那边的巨物是真字形源)
pub const MAX_MAIN_FONT_BYTES: u64 = 8 * 1024 * 1024;

/// 按候选顺序加载第一个可读、fontdue 认得、能画出字、且等宽的字体，
/// 返回 (来源路径, 字体)。路径候选全灭时落内嵌等宽字体（路径标记
/// "<内嵌>"）；内嵌也废（不可能，有钉）才返回 None。本函数不 panic。
pub fn load_font(candidates: &[&str]) -> Option<(String, fontdue::Font)> {
    for path in candidates {
        // 体积闸:metadata 即判,几十 MB 的巨物连读都不读
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_MAIN_FONT_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Ok(font) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
            // 能载不能画的（BAR-002 NotoSansCJK.ttc）与比例字体（BAR-003 Roboto）
            // 都跳过，给后面的候选机会
            if font_usable(&font, 'M') && font_monospaced(&font) {
                return Some((path.to_string(), font));
            }
        }
    }
    let font =
        fontdue::Font::from_bytes(VENDORED_MONO_FONT, fontdue::FontSettings::default()).ok()?;
    Some(("<内嵌>".to_string(), font))
}

/// CJK 备用字体候选（按序取第一个真能画出 '中' 的）：
/// HYQiHei = vivo 汉仪旗黑（12:09 真机普查实见），BBK/Monster = 国产 ROM
/// fallback 系；NotoSansCJK.ttc 空光栅（BAR-002）会被 usable 判定自动跳过；
/// 末位 host DejaVu 只供 host 测试（tofu 也有墨，链路可验证）。
/// 注意：usable 探针分不出 tofu 和真字形——所以主字体（内嵌 DejaVuSansMono）
/// 绝不能进这份清单，否则设备永远停在豆腐块
pub const CJK_FONT_CANDIDATES: &[&str] = &[
    "/system/fonts/HYQiHei-40_vivo-Design-02.ttf",
    "/system/fonts/DroidSansFallbackBBK.ttf",
    "/system/fonts/DroidSansFallbackMonster.ttf",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
];

/// 按候选顺序加载第一个真能画出 '中' 的 CJK 备用字体。全灭返回 None
/// （主字体的 tofu 顶班，不 panic）
pub fn load_cjk_font(candidates: &[&str]) -> Option<(String, fontdue::Font)> {
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Ok(font) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            && font_usable(&font, '中')
        {
            return Some((path.to_string(), font));
        }
    }
    None
}

/// 布局数学（A 档考题钉死）：窗口 px 尺寸 + 单元格 px 尺寸 → (cols, rows)。
/// 任一边为 0（窗口未出/单元格非法）或装不下一个格子 → 对应维度为 0。
pub fn grid_dims(win_w: u32, win_h: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    if cell_w == 0 || cell_h == 0 {
        return (0, 0);
    }
    (win_w / cell_w, win_h / cell_h)
}

/// 布局数学（A 档考题钉死）：格坐标 → 帧缓冲像素原点（左上角）。
pub fn cell_origin(col: u32, row: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    (col * cell_w, row * cell_h)
}

/// 坐标换算（A 档考题钉死）：帧缓冲像素 → 屏内格 (col, row)。
/// 渲染的反向：减边距 MARGIN_X 与顶带 margin_top(cell_h)（格高随缩放变，
/// 判定尺必须与 render_into 同一把）；越界（边距带内/网格外）钳到网格边缘
pub fn px_to_cell(x: f64, y: f64, cols: u32, rows: u32, cell_w: u32, cell_h: u32) -> (u32, u32) {
    let col = ((x - f64::from(MARGIN_X)) / f64::from(cell_w.max(1))).floor();
    let row = ((y - f64::from(margin_top(cell_h))) / f64::from(cell_h.max(1))).floor();
    let max_col = f64::from(cols.max(1)) - 1.0;
    let max_row = f64::from(rows.max(1)) - 1.0;
    (
        col.clamp(0.0, max_col) as u32,
        row.clamp(0.0, max_row) as u32,
    )
}

/// 词选择字符集（A 档考题钉死）：字母数字 + 常见路径字符 `_-./:~`
/// 连续段算一个词——长按选词就是要把路径/URL/选项串整段拎出来
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '~')
}

/// 选择区（网格坐标 (Line, Column)：行号含历史负行——滚进历史后选择
/// 跟着内容走，与 render_into 的 display_iter 行号同坐标系）。
/// anchor = 长按落点词首，cursor = 拖动当前点；归一化在判定/提取时做
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (i32, u32),
    pub cursor: (i32, u32),
}

/// 选择范围判定（A 档考题钉死）：anchor/cursor 归一化（反向拖也算），
/// 含端点的闭区间按 (行, 列) 字典序
pub fn in_selection(anchor: (i32, u32), cursor: (i32, u32), line: i32, col: u32) -> bool {
    let (s, e) = if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    (line, col) >= s && (line, col) <= e
}

/// xterm 256 色索引 → XRGB（A 档考题钉死边界）：
/// 0-15 走 ANSI 表；16-231 是 6×6×6 色立方；232-255 是 24 级灰阶。
pub fn indexed_color(n: u8) -> u32 {
    const LEVELS: [u32; 6] = [0, 95, 135, 175, 215, 255];
    match n {
        0..=15 => ANSI_16[n as usize],
        16..=231 => {
            let n = u32::from(n) - 16;
            let r = LEVELS[(n / 36) as usize];
            let g = LEVELS[((n / 6) % 6) as usize];
            let b = LEVELS[(n % 6) as usize];
            (r << 16) | (g << 8) | b
        }
        232..=255 => {
            let v = 8 + u32::from(n - 232) * 10;
            (v << 16) | (v << 8) | v
        }
    }
}

/// alacritty 颜色 → XRGB。命名色走表，前景/背景走默认，Spec 直包，
/// 未专门处理的（Cursor/Dim*/BrightForeground…）归默认前景。
pub fn color_to_xrgb(c: Color) -> u32 {
    match c {
        Color::Named(named) => match named {
            NamedColor::Foreground | NamedColor::BrightForeground => DEFAULT_FG,
            NamedColor::Background => DEFAULT_BG,
            // 0-15 顺序与 ANSI 表一致（vte 定义即如此），直接转索引
            n if (n as usize) < 16 => ANSI_16[n as usize],
            n if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                .contains(&(n as usize)) =>
            {
                // Dim 系：对应普通色减半亮度
                let base = ANSI_16[n as usize - NamedColor::DimBlack as usize];
                let (r, g, b) = (
                    ((base >> 16) & 0xFF) / 2,
                    ((base >> 8) & 0xFF) / 2,
                    (base & 0xFF) / 2,
                );
                (r << 16) | (g << 8) | b
            }
            _ => DEFAULT_FG, // Cursor 等：无画面语义的归前景
        },
        Color::Spec(rgb) => (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b),
        Color::Indexed(n) => indexed_color(n),
    }
}

/// 字号几何（A 档考题钉死）：给出 (光栅字号, 格内基线偏移)。
/// 约束一（BAR-001 基线对齐）：行盒(ascent-descent)装进格内并居中，
///   行盒比格高则按比例缩字号；
/// 约束二（宽度帽）：探针字符步进宽不得超过格宽，超了再缩——否则
///   相邻格字形互相渗透（放大字号后 DejaVuSansMono 自然超宽）
fn fit_probe_px(font: &fontdue::Font, probe: char, cell_w: u32, cell_h: u32) -> (f32, f32) {
    let px0 = cell_h as f32;
    match font.horizontal_line_metrics(px0) {
        Some(lm) if lm.ascent > 0.0 => {
            let line = lm.ascent - lm.descent; // descent 为负，相减即行盒高
            let mut px = if line > px0 { px0 * px0 / line } else { px0 };
            let (mm, _) = font.rasterize(probe, px);
            if mm.advance_width > cell_w as f32 {
                px *= cell_w as f32 / mm.advance_width;
            }
            let lm2 = font.horizontal_line_metrics(px).unwrap_or(lm);
            let pad = (px0 - (lm2.ascent - lm2.descent)).max(0.0) / 2.0;
            (px, pad + lm2.ascent)
        }
        // 无水平度量（极端字体）兜底：原字号 + 经验基线 80% 处
        _ => (px0, px0 * 0.8),
    }
}

/// 主字体（西文等宽）字号几何：宽度帽探针 'M'
pub fn fit_font_px(font: &fontdue::Font, cell_w: u32, cell_h: u32) -> (f32, f32) {
    fit_probe_px(font, 'M', cell_w, cell_h)
}

/// CJK 备用字体字号几何：全角字占两格，宽度帽探针 '中'（调用方传 2 倍格宽）
pub fn fit_cjk_px(font: &fontdue::Font, two_cell_w: u32, cell_h: u32) -> (f32, f32) {
    fit_probe_px(font, '中', two_cell_w, cell_h)
}

/// Term 尺寸适配器（alacritty_terminal::grid::Dimensions 的本地实现）
#[derive(Clone, Copy)]
struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// CJK 备用字体的字号几何（主字体的同款三件套，按两格宽适配）
/// 不止 CJK：主字体缺的都归它（盲文/符号），见 prefer_cjk
struct CjkStyle {
    font: fontdue::Font,
    px: f32,
    baseline_off: f32,
}

/// 终端视图：Term + vte 解析器 + 字体。事件用 VoidListener 空实现丢弃
/// （OSC52 剪贴板/标题改写等本切片不消费）。
pub struct TermView {
    term: Term<VoidListener>,
    processor: Processor,
    font: fontdue::Font,
    /// CJK 备用字体（fallback 链第一节）：主字体缺的字符归它画（prefer_cjk）；
    /// None = 主字体 tofu 顶班
    cjk: Option<CjkStyle>,
    /// tofu 目击名单（去重，16 格）：双字体都缺的字符攒着，android_app
    /// 定期取走上报——「那个方框到底是什么字」不问用户，问机器。
    /// RefCell：render_into 的 display_iter 借用着 term，draw_glyph 只能 &self
    tofu_seen: std::cell::RefCell<Vec<char>>,
    cell_w: u32,
    cell_h: u32,
    /// 实际光栅字号：行盒（ascent-descent）比格高时按比例缩小，保证装进格
    font_px: f32,
    /// 基线在格内的纵向偏移（格顶向下，px）——BAR-001 基线对齐用
    baseline_off: f32,
    /// 长按选择区（网格坐标，含历史负行）：Some = 选择模式激活，
    /// 渲染高亮 + 单击复制；None = 无选区
    selection: Option<Selection>,
    /// 设计 token（theme.rs 第 2 层）：控件渲染只读这里，不认字面颜色。
    /// pub = 主题包插件/考题可直接换肤；生产默认 kfmv4 配方
    pub theme: crate::theme::Theme,
}

/// AI 页一行展示行：(文字色, 该行的已量宽字符)——build_ai_rows 返回值的
/// 类型别名（clippy type_complexity 要求；inherent 关联类型不稳定，只能放模块级）
type AiRow<'a> = (u32, Vec<(&'a fontdue::Font, char, f32)>);

impl TermView {
    /// scrollback 容量(行)。2026-08-27 两线横向审计漂移 #1 用户拍板:
    /// 各线显式钉值——na 保持 10000(alacritty 上游默认原值,实证见
    /// 信箱 kfmv4-audit-term-parity-na-response.md):手机端核心场景是
    /// 长输出后上滑找错,1000 行级别是截肢;内存代价水位环实测可控
    /// (整机 rss ≈146-150MB,网格按行惰性分配)。**不许再悄悄继承
    /// 上游默认**——钉成常量,改它要走双向评审(term-contract 待立项)
    pub const SCROLLBACK_LINES: usize = 10_000;

    /// 建视图：cols/rows 为初始网格尺寸（窗口未出时给个占位，resize 随后到）。
    /// 任一为 0 会被钳到 1——alacritty Grid 不接受 0 维（会下溢 panic）。
    /// cjk_font 为 CJK 备用字体（可 None）
    pub fn new(
        font: fontdue::Font,
        cjk_font: Option<fontdue::Font>,
        cols: u32,
        rows: u32,
        cell_w: u32,
        cell_h: u32,
    ) -> Self {
        let size = TermSize {
            cols: (cols.max(1)) as usize,
            rows: (rows.max(1)) as usize,
        };
        let cell_h = cell_h.max(1);
        let cell_w = cell_w.max(1);
        // 基线几何（BAR-001）+ 宽度帽：见 fit_font_px/fit_cjk_px 文档
        let (font_px, baseline_off) = fit_font_px(&font, cell_w, cell_h);
        let cjk = cjk_font.map(|f| {
            let (px, bo) = fit_cjk_px(&f, cell_w * 2, cell_h);
            CjkStyle {
                font: f,
                px,
                baseline_off: bo,
            }
        });
        Self {
            // scrollback 显式钉值(SCROLLBACK_LINES 注释有出处)——
            // Config::default() 裸用 = 上游改默认我们跟着漂,审计漂移 #1
            // 的病根就是这个,不许回退
            term: Term::new(
                Config {
                    scrolling_history: Self::SCROLLBACK_LINES,
                    ..Config::default()
                },
                &size,
                VoidListener,
            ),
            processor: Processor::new(),
            font,
            cjk,
            tofu_seen: std::cell::RefCell::new(Vec::new()),
            cell_w,
            cell_h,
            font_px,
            baseline_off,
            selection: None,
            theme: crate::theme::Theme::default(),
        }
    }

    /// 运行期改格尺寸（双指捏合缩放，2026-08-21）：重算光栅字号/基线/
    /// CJK 备用三件套（逻辑同 new 的 fit_font_px/fit_cjk_px）。网格重排
    /// 不在此做——调用方随后 resize_cells（alacritty resize 自带 reflow）。
    /// 0 维钳 1，同 new；尺寸没变则不动（防抖链最后一环）
    pub fn set_cell_size(&mut self, cell_w: u32, cell_h: u32) {
        let cell_w = cell_w.max(1);
        let cell_h = cell_h.max(1);
        if (cell_w, cell_h) == (self.cell_w, self.cell_h) {
            return;
        }
        self.cell_w = cell_w;
        self.cell_h = cell_h;
        let (px, bo) = fit_font_px(&self.font, cell_w, cell_h);
        self.font_px = px;
        self.baseline_off = bo;
        if let Some(cjk) = &mut self.cjk {
            let (px, bo) = fit_cjk_px(&cjk.font, cell_w * 2, cell_h);
            cjk.px = px;
            cjk.baseline_off = bo;
        }
    }

    /// 喂 PTY 原始字节流（含 ANSI/UTF-8），vte 解析器驱动 Term 状态迁移
    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    /// 改网格尺寸（窗口 Resized 时调）。0 维钳 1，理由同 new。
    pub fn resize_cells(&mut self, cols: u32, rows: u32) {
        self.term.resize(TermSize {
            cols: (cols.max(1)) as usize,
            rows: (rows.max(1)) as usize,
        });
    }

    /// 字体探针（诊断用）：光栅化单字符，返回 (宽, 高, 非零覆盖像素数)。
    /// 真机「只见光标不见字」判卷：字体加载成功 ≠ 能出字形（2026-08-13 实拍，
    /// NotoSansCJK.ttc 载上了但疑似光栅全空）——数字传回，存在性说话
    pub fn font_probe(&self, c: char) -> (usize, usize, usize) {
        let (m, bmp) = self.font.rasterize(c, self.cell_h as f32);
        (m.width, m.height, bmp.iter().filter(|&&a| a > 0).count())
    }

    /// 取走 tofu 目击名单（清缓冲）：双字体都缺的字符，android_app 上报用
    pub fn take_tofu_chars(&self) -> Vec<char> {
        self.tofu_seen.take()
    }

    /// 滚动可视窗口（scrollback）：lines 正 = 看更老的历史（手指向下拖），
    /// 负 = 往最新回。alacritty 内部自钳到历史顶/底，调用方不用管边界
    pub fn scroll_lines(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// 回到底部贴最新输出（用户输入时调用——打字了就是要看现在，不是看历史）
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// 当前显示偏移（行，0 = 贴底）——B 档考题钉 + 实拍上报用
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// 当前 scrollback 已存行数（≤ SCROLLBACK_LINES）——容量考题与
    /// 观测用；内部 is_spacer/选区钳制早就在读它,只是没公开
    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    /// 网格光标所在列(0 基)——term-contract C4「同串→光标推进列数」
    /// 的判卷尺(nz 对拍用 measureCell 同一语义;评审教训:经 PTY/shell
    /// 注入测宽度会混入 zsh ZLE 转义回显,E0B0 实测被推 4 列,必须
    /// 直喂网格断 cursor)
    pub fn cursor_col(&self) -> usize {
        self.term.grid().cursor.point.column.0
    }

    /// 当前视野纯文本导出（调试闸门 text-req 通道，2026-08-24）：
    /// 可见区 = display_offset 起 screen_lines 行（滚动中跟视野走），
    /// 逐格收字符、跳过宽字符 spacer 半格，行尾 trim，行间 \n。
    /// v1 不导 scrollback——闸门只对齐「所见」（网格眼睛胚胎）
    pub fn dump_text(&self) -> String {
        let grid = self.term.grid();
        let off = grid.display_offset() as i32;
        let lines = grid.screen_lines() as i32;
        let cols = grid.columns();
        let mut out = String::with_capacity((lines as usize) * (cols / 2));
        for row in 0..lines {
            let grid_line = Line(row - off);
            let mut s = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[grid_line][Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue; // CJK 宽字符的后半格:字已在前半格收过
                }
                s.push(cell.c);
            }
            out.push_str(s.trim_end());
            out.push('\n');
        }
        out
    }

    /// 对端（tmux/kimicode 等 TUI）是否开了鼠标上报（?1000/1002/1003 任一）——
    /// 开了滚屏就必须翻成滚轮事件发过去（BAR-016：alt screen 没有本地历史）
    pub fn mouse_report_active(&self) -> bool {
        self.term.mode().intersects(
            TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
        )
    }

    /// 对端是否开了应用光标模式（?1h，vim/kimicode 会开）——快捷键行的
    /// 方向键/End 序列按它分岔（keymap.rs key_seq 的 app_cursor 参数）
    pub fn app_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// 单元格像素尺寸（android_app 用窗口尺寸反推 cols/rows 时取值）
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    // ---- 长按选择（2026-08-21，状态机/坐标约定见 docs/active/壳层交互.md） ----

    /// 选择模式激活中（有选区）——android_app 据此改路由：拖动 = 扩选，
    /// 单击 = 复制清选，点按唤键盘让路
    pub fn selection_active(&self) -> bool {
        self.selection.is_some()
    }

    /// 像素 → 网格点 (Line 含历史负行, Column)：屏格走 px_to_cell
    /// （边距/顶带同 render_into 一把尺），网格行 = 屏行 - display_offset
    /// （render_into 屏行 = 网格行 + display_offset 的逆运算）
    fn grid_point_at(&self, x: f64, y: f64) -> (i32, u32) {
        let grid = self.term.grid();
        let (col, row) = px_to_cell(
            x,
            y,
            grid.columns() as u32,
            grid.screen_lines() as u32,
            self.cell_w,
            self.cell_h,
        );
        let line = row as i32 - grid.display_offset() as i32;
        (line, col)
    }

    /// 该格是否 CJK 宽字符的 spacer 半格（宽字符占 col-1..col 两格，
    /// col 是 spacer）。行出界（含历史区）按 false 防御
    fn is_spacer(&self, line: i32, col: u32) -> bool {
        let grid = self.term.grid();
        let lo = -(grid.history_size() as i32);
        let hi = grid.screen_lines() as i32 - 1;
        if !(lo..=hi).contains(&line) {
            return false;
        }
        grid[Line(line)][Column(col as usize)]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
    }

    /// 宽字符边界钳制（2026-08-21 kfmv4 对齐）：端点落在 CJK spacer 半格
    /// 时按拖动方向钳——右移钳 col+1（越过该字到下一格），左移钳 col-1
    /// （回到该字格 0）。端点永不劈字。固有结果（实拍判卷点）：右拖终点
    /// 到 spacer 会把后一格也包进选区（后一格非空白时多选一个字）；
    /// 提取本就不收 spacer（selected_text 跳过），钳制前后提取等价
    /// （一致性考题 spec_选择_宽字符钳制提取一致性 钉死）
    fn clamp_wide_endpoint(&self, point: (i32, u32), moving_right: bool) -> (i32, u32) {
        let (line, col) = point;
        if !self.is_spacer(line, col) {
            return point;
        }
        if moving_right {
            let last = self.term.grid().columns() as u32 - 1;
            (line, (col + 1).min(last))
        } else {
            (line, col - 1) // spacer 的格 0 必在 col-1（col ≥ 1）
        }
    }

    /// 长按选词：落点所在词（is_word_char 连续段）整段选中；落点非词
    /// 字符（空白/标点）只选该格。落点在 CJK spacer 半格 → 当作按在该字
    /// 格 0（按下无方向，归字内）；词尾是宽字符格 0 时把它的 spacer 格
    /// 带进选区（端点落整字边界，渲染/提取同尺不劈字）。滚进历史后选的
    /// 就是历史行（坐标含 display_offset，见 grid_point_at）
    pub fn select_word_at(&mut self, x: f64, y: f64) {
        let (line, col) = self.grid_point_at(x, y);
        let col = if self.is_spacer(line, col) {
            col - 1
        } else {
            col
        };
        let cols = self.term.grid().columns() as u32;
        let at = |c: u32| self.term.grid()[Line(line)][Column(c as usize)].c;
        let (mut start, mut end) = (col, col);
        if is_word_char(at(col)) {
            while start > 0 && is_word_char(at(start - 1)) {
                start -= 1;
            }
            while end + 1 < cols && is_word_char(at(end + 1)) {
                end += 1;
            }
        }
        if end + 1 < cols && self.is_spacer(line, end + 1) {
            end += 1; // 词尾宽字符：带上它的 spacer 格
        }
        self.selection = Some(Selection {
            anchor: (line, start),
            cursor: (line, end),
        });
    }

    /// 选择模式拖动扩选：cursor 端跟手指走（落 spacer 半格按拖动方向
    /// 钳，见 clamp_wide_endpoint），跨行/反向/历史区同尺
    /// （归一化在 in_selection/selected_text 做）。反向拖过 anchor 时
    /// 固定端翻转到原词另一端——整词保持在选区内（选词后上拖不收掉半词）
    pub fn extend_selection(&mut self, x: f64, y: f64) {
        let Some(mut sel) = self.selection else {
            return;
        };
        let raw = self.grid_point_at(x, y);
        let point = self.clamp_wide_endpoint(raw, raw >= sel.cursor);
        if (point < sel.anchor && sel.cursor >= sel.anchor)
            || (point > sel.anchor && sel.cursor < sel.anchor)
        {
            sel.anchor = sel.cursor;
        }
        sel.cursor = point;
        self.selection = Some(sel);
    }

    /// 清高亮（复制后/会话重开等）
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// 考题探针：绕过宽字符边界钳制直接摆放选区端点——一致性考题拿它
    /// 把端点人为放到 spacer 半格上，比对「raw 提取 ≡ 钳后提取」
    /// （spec_选择_宽字符钳制提取一致性）。生产路径不走这里
    #[doc(hidden)]
    pub fn set_selection_raw(&mut self, anchor: (i32, u32), cursor: (i32, u32)) {
        self.selection = Some(Selection { anchor, cursor });
    }

    /// 提取选中文字（复制用）：归一化区间逐行收 cell.c——tab 本体在格内
    /// 原样还原（BAR-015：put_tab 写的就是 '\t'）；宽字符占位格跳过；
    /// zerowidth 组合符带上；行尾空白 trim，行间补 \n。无选区 → None
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (s, e) = if sel.anchor <= sel.cursor {
            (sel.anchor, sel.cursor)
        } else {
            (sel.cursor, sel.anchor)
        };
        let grid = self.term.grid();
        let last_col = grid.columns() as u32 - 1;
        // 防御钳制：选区存活期间滚屏/新输出可能让行号出界
        let lo = -(grid.history_size() as i32);
        let hi = grid.screen_lines() as i32 - 1;
        let last_line = e.0.min(hi);
        let mut out = String::new();
        for l in s.0.max(lo)..=last_line {
            let from = if l == s.0 { s.1 } else { 0 };
            let to = if l == e.0 { e.1 } else { last_col };
            let mut line = String::new();
            for c in from..=to.min(last_col) {
                let cell = &grid[Line(l)][Column(c as usize)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                line.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    for &z in zw {
                        line.push(z);
                    }
                }
            }
            out.push_str(line.trim_end());
            if l < last_line {
                out.push('\n');
            }
        }
        Some(out)
    }

    /// 触点命中选区哪一端（边界直拖——2026-08-21 拖柄废除改此：水滴柄
    /// 丑且白占一行高）：触点格与归一化起/止端格的行差、列差都 ≤1
    /// （触控宽容，手指不是鼠标）即算抓住；两端同圈（相邻格小选区）取
    /// 距触点像素近的一端，等距钉 Start（确定性规则，考题钉死）。
    /// 屏外端点天然抓不到（触点总在屏内，距离必超圈）。无选区 → None
    pub fn hit_boundary(&self, x: f64, y: f64) -> Option<SelEnd> {
        let sel = self.selection?;
        let (s, e) = if sel.anchor <= sel.cursor {
            (sel.anchor, sel.cursor)
        } else {
            (sel.cursor, sel.anchor)
        };
        let (line, col) = self.grid_point_at(x, y);
        let near =
            |end: (i32, u32)| (end.0 - line).abs() <= 1 && (end.1 as i32 - col as i32).abs() <= 1;
        // 平局裁决：端点格心距触点的像素距离（行换算回屏行 = +display_offset）
        let dist = |end: (i32, u32)| {
            let cx = f64::from(MARGIN_X + end.1 * self.cell_w) + f64::from(self.cell_w) / 2.0;
            let row = end.0 + self.term.grid().display_offset() as i32;
            let cy =
                f64::from(margin_top(self.cell_h)) + (row as f64 + 0.5) * f64::from(self.cell_h);
            (x - cx).powi(2) + (y - cy).powi(2)
        };
        match (near(s), near(e)) {
            (true, true) => Some(if dist(s) <= dist(e) {
                SelEnd::Start
            } else {
                SelEnd::End
            }),
            (true, false) => Some(SelEnd::Start),
            (false, true) => Some(SelEnd::End),
            (false, false) => None,
        }
    }

    /// 拖动选区边界移动端点：归一化起/止端谁被拖就谁跟手指（网格坐标换算
    /// 沿用 grid_point_at——跨行/历史区同尺；落 spacer 半格按拖动方向钳，
    /// 方向 = 新落点 vs 该端旧位置字典序）。拖过另一端则角色互换
    /// （起点拖过终点 → 它变成新终点），选区不塌缩翻转
    pub fn move_selection_end(&mut self, which: SelEnd, x: f64, y: f64) {
        let Some(sel) = self.selection else { return };
        let (s, e) = if sel.anchor <= sel.cursor {
            (sel.anchor, sel.cursor)
        } else {
            (sel.cursor, sel.anchor)
        };
        let raw = self.grid_point_at(x, y);
        let old = match which {
            SelEnd::Start => s,
            SelEnd::End => e,
        };
        let p = self.clamp_wide_endpoint(raw, raw >= old);
        self.selection = Some(match which {
            SelEnd::Start => {
                if p <= e {
                    Selection {
                        anchor: p,
                        cursor: e,
                    }
                } else {
                    Selection {
                        anchor: e,
                        cursor: p,
                    }
                }
            }
            SelEnd::End => {
                if p >= s {
                    Selection {
                        anchor: s,
                        cursor: p,
                    }
                } else {
                    Selection {
                        anchor: p,
                        cursor: s,
                    }
                }
            }
        });
    }

    /// 放大镜（边界拖动中，android_app 在主渲染+快捷键行之后调用）：
    /// 触点正下方那格为中心，±MAG_HALF_COLS 格 × ±MAG_HALF_ROWS 行的
    /// 帧缓冲源区最近邻 MAG_ZOOM 倍贴进带边框的圆角浮窗，默认浮在触点
    /// 上方（MAG_GAP_PX 间距不挡手）；上方放不下翻转到触点下方，两侧都
    /// 放不下才钳屏内。源区出屏部分留衬底黑
    pub fn render_magnifier(&self, buf: &mut [u32], buf_w: u32, buf_h: u32, x: f64, y: f64) {
        if buf_w == 0 || buf_h == 0 || buf.len() < (buf_w * buf_h) as usize {
            return;
        }
        // 源区中心 = 触点正下方那格的格心（不是触点本身——对齐到格，
        // 用户看清的是「端点正往哪个字符上放」）
        let (col, row) = px_to_cell(
            x,
            y,
            self.term.grid().columns() as u32,
            self.term.grid().screen_lines() as u32,
            self.cell_w,
            self.cell_h,
        );
        let cx = f64::from(MARGIN_X + col * self.cell_w) + f64::from(self.cell_w) / 2.0;
        let cy =
            f64::from(margin_top(self.cell_h) + row * self.cell_h) + f64::from(self.cell_h) / 2.0;
        let src_hw = MAG_HALF_COLS * self.cell_w; // 源区半宽（px）
        let src_hh = MAG_HALF_ROWS * self.cell_h;
        let win_w = src_hw * 2 * MAG_ZOOM;
        let win_h = src_hh * 2 * MAG_ZOOM;
        if win_w == 0 || win_h == 0 || win_w > buf_w || win_h > buf_h {
            return; // 窗比屏大（极端小窗）：保命不画
        }
        // 浮窗位置：水平对触点居中；默认浮触点上方 MAG_GAP_PX 不挡手——
        // 上方放不下（触点贴屏顶）翻转到触点下方（2026-08-21 实拍：贴顶
        // 拖动时旧钳制把浮窗压到屏顶盖住触点，看不见 = 失控）；两侧都
        // 放不下（极端矮屏）才退回屏内钳制保命
        const BORDER: u32 = 2;
        let win_x = (x as i64 - (win_w / 2) as i64).clamp(0, (buf_w - win_w) as i64) as u32;
        let above_y = y as i64 - i64::from(MAG_GAP_PX) - win_h as i64;
        let win_y = if above_y >= 0 {
            above_y as u32
        } else {
            let below_y = y as i64 + i64::from(MAG_GAP_PX);
            if below_y + win_h as i64 <= i64::from(buf_h) {
                below_y as u32
            } else {
                above_y.clamp(0, (buf_h - win_h) as i64) as u32
            }
        };
        // 先把源区拷出来（读写同一块 buf，不拷会自踩）
        let src_x0 = (cx as i64 - src_hw as i64).max(0);
        let src_y0 = (cy as i64 - src_hh as i64).max(0);
        let src_x1 = (cx as i64 + src_hw as i64).min(i64::from(buf_w) - 1);
        let src_y1 = (cy as i64 + src_hh as i64).min(i64::from(buf_h) - 1);
        let (rw, rh) = (
            (src_x1 - src_x0 + 1).max(0) as u32,
            (src_y1 - src_y0 + 1).max(0) as u32,
        );
        let mut region = vec![DEFAULT_BG; (rw * rh) as usize];
        for ry in 0..rh {
            let sy = src_y0 + i64::from(ry);
            for rx in 0..rw {
                let sx = src_x0 + i64::from(rx);
                region[(ry * rw + rx) as usize] = buf[(sy * i64::from(buf_w) + sx) as usize];
            }
        }
        // 边框 + 圆角（外圈），内容贴进内圈
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        frame.fill_round_rect(
            win_x.saturating_sub(BORDER),
            win_y.saturating_sub(BORDER),
            win_w + 2 * BORDER,
            win_h + 2 * BORDER,
            14,
            MAG_BORDER,
        );
        // 最近邻放大：dest 像素 (dx,dy) ← 源 (cx + (dx - win_w/2)/ZOOM, …)
        for dy in 0..win_h {
            let sy = (cy + (f64::from(dy) - win_h as f64 / 2.0) / MAG_ZOOM as f64).round() as i64;
            if sy < src_y0 || sy > src_y1 {
                continue; // 源区外（屏外）：留衬底/边框
            }
            for dx in 0..win_w {
                let sx =
                    (cx + (f64::from(dx) - win_w as f64 / 2.0) / MAG_ZOOM as f64).round() as i64;
                if sx < src_x0 || sx > src_x1 {
                    continue;
                }
                let px = region[((sy - src_y0) as u32 * rw + (sx - src_x0) as u32) as usize];
                frame.buf[((win_y + dy) * buf_w + win_x + dx) as usize] = px;
            }
        }
    }

    /// 把当前可见网格渲染进 XRGB 帧缓冲（黑底，满幅重绘）。
    /// buf 尺寸必须与 buf_w*buf_h 一致（调用方 softbuffer 保证；不一致只画放得下的部分）。
    pub fn render_into(&mut self, buf: &mut [u32], buf_w: u32, buf_h: u32) {
        buf.fill(DEFAULT_BG);
        if buf_w == 0 || buf_h == 0 {
            return;
        }
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        let content = self.term.renderable_content();
        let cursor = content.cursor;
        let selection = self.selection; // Copy 出来，与 content 的 term 借用拆开
        // 屏行 = 网格行 + 显示偏移（BAR-016）：滚进历史后 alacritty 给的行号
        // 是负的（Line(-offset)），跳过或直接用绝对行号都会让内容不随偏移
        // 移动、每滚一行底部黑一行（实拍「从下到上一行行消失」）
        let offset = content.display_offset as i32;
        // 两遍绘制（2026-08-21 实拍「选中态中文只剩左半」病灶）：先全部背景
        // （含选择高亮），后全部字形。一遍绘制时宽字符（CJK）在格 0 画双宽
        // 字形、墨探进格 1，随后 spacer 格的背景填充（选中=SELECT_BG）把
        // 右半字形盖掉——两遍制让一切背景都在字形之下
        struct Cell2D {
            px: u32,
            py: u32,
            fg: u32,
            bg: u32,
            c: char,
            flags: Flags,
        }
        let mut cells: Vec<Cell2D> = Vec::new();
        for indexed in content.display_iter {
            let line = indexed.point.line.0 + offset;
            if !(0..self.term.grid().screen_lines() as i32).contains(&line) {
                continue; // 钳到屏内（防御：迭代区间理论上已对齐）
            }
            let (mut fg, mut bg) = (
                color_to_xrgb(indexed.cell.fg),
                color_to_xrgb(indexed.cell.bg),
            );
            if indexed.cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            // 长按选择高亮：选中格盖选择底色（与网格行同坐标系，滚屏自动跟随）。
            // 宽字符整字扩边：spacer 的格 0 选中 → spacer 也亮；格 0 的 spacer
            // 选中（选词带 spacer 收尾）→ 格 0 也亮——任何钳法下都不劈字
            if let Some(sel) = selection {
                let (line0, col0) = (indexed.point.line.0, indexed.point.column.0 as u32);
                let selected = in_selection(sel.anchor, sel.cursor, line0, col0)
                    || (col0 > 0
                        && indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                        && in_selection(sel.anchor, sel.cursor, line0, col0 - 1))
                    || (indexed.cell.flags.contains(Flags::WIDE_CHAR)
                        && in_selection(sel.anchor, sel.cursor, line0, col0 + 1));
                if selected {
                    bg = SELECT_BG;
                }
            }
            let is_cursor = cursor.shape != CursorShape::Hidden && indexed.point == cursor.point;
            if is_cursor {
                std::mem::swap(&mut fg, &mut bg);
            }
            let (px, py) = cell_origin(
                indexed.point.column.0 as u32,
                line as u32,
                self.cell_w,
                self.cell_h,
            );
            // BAR-005：格原点加边距，网格不贴边（边距带留黑）；
            // BAR-010：顶部走动态顶带 margin_top（圆角屏下探一整行，
            // 格高随捏合缩放变，顶带跟格高走）
            let (px, py) = (px + MARGIN_X, py + margin_top(self.cell_h));
            if px >= buf_w || py >= buf_h {
                continue; // 窗口比网格小（resize 途中）：裁掉放不下的格
            }
            cells.push(Cell2D {
                px,
                py,
                fg,
                bg,
                c: indexed.cell.c,
                flags: indexed.cell.flags,
            });
        }
        // 第一遍：背景。不满格重画（全帧已填 DEFAULT_BG），非默认背景补色块
        for cell in &cells {
            if cell.bg != DEFAULT_BG {
                frame.fill_rect(cell.px, cell.py, self.cell_w, self.cell_h, cell.bg);
            }
        }
        // 第二遍：字形。空格/控制符（BAR-015：tab 本体）无字形不画；
        // 宽字符第二格（spacer）不画。裁剪宽：宽字符 2 格，其余 1 格——
        // 模糊宽度字符（如 ⇄，宽度判 1 格但 CJK 备用字体是全角字形）的
        // 墨不许溢进下一格（2026-08-21 实拍）
        for cell in &cells {
            if !paintable(cell.c) || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let clip_w = if cell.flags.contains(Flags::WIDE_CHAR) {
                self.cell_w * 2
            } else {
                self.cell_w
            };
            self.draw_glyph(&mut frame, cell.c, cell.px, cell.py, cell.fg, clip_w);
        }
    }

    /// GPU 网格收集（期 1 第 2 层）：把 render_into 的收集段（颜色决策
    /// ——INVERSE/选择高亮/光标 swap——与几何裁剪原样复制）产成 GpuCell，
    /// 供 GLES 路径 grid_to_instances 用。与 render_into 的字形/背景
    /// 两遍制逐语义对齐——对拍验收就在这两份代码的咬合上。
    /// 注意：只收集，不光栅化（光栅化归 rasterize_for_atlas，图集未
    /// 命中才调，命中走缓存——第 2 层的性能来源）。
    pub fn collect_gpu_cells(&mut self, w: u32, h: u32) -> Vec<crate::glyph_atlas::GpuCell> {
        use crate::glyph_atlas::GpuCell;
        let mut out = Vec::new();
        if w == 0 || h == 0 {
            return out;
        }
        let content = self.term.renderable_content();
        let cursor = content.cursor;
        let selection = self.selection;
        let offset = content.display_offset as i32;
        let margin_top = margin_top(self.cell_h);
        for indexed in content.display_iter {
            let line = indexed.point.line.0 + offset;
            if !(0..self.term.grid().screen_lines() as i32).contains(&line) {
                continue;
            }
            let (mut fg, mut bg) = (
                color_to_xrgb(indexed.cell.fg),
                color_to_xrgb(indexed.cell.bg),
            );
            if indexed.cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if let Some(sel) = selection {
                let (line0, col0) = (indexed.point.line.0, indexed.point.column.0 as u32);
                let selected = in_selection(sel.anchor, sel.cursor, line0, col0)
                    || (col0 > 0
                        && indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                        && in_selection(sel.anchor, sel.cursor, line0, col0 - 1))
                    || (indexed.cell.flags.contains(Flags::WIDE_CHAR)
                        && in_selection(sel.anchor, sel.cursor, line0, col0 + 1));
                if selected {
                    bg = SELECT_BG;
                }
            }
            let is_cursor = cursor.shape != CursorShape::Hidden && indexed.point == cursor.point;
            if is_cursor {
                std::mem::swap(&mut fg, &mut bg);
            }
            let (px, py) = cell_origin(
                indexed.point.column.0 as u32,
                line as u32,
                self.cell_w,
                self.cell_h,
            );
            let (px, py) = (px + MARGIN_X, py + margin_top);
            if px >= w || py >= h {
                continue;
            }
            out.push(GpuCell {
                px,
                py,
                fg,
                bg,
                c: indexed.cell.c,
                wide: indexed.cell.flags.contains(Flags::WIDE_CHAR),
                spacer: indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER),
            });
        }
        out
    }

    /// 图集供墨（期 1 第 2 层）：字体路由（prefer_cjk）+ 光栅化 + 放置
    /// 偏移一次给出；tofu 目击记账与 draw_glyph 同款（主字体缺字形且
    /// CJK 也不覆盖 → 上报名单）。None = 空字形（fontdue 空 位图），
    /// 调用方跳过装载（图集契约：空字形不进）。
    pub fn rasterize_for_atlas(
        &self,
        c: char,
    ) -> Option<(u8, fontdue::Metrics, Vec<u8>, i16, i16)> {
        let cjk_px = self.cjk.as_ref().map_or(0.0, |k| k.px);
        let (font_id, metrics, bitmap) = self.rasterize_for_atlas_px(c, self.font_px, cjk_px)?;
        // off_y 与 draw_glyph 的 top 推导同式：top = py + baseline - ymin - h
        // （终端约定：每字体各自的格基线）
        let baseline = if font_id == 1 {
            self.cjk
                .as_ref()
                .map_or(self.baseline_off, |k| k.baseline_off)
        } else {
            self.baseline_off
        };
        let off_y = baseline - metrics.ymin as f32 - metrics.height as f32;
        Some((font_id, metrics, bitmap, metrics.xmin as i16, off_y as i16))
    }

    /// 泛化供墨核心（2026-09-05 C 档字号参数化）：路由（prefer_cjk）+
    /// tofu 记账与终端供墨同款，光栅字号由调用方给——终端（font_px /
    /// cjk.px 各归各）与 AI 页（AI_PAGE_PX 一刀切，draw_items_left 画
    /// AI 文字就是单一 px）共用这一份。off 不在这算：终端烤格基线、
    /// AI 页烤行基线（ai_text_baseline_off），两种约定调用方各自折算
    /// 后走 atlas_insert。None = 空字形（图集契约：空字形不进）
    pub fn rasterize_for_atlas_px(
        &self,
        c: char,
        px: f32,
        px_cjk: f32,
    ) -> Option<(u8, fontdue::Metrics, Vec<u8>)> {
        if self.font.lookup_glyph_index(c) == 0 {
            let covered = self
                .cjk
                .as_ref()
                .is_some_and(|k| k.font.lookup_glyph_index(c) != 0);
            let mut seen = self.tofu_seen.borrow_mut();
            if !covered && !seen.contains(&c) && seen.len() < 16 {
                seen.push(c);
            }
        }
        let (font_id, font, size_px) = match &self.cjk {
            Some(cjk) if prefer_cjk(&self.font, &cjk.font, c) => (1u8, &cjk.font, px_cjk),
            _ => (0u8, &self.font, px),
        };
        let (metrics, bitmap) = font.rasterize(c, size_px);
        if metrics.width == 0 || metrics.height == 0 {
            return None;
        }
        Some((font_id, metrics, bitmap))
    }

    /// AI 页行基线（相对行顶）：draw_items_left 的 baseline 公式在
    /// (AI_PAGE_PX, AI_PAGE_LINE_H) 下的读数——off_y 装载折算的唯一
    /// 尺子（实例收集只管行顶，基线归槽位偏移，两处各算各的 = 错位）
    pub fn ai_text_baseline_off(&self) -> f32 {
        match self.font.horizontal_line_metrics(AI_PAGE_PX) {
            Some(hm) => (AI_PAGE_LINE_H as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent,
            None => 0.0,
        }
    }

    /// AI 全屏页真对话渲染（期 0③，取代占位空壳；合成网格美化是期 0⑤）。
    /// 简版纯文本消息行：角色标签行（你=青 / AI=浅紫）+ 正文折行（输入栏
    /// 同款 wrap_starts 贪心断行）。
    /// scroll_rows = 距底行数（期 0④ 视口，ui/ai_page.rs 状态机的读数）：
    /// 0 = 尾随锁定贴底；>0 = 视口上移看历史。返回（总行数, 一屏行数）
    /// ——调用方写回 AiChatState.scroll_sync_layout（眼手同尺：手势钳制
    /// 与渲染用同一份布局）。
    /// bottom_inset = 视口下沿让位（键盘高 + 输入栏当前带高，2026-09-04
    /// 用户拍板：键盘弹起时追底追到输入栏上沿，不许越过栏带往下画）；
    /// live_tail = 末条思考相位进行中（思考块 ≤3 行活窗）；false =
    /// 思考已结束（正文开始或整轮收流），折叠成一行暗色「已思考」
    /// （存档不丢，用户拍板：思考一结束立即折，不等整轮收流）
    #[allow(clippy::too_many_arguments)]
    pub fn render_ai_page(
        &self,
        buf: &mut [u32],
        buf_w: u32,
        buf_h: u32,
        msgs: &[(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
    ) -> (u32, u32) {
        if buf_w == 0 || buf_h == 0 {
            return (0, 0);
        }
        buf.fill(AI_PAGE_BG);
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        // 边框（2026-09-04 用户拍板装修，仿 kfmv4 orb-panel）：配方在
        // paint_ai_frame_ring（GPU chrome 路径 paint_ai_page_chrome 共用
        // 这一份——修配方两处一起修）。off=0：CPU 路径不平移
        paint_ai_frame_ring(&mut frame, buf_w, buf_h, bottom_inset, 0);
        let (rows, fit, skip) =
            self.ai_page_layout(buf_w, buf_h, msgs, scroll_rows, bottom_inset, live_tail);
        for (i, (fg, items)) in rows.iter().skip(skip).take(fit as usize).enumerate() {
            let y = AI_PAGE_TOP + i as u32 * AI_PAGE_LINE_H;
            self.draw_items_left(
                &mut frame,
                items,
                AI_PAGE_MARGIN_X,
                buf_w.saturating_sub(AI_PAGE_MARGIN_X * 2),
                y,
                AI_PAGE_LINE_H,
                AI_PAGE_PX,
                *fg,
                None,
            );
        }
        (rows.len() as u32, fit)
    }

    /// AI 页视口布局（单源，2026-09-05 从 render_ai_page 抽出）：折行 +
    /// 思考活窗/折叠 + 视口 skip——CPU 画（render_ai_page）与 GPU 实例
    /// 收集（ai_page_glyphs）共用这一份，眼手同尺的物质基础。返回
    /// （全部展示行，一屏行数，跳过行数）
    fn ai_page_layout<'a>(
        &'a self,
        buf_w: u32,
        buf_h: u32,
        msgs: &'a [(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
    ) -> (Vec<AiRow<'a>>, u32, usize) {
        let fit = ai_page_fit(buf_h, bottom_inset);
        let rows = self.build_ai_rows(msgs, buf_w, live_tail);
        // 视口：贴底基线 - 距底行数（期 0④——期 0③ 是整行丢弃没有视口）
        let base_skip = rows.len().saturating_sub(fit as usize);
        let skip = base_skip.saturating_sub(scroll_rows as usize);
        (rows, fit, skip)
    }

    /// AI 页文字 → GPU 字形收集（期 1 第 2 层 C 档：AI 页接入图集管线，
    /// 病根是 CPU 逐字 fontdue 光栅化每帧 48ms）。布局与 render_ai_page
    /// 同源（ai_page_layout）；画字语义与 draw_items_left 逐条对齐——
    /// 起笔内缩 18、主字体行尺垂直居中、右缘装不下即 break、不可上屏
    /// 字符（空格/控制符，BAR-015）不落墨只推笔。返回（布局读数, 字形
    /// 列表）：读数喂 scroll_sync_layout（眼手同尺），列表归调用方经
    /// 图集转实例（xmin/off_y 槽位偏移在 ai_glyphs_to_instances 补）。
    /// panel_off 直接加进行 y（面板刚体平移——2026-09-05 拍板：过渡帧
    /// 不再 scratch 全页渲染 + blit）
    #[allow(clippy::too_many_arguments)]
    pub fn ai_page_glyphs(
        &self,
        buf_w: u32,
        buf_h: u32,
        msgs: &[(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
        panel_off: i32,
    ) -> ((u32, u32), Vec<crate::glyph_atlas::AiGlyph>) {
        let (rows, fit, skip) =
            self.ai_page_layout(buf_w, buf_h, msgs, scroll_rows, bottom_inset, live_tail);
        let mut out = Vec::new();
        // 行尺 None（字体无横向量尺）= draw_items_left 同款空转，零实例
        if self.font.horizontal_line_metrics(AI_PAGE_PX).is_some() {
            let clip_right = buf_w.saturating_sub(AI_PAGE_MARGIN_X) as f32;
            for (i, (fg, items)) in rows.iter().skip(skip).take(fit as usize).enumerate() {
                // y = 行顶 + 刚体平移（垂直居中基线归图集槽位 off_y，
                // 装载方按 ai_text_baseline_off 折算——收集只管行顶）
                let y = (AI_PAGE_TOP + i as u32 * AI_PAGE_LINE_H) as f32 + panel_off as f32;
                let mut pen = AI_PAGE_MARGIN_X as f32 + 18.0;
                for (_, c, adv) in items {
                    if pen + adv >= clip_right {
                        break; // 右缘装不下就停（draw_items_left 同判据）
                    }
                    if paintable(*c) {
                        // 字体路由与 rasterize_for_atlas_px 同判据
                        // （prefer_cjk）——收集键与装载键必须同一槽
                        let font_id = match &self.cjk {
                            Some(cjk) if prefer_cjk(&self.font, &cjk.font, *c) => 1u8,
                            _ => 0u8,
                        };
                        out.push(crate::glyph_atlas::AiGlyph {
                            x: pen,
                            y,
                            c: *c,
                            font: font_id,
                            fg: *fg,
                        });
                    }
                    pen += adv;
                }
            }
        }
        ((rows.len() as u32, fit), out)
    }

    /// 全部展示行：(文字色, 该行的已量宽字符)——角色标签行 + 思考块
    /// （流式中的末条：≤3 行暗色尾随活窗；已收流：折叠成一行暗色
    /// 「已思考」——2026-09-04 用户拍板：思考往往不重要但必须存在）
    /// + 正文折行（渲染与布局测量共用这一份：眼手同尺的单源）
    fn build_ai_rows<'a>(
        &'a self,
        msgs: &'a [(bool, String, String)],
        buf_w: u32,
        live_tail: bool,
    ) -> Vec<AiRow<'a>> {
        let row_w = buf_w.saturating_sub(AI_PAGE_MARGIN_X * 2);
        // draw_items_left 起笔内缩 18，折行可用宽要扣掉
        let wrap_w = row_w.saturating_sub(18) as f32;
        // 折行辅助改方法（闭包推不出 'a 生命周期）
        let mut rows = Vec::new();
        let last = msgs.len().saturating_sub(1);
        for (i, (is_user, text, thinking)) in msgs.iter().enumerate() {
            let label_fg = if *is_user { MAG_BORDER } else { AI_PAGE_FG };
            let label = if *is_user { "你" } else { "AI" };
            rows.push((label_fg, self.measure_items(label, AI_PAGE_PX)));
            if !is_user && !thinking.is_empty() {
                if live_tail && i == last {
                    // 活窗：尾随窗 ≤3 行（thinking_window 纯函数钉计数与
                    // 尾随语义）——块高恒定，流式时窗口跟尾 = 自己滚动
                    let think_rows = self.wrap_ai_lines(thinking, wrap_w);
                    for items in &think_rows[crate::ui::ai_page::thinking_window(think_rows.len())]
                    {
                        rows.push((AI_THINK_FG, items.clone()));
                    }
                } else {
                    // 收流折叠：一行暗色占位（思考全文随消息存档，不丢）
                    rows.push((
                        AI_THINK_FG,
                        self.measure_items(AI_THINK_COLLAPSED, AI_PAGE_PX),
                    ));
                }
            }
            for items in self.wrap_ai_lines(text, wrap_w) {
                rows.push((DEFAULT_FG, items));
            }
        }
        rows
    }

    /// 折行辅助：一段文本 → 若干展示行（与正文同尺贪心断行）
    fn wrap_ai_lines<'a>(
        &'a self,
        text: &str,
        wrap_w: f32,
    ) -> Vec<Vec<(&'a fontdue::Font, char, f32)>> {
        let mut out = Vec::new();
        for line in text.split('\n') {
            let items = self.measure_items(line, AI_PAGE_PX);
            let widths: Vec<f32> = items.iter().map(|i| i.2).collect();
            let starts = wrap_starts(&widths, wrap_w);
            for (li, &st) in starts.iter().enumerate() {
                let en = starts.get(li + 1).copied().unwrap_or(items.len());
                out.push(items[st..en].to_vec());
            }
        }
        out
    }

    /// 雾状光球（D8 拟合定稿 2026-08-30，加法合成）：视图本体在 ui/orb.rs
    /// （2026-09-01 控件库立形物理搬移，零逻辑变化）——本方法只剩 trait 转发
    #[allow(clippy::too_many_arguments)]
    pub fn render_orb(
        &self,
        buf: &mut [u32],
        buf_w: u32,
        buf_h: u32,
        x: f64,
        y: f64,
        gain: f32,
        halo_gain: f32,
        alpha_out: bool,
    ) {
        if alpha_out {
            // chrome 层半透写出（GLES over 层——BAR-066：加法 sprite 在
            // 透明画布上没有真背景可加，改 (α, E) 走 GPU 标准混合）
            crate::ui::orb::render_alpha(buf, buf_w, buf_h, x, y, gain, halo_gain);
        } else {
            // 真背景饱和加（softbuffer 单层，画在已就位的页面之上）
            crate::ui::orb::render(buf, buf_w, buf_h, x, y, gain, halo_gain);
        }
    }

    /// 快捷键行标签：水平居中 + 垂直居中光栅文本。主字体缺字形走 CJK 备用
    /// （↑↓←→ 的命），双缺记 tofu 目击名单后跳过（不画方框吓唬人）。
    /// fg = 文字色（快捷键行 KEYBAR_LABEL / AI 页 AI_PAGE_FG）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_label(
        &self,
        frame: &mut Frame<'_>,
        text: &str,
        cx: u32,
        cw: u32,
        cy: u32,
        rh: u32,
        fg: u32,
    ) {
        let px = rh as f32 * 0.26; // 字号：行高的 1/4 左右（实拍「太大」后收敛）
        let Some(hm) = self.font.horizontal_line_metrics(px) else {
            return;
        };
        // 逐字挑字体（与 draw_glyph 同规则），顺便算总宽
        let pick = |c: char| -> Option<&fontdue::Font> {
            if self.font.lookup_glyph_index(c) != 0 {
                Some(&self.font)
            } else if let Some(k) = &self.cjk {
                if k.font.lookup_glyph_index(c) != 0 {
                    Some(&k.font)
                } else {
                    None
                }
            } else {
                None
            }
        };
        let mut glyphs = Vec::new();
        let mut width = 0.0f32;
        for c in text.chars() {
            let Some(f) = pick(c) else {
                let mut seen = self.tofu_seen.borrow_mut();
                if !seen.contains(&c) && seen.len() < 16 {
                    seen.push(c); // 标签缺字也上报（↑ 在不在设备字体里，问机器）
                }
                continue;
            };
            let m = f.metrics(c, px);
            glyphs.push((f, c, m.advance_width));
            width += m.advance_width;
        }
        if glyphs.is_empty() {
            return;
        }
        let mut pen_x = cx as f32 + (cw as f32 - width).max(0.0) / 2.0;
        // 垂直居中：行内盒（ascent-descent）放进键格正中
        let baseline = cy as f32 + (rh as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent;
        for (f, c, adv) in glyphs {
            let (m, bmp) = f.rasterize(c, px);
            let top = baseline - m.ymin as f32 - m.height as f32;
            for gy in 0..m.height as u32 {
                let y = top as i64 + i64::from(gy);
                if y < 0 || y >= i64::from(frame.h) {
                    continue;
                }
                for gx in 0..m.width as u32 {
                    let x = (pen_x + m.xmin as f32) as i64 + i64::from(gx);
                    if x < 0 || x >= i64::from(frame.w) {
                        continue;
                    }
                    let a = u32::from(bmp[(gy * m.width as u32 + gx) as usize]);
                    if a > 0 {
                        frame.blend_px(x as u32, y as u32, fg, a);
                    }
                }
            }
            pen_x += adv;
        }
    }

    /// 逐字挑字体（输入栏文本规则，与 draw_label 同）：主字体缺走 CJK
    /// 备用，双缺 = None（调用方记 tofu）
    fn pick_font(&self, c: char) -> Option<&fontdue::Font> {
        if self.font.lookup_glyph_index(c) != 0 {
            Some(&self.font)
        } else if let Some(k) = &self.cjk {
            if k.font.lookup_glyph_index(c) != 0 {
                Some(&k.font)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 文本 → (字体, 字, 步进宽) 序列（px 字号下量宽；缺字记 tofu 跳过）。
    /// 折行量宽与画字共用这一条序列——眼手同尺的物质基础
    pub(crate) fn measure_items(&self, text: &str, px: f32) -> Vec<(&fontdue::Font, char, f32)> {
        let mut items = Vec::new();
        for c in text.chars() {
            let Some(f) = self.pick_font(c) else {
                let mut seen = self.tofu_seen.borrow_mut();
                if !seen.contains(&c) && seen.len() < 16 {
                    seen.push(c);
                }
                continue;
            };
            items.push((f, c, f.metrics(c, px).advance_width));
        }
        items
    }

    /// 输入栏量宽（2026-09-04 Enter 换行多逻辑行排版）：与 measure_items
    /// 唯一差异——'\n' 保留为零宽条目，不被 pick_font 跳过。下游全家
    /// （starts/光标/选区/锚点柄/菜单）建立在「item 下标 == char 下标
    /// 1:1」假设上，'\n' 进序列才能一处不破。零宽光栅零面积，
    /// draw_items_left 对它天然安全（循环不执行，不进墨）。
    pub(crate) fn measure_bar_items(
        &self,
        text: &str,
        px: f32,
    ) -> Vec<(&fontdue::Font, char, f32)> {
        let mut items = Vec::new();
        for c in text.chars() {
            if c == '\n' {
                items.push((&self.font, '\n', 0.0));
                continue;
            }
            let Some(f) = self.pick_font(c) else {
                let mut seen = self.tofu_seen.borrow_mut();
                if !seen.contains(&c) && seen.len() < 16 {
                    seen.push(c);
                }
                continue;
            };
            items.push((f, c, f.metrics(c, px).advance_width));
        }
        items
    }

    /// 输入栏文本：左对齐（内缩 18px）+ 垂直居中，右缘按 cw 裁剪。
    /// px = 显式字号（textarea 多行后字号不随行高缩，调用方给 BAR_TEXT_PX）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_text_left(
        &self,
        frame: &mut Frame<'_>,
        text: &str,
        cx: u32,
        cw: u32,
        cy: u32,
        rh: u32,
        px: f32,
        fg: u32,
    ) {
        let items = self.measure_items(text, px);
        self.draw_items_left(frame, &items, cx, cw, cy, rh, px, fg, None);
    }

    /// 居中画一行文字（BAR-046 选择菜单按钮标签，2026-09-03）：水平居中
    /// 于 (cx,cw)，垂直居中于 (cy,ch)，右缘裁剪。与 draw_items_left 同
    /// 光栅化路径，只是起笔 = 格心 - 文本半宽、无 18 内缩。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_text_centered(
        &self,
        frame: &mut Frame<'_>,
        text: &str,
        cx: u32,
        cy: u32,
        cw: u32,
        ch: u32,
        px: f32,
        fg: u32,
    ) {
        let items = self.measure_items(text, px);
        if items.is_empty() {
            return;
        }
        let Some(hm) = self.font.horizontal_line_metrics(px) else {
            return;
        };
        let text_w: f32 = items.iter().map(|i| i.2).sum();
        let mut pen_x = cx as f32 + (cw as f32 - text_w).max(0.0) / 2.0;
        let clip_right = cx + cw;
        let baseline = cy as f32 + (ch as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent;
        for (f, c, adv) in items {
            if pen_x + adv >= clip_right as f32 {
                break; // 格内装不下就停（与 draw_items_left 同判据）
            }
            let (m, bmp) = f.rasterize(c, px);
            let top = baseline - m.ymin as f32 - m.height as f32;
            for gy in 0..m.height as u32 {
                let y = top as i64 + i64::from(gy);
                if y < 0 || y >= i64::from(frame.h) {
                    continue;
                }
                for gx in 0..m.width as u32 {
                    let x = (pen_x + m.xmin as f32) as i64 + i64::from(gx);
                    if x < 0 || x >= i64::from(clip_right) {
                        continue;
                    }
                    let a = u32::from(bmp[(gy * m.width as u32 + gx) as usize]);
                    if a > 0 {
                        frame.blend_px(x as u32, y as u32, fg, a);
                    }
                }
            }
            pen_x += adv;
        }
    }

    /// 画一串已量宽的字符（折行后逐行画走这里）：左对齐内缩 18 +
    /// 垂直居中 + 右缘裁剪，规则与 draw_text_left 一致
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_items_left(
        &self,
        frame: &mut Frame<'_>,
        items: &[(&fontdue::Font, char, f32)],
        cx: u32,
        cw: u32,
        cy: u32,
        rh: u32,
        px: f32,
        fg: u32,
        clip_y: Option<(i32, i32)>,
    ) {
        let Some(hm) = self.font.horizontal_line_metrics(px) else {
            return;
        };
        let mut pen_x = cx as f32 + 18.0;
        let clip_right = cx + cw;
        let baseline = cy as f32 + (rh as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent;
        for (f, c, adv) in items {
            if pen_x + adv >= clip_right as f32 {
                break; // 右缘装不下就停（v1 无横滚，截断即判卷）
            }
            let (m, bmp) = f.rasterize(*c, px);
            let top = baseline - m.ymin as f32 - m.height as f32;
            for gy in 0..m.height as u32 {
                let y = top as i64 + i64::from(gy);
                if y < 0 || y >= i64::from(frame.h) {
                    continue;
                }
                if let Some((cy0, cy1)) = clip_y
                    && (y < i64::from(cy0) || y >= i64::from(cy1))
                {
                    continue;
                }
                for gx in 0..m.width as u32 {
                    let x = (pen_x + m.xmin as f32) as i64 + i64::from(gx);
                    if x < 0 || x >= i64::from(clip_right) {
                        continue;
                    }
                    let a = u32::from(bmp[(gy * m.width as u32 + gx) as usize]);
                    if a > 0 {
                        frame.blend_px(x as u32, y as u32, fg, a);
                    }
                }
            }
            pen_x += adv;
        }
    }

    /// 光栅化单字形并 alpha 混合进帧缓冲。基线对齐（BAR-001）：fontdue
    /// y 轴向上，metrics.ymin 是位图底边相对基线的偏移（下伸字母为负），
    /// 位图顶边（屏坐标）= 格顶 + 基线偏移 - (ymin + 位图高)。
    /// 字体选择：主字体缺该字且备用有 → CJK 三件套（prefer_cjk，两格宽适配）；
    /// 双字体都缺 → 记 tofu 目击名单（主字体画 .notdef 方框）。
    /// clip_w = 右缘裁剪宽（格宽的 1 或 2 倍）：模糊宽度字符（宽度判 1 格
    /// 但落在全角比例的 CJK 字体上，如 ⇄）墨不许溢进下一格的内容区
    fn draw_glyph(&self, frame: &mut Frame<'_>, c: char, px: u32, py: u32, fg: u32, clip_w: u32) {
        if self.font.lookup_glyph_index(c) == 0 {
            let covered = self
                .cjk
                .as_ref()
                .is_some_and(|k| k.font.lookup_glyph_index(c) != 0);
            let mut seen = self.tofu_seen.borrow_mut();
            if !covered && !seen.contains(&c) && seen.len() < 16 {
                seen.push(c);
            }
        }
        let (font, font_px, baseline) = match &self.cjk {
            Some(cjk) if prefer_cjk(&self.font, &cjk.font, c) => {
                (&cjk.font, cjk.px, cjk.baseline_off)
            }
            _ => (&self.font, self.font_px, self.baseline_off),
        };
        let (metrics, bitmap) = font.rasterize(c, font_px);
        if metrics.width == 0 || metrics.height == 0 {
            return; // 缺字形/空白字形：fontdue 给空位图，不 panic
        }
        let top = py as i64 + baseline as i64 - i64::from(metrics.ymin) - metrics.height as i64;
        let clip_right = px as i64 + i64::from(clip_w);
        for gy in 0..metrics.height as u32 {
            let y = top + i64::from(gy);
            if y < 0 {
                continue; // 上探出屏（基线偏移 + 高字形）：裁
            }
            if y >= i64::from(frame.h) {
                break;
            }
            for gx in 0..metrics.width as u32 {
                // xmin 可为负（斜体左探）：用有符号算再裁
                let x = px as i64 + i64::from(metrics.xmin) + i64::from(gx);
                if x < 0 || x >= i64::from(frame.w) || x >= clip_right {
                    continue;
                }
                let a = u32::from(bitmap[(gy * metrics.width as u32 + gx) as usize]);
                if a == 0 {
                    continue;
                }
                frame.blend_px(x as u32, y as u32, fg, a);
            }
        }
    }
}

/// chrome 层条件 alpha（「纯黑=空白」契约，BAR-066 扩版）：RGB 非零且
/// 高字节为 0 → 强转不透明（keybar/输入栏/AI 页底色的可见内容全为
/// 非纯黑）；高字节非 0（光球半透像素自带 alpha）→ 原样直通；纯零 →
/// 透明（网格层透出）。黑屏案 2026-09-05 教训：一刀切 |= alpha 会把
/// chrome 变成不透明黑膜；光球半透案：一刀切会把 (alpha,E) 压成不
/// 透明暗块。纯逻辑（A 档），android_app GLES 双层扫描调用方
pub fn mark_chrome_alpha(px: &mut [u32]) {
    for p in px.iter_mut() {
        let rgb = *p & 0x00FF_FFFF;
        if rgb != 0 && *p & 0xFF00_0000 == 0 {
            *p = 0xFF00_0000 | rgb;
        }
    }
}

/// 该字符是否值得上屏（BAR-015）：空格与控制符（C0/C1/DEL）无字形——
/// alacritty put_tab 把 '\t' 本体写进格（为选中/复制能还原 tab），
/// 设备主字体（DroidSansMono）没有 tab 字形 → 不拦就画方框（2026-08-14
/// 实拍：ls 输出文件夹名后方框，tofu 目击名单实锤 U+0009）。
/// 契约钉在本纯函数（A 档考题 spec_渲染_tab控制符不落墨不进目击名单）：
/// host 的 DejaVuSansMono 有 tab 空白字形，像素层面咬不住，必须从这里过滤
pub fn paintable(c: char) -> bool {
    c != ' ' && !c.is_control()
}

// 输入栏配色已迁 theme.rs（2026-09-01 token 化立层）——控件只读
// self.theme.bar.*，不再认字面颜色；默认配方考题 spec_theme_默认kfmv4配方
// 在 tests/theme_spec.rs。keybar 配色与 SELECT_BG 终端线暂留此处，
// token 化跟随各自线的下一次重构。

/// 长按选择高亮底色（kfmv4 正蓝 #3B82F6，2026-08-21 品牌色板统一——
/// 此前借用的 KEYBAR_MOD_ON 0x3E6FB4 是快捷键行私色，不成套）
pub const SELECT_BG: u32 = 0x003B_82F6;

/// 放大镜（边界拖动中浮窗）：源区 = 触点格 ±5 格宽 × ±3 行高，最近邻 2 倍；
/// 边框 kfmv4 青 #06B6D4（与选中条正蓝同品牌色板），衬底黑
pub const MAG_HALF_COLS: u32 = 5;
pub const MAG_HALF_ROWS: u32 = 3;
pub const MAG_ZOOM: u32 = 2;
pub const MAG_BORDER: u32 = 0x0006_B6D4;
/// 浮窗底缘与触点的间距（不挡手）
pub const MAG_GAP_PX: u32 = 60;

/// AI 页占位空壳配色（ai-presence 期 0 组件一）：深紫暗底 + 浅紫标记文字
/// （kfmv4 紫色板血统：核 #7C3AED 的暗化/亮化两端）
pub const AI_PAGE_BG: u32 = 0x0014_0A24;
pub const AI_PAGE_FG: u32 = 0x00C4_B5FD;

/// AI 页边框（2026-09-04 用户拍板「装修」：仿 kfmv4 对话面板 orb-panel
/// ——orb.ts createPanel 的 CSS 配方直译）：135° 渐变描边（青 .8 → 紫 .7，
/// kfmv4 中段靛 = 两端 50% 混合的天然近似）+ 左缘 3 倍粗 + 圆角 12 CSS
/// px + 紫外发光（0 0 24px α0.25 → spread 14 α64）。物理像素 = CSS × 3。
pub const AI_PAGE_FRAME_C1: u32 = 0x0000_D4FF; // 青 rgba(0,212,255,~.8)
pub const AI_PAGE_FRAME_C2: u32 = 0x007C_3AED; // 紫 rgba(124,58,237,~.7)
/// 边框外缘距屏幕边的留白（左/右/上；下缘距输入栏带上沿同此）
pub const AI_PAGE_FRAME_MARGIN: u32 = 16;
/// 描边厚（上/右/下；左缘 3 倍 = 9，kfmv4 border-left-width:3px）
pub const AI_PAGE_FRAME_W: u32 = 3;
/// 圆角半径（kfmv4 border-radius:12px × 3）
pub const AI_PAGE_FRAME_R: u32 = 36;

// 光球 sprite 机制已迁 ui/orb.rs（2026-09-01 控件库立形）——配方常量/
// build_orb_sprite/blit_orb_sprite/双缓存/绘制本体全部随迁，零逻辑变化；
// 考题同源路径 tests/ai_presence_spec.rs（kfm_na::ui::orb::）

/// 选区边界端点：Start = 归一化后的起端（字典序小），End = 止端
/// （2026-08-21 拖柄废除后改名 SelEnd——柄没了，端点还在）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelEnd {
    Start,
    End,
}

/// 帧缓冲视图：把 buf + 尺寸打包，免得每个画图函数都拖一溜参数（clippy 红线）
pub(crate) struct Frame<'a> {
    pub(crate) buf: &'a mut [u32],
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// 渐变填色参数（fill_round_rect_grad 用，同 Frame 的打包纪律）
#[derive(Clone, Copy)]
pub(crate) struct GradSpec {
    pub(crate) c1: u32,
    pub(crate) c2: u32,
    /// false = 沿横向，true = 沿主对角线
    pub(crate) diag: bool,
}

/// 外发光/投影参数（glow_round_rect 用）
#[derive(Clone, Copy)]
pub(crate) struct GlowSpec {
    pub(crate) color: u32,
    pub(crate) alpha: u32,
    pub(crate) spread: u32,
    /// 投影纵向偏移（0 = 对称光晕，>0 = 向下投影）
    pub(crate) y_off: u32,
}

/// 顶内侧高光/内阴影参数（inner_top_veil 用）
#[derive(Clone, Copy)]
pub(crate) struct VeilSpec {
    pub(crate) color: u32,
    pub(crate) alpha: u32,
    pub(crate) rows: u32,
}

impl Frame<'_> {
    /// 画纯色矩形（裁剪到帧缓冲内）
    pub(crate) fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        for row in y..(y + h).min(self.h) {
            for col in x..(x + w).min(self.w) {
                self.buf[(row * self.w + col) as usize] = color;
            }
        }
    }

    /// 画圆角矩形（SDF 抗锯齿：边界 1px 覆盖率过渡），快捷键行药丸键用
    pub(crate) fn fill_round_rect(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, color: u32) {
        let r = r.min(w / 2).min(h / 2);
        for py in 0..h {
            for px in 0..w {
                let cov = rr_cover(px, py, w, h, r);
                if cov == 0 {
                    continue;
                }
                let (ax, ay) = (x + px, y + py);
                if ax < self.w && ay < self.h {
                    if cov == 255 {
                        self.buf[(ay * self.w + ax) as usize] = color;
                    } else {
                        self.blend_px(ax, ay, color, cov);
                    }
                }
            }
        }
    }

    /// 外发光/投影（kfmv4 box-shadow 质感）：沿 SDF 向外 spread px 二次
    /// 衰减，只画矩形外部（内部归主体）。y_off 模拟投影偏移（正 = 向下）
    pub(crate) fn glow_round_rect(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, g: GlowSpec) {
        let spread = i64::from(g.spread);
        let (x, y) = (i64::from(x), i64::from(y) + i64::from(g.y_off));
        let x0 = (x - spread).max(0);
        let y0 = (y - spread).max(0);
        let x1 = (x + i64::from(w) + spread).min(i64::from(self.w));
        let y1 = (y + i64::from(h) + spread).min(i64::from(self.h));
        for ay in y0..y1 {
            for ax in x0..x1 {
                let d = rr_sdf((ax - x) as f32 + 0.5, (ay - y) as f32 + 0.5, w, h, r);
                if d <= 0.0 {
                    continue; // 内部归主体画
                }
                let t = (1.0 - d / g.spread as f32).max(0.0);
                let a = (g.alpha as f32 * t * t) as u32;
                if a > 0 {
                    self.blend_px(ax as u32, ay as u32, g.color, a);
                }
            }
        }
    }

    /// 渐变圆角矩形（输入栏描边/发送钮用）：SDF 抗锯齿，颜色从 g.c1
    /// 渐变到 g.c2——g.diag=false 沿横向，true 沿主对角线
    pub(crate) fn fill_round_rect_grad(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        r: u32,
        g: GradSpec,
    ) {
        let r = r.min(w / 2).min(h / 2);
        // t 的分母：横向 = w-1；对角 = 归一到 (w-1)+(h-1)
        let denom = if g.diag { (w - 1) + (h - 1) } else { w - 1 }.max(1);
        for py in 0..h {
            for px in 0..w {
                let cov = rr_cover(px, py, w, h, r);
                if cov == 0 {
                    continue;
                }
                let (ax, ay) = (x + px, y + py);
                if ax >= self.w || ay >= self.h {
                    continue;
                }
                let t = if g.diag { px + py } else { px };
                let color = lerp_rgb(g.c1, g.c2, (t * 255 / denom).min(255));
                if cov == 255 {
                    self.buf[(ay * self.w + ax) as usize] = color;
                } else {
                    self.blend_px(ax, ay, color, cov);
                }
            }
        }
    }

    /// 顶内侧高光/内阴影（kfmv4 inset 质感）：圆角矩形内顶起 rows 高一条，
    /// 按形状覆盖率混合（color/alpha 调用方定——白 0.15 = 玻璃高光，
    /// 黑 0.2 = 内阴影）
    pub(crate) fn inner_top_veil(&mut self, x: u32, y: u32, w: u32, h: u32, r: u32, v: VeilSpec) {
        let r = r.min(w / 2).min(h / 2);
        for py in 0..v.rows.min(h) {
            for px in 0..w {
                let cov = rr_cover(px, py, w, h, r);
                if cov == 0 {
                    continue;
                }
                let (ax, ay) = (x + px, y + py);
                if ax < self.w && ay < self.h {
                    let a = v.alpha * cov / 255;
                    if a > 0 {
                        self.blend_px(ax, ay, v.color, a);
                    }
                }
            }
        }
    }

    /// 右指实心三角（发送钮 ▶ 图标）：以 (cx, cy) 为中心、高 size、
    /// 宽 = size*3/4。逐行扫：该行右端 = 顶点回缩 |dy| 按比例
    pub(crate) fn fill_triangle_right(&mut self, cx: u32, cy: u32, size: u32, color: u32) {
        let half_h = (size / 2) as i64;
        let half_w = (size * 3 / 8) as i64;
        let (cx, cy) = (i64::from(cx), i64::from(cy));
        for dy in -half_h..=half_h {
            // 行右端：中心行抵顶点，向两端按 dy 比例回缩到左竖边
            let xr = cx + half_w - dy.abs() * (2 * half_w) / half_h.max(1);
            for x in (cx - half_w)..=xr {
                let y = cy + dy;
                if x >= 0 && y >= 0 && x < i64::from(self.w) && y < i64::from(self.h) {
                    self.buf[(y * i64::from(self.w) + x) as usize] = color;
                }
            }
        }
    }

    /// 单像素按覆盖率 a 混合（调用方保证 x/y 已在界内）
    pub(crate) fn blend_px(&mut self, x: u32, y: u32, fg: u32, a: u32) {
        let dst = &mut self.buf[(y * self.w + x) as usize];
        *dst = blend(fg, *dst, a);
    }

    /// 图钉柄一体光栅（BAR-052）：尖三角 + 肩部钝角圆角 + 圆角承载块，
    /// 逐行解析跨度填充，左右按承载块边缘等距镜像（BAR-051 同轴纪律：
    /// 尖轴 ≡ 块轴，承载块跨 [cx-half_w, cx+half_w)）。
    /// 肩部 fillet = 斜边与立边的精确切圆：斜率 m=(half_w-1)/(tri_h-1)，
    /// 圆心 (xl+r, y_v + r(√(1+m²)-1)/m) 同时与立边(x=xl)和斜边相切，
    /// 过渡摊 4~6 行、逐行 |Δx| ≤ 1——无平顶拼接的接缝台阶（用户实拍
    /// 「三角和正方形交接生硬」对症；成熟输入法柄同形）。
    ///   cx      柄轴（承载块 [cx-half_w, cx+half_w)）
    ///   tip_y   尖顶行
    ///   half_w  承载块半宽（立边 x = cx-half_w）
    ///   tri_h   三角行数（尖顶行半宽 1 → 顶点行抵立边）
    ///   bulb_h  承载块高（顶点行起算，含底角弧）
    ///   r_sh    肩部 fillet 半径（钝角圆角）
    ///   r_bot   承载块底角半径
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill_pin_handle(
        &mut self,
        cx: u32,
        tip_y: u32,
        half_w: u32,
        tri_h: u32,
        bulb_h: u32,
        r_sh: u32,
        r_bot: u32,
        color: u32,
    ) {
        let cx = cx as f32;
        let xl = cx - half_w as f32; // 立边（承载块左缘）
        let xr = cx + half_w as f32; // 承载块右缘（半开）
        let tip_y = tip_y as f32;
        let y_v = tip_y + tri_h as f32 - 1.0; // 顶点行：斜边抵立边处
        let y_bot = y_v + bulb_h as f32 - 1.0; // 承载块底边行
        let m = (half_w as f32 - 1.0) / (tri_h as f32 - 1.0).max(1.0); // 斜边横纵比
        let r_sh = r_sh as f32;
        let r_bot = r_bot as f32;
        // 肩部切圆：T1=斜边切点（垂足投影行），T2=立边切点（圆心正左）
        let g = (1.0 + m * m).sqrt();
        let scx = xl + r_sh;
        let scy = y_v + r_sh * (g - 1.0) / m;
        let t1y = scy - r_sh * m / g;
        let t2y = scy;
        let bcy = y_bot - r_bot; // 底角圆心行
        let (y0, y1) = (tip_y as i64, y_bot.ceil() as i64);
        for y in y0..=y1 {
            let yf = y as f32;
            let x_left = if yf < t1y {
                cx - 1.0 - (yf - tip_y) * m // 斜边
            } else if yf <= t2y {
                let dy = yf - scy;
                scx - (r_sh * r_sh - dy * dy).max(0.0).sqrt() // 肩部弧
            } else if yf <= bcy {
                xl // 立边
            } else {
                let dy = yf - bcy;
                xl + r_bot - (r_bot * r_bot - dy * dy).max(0.0).sqrt() // 底角弧
            };
            // 右缘镜像：与承载块右缘等距（同轴），填 [(xl+off), (xr-off))
            let off = (x_left - xl).max(0.0);
            let (x0, x1) = ((xl + off).round() as i64, (xr - off).round() as i64);
            for x in x0..x1 {
                if x >= 0 && y >= 0 && x < i64::from(self.w) && y < i64::from(self.h) {
                    self.buf[(y * i64::from(self.w) + x) as usize] = color;
                }
            }
        }
    }
}

/// 按覆盖率 a（0-255）把 fg 混合到 dst 上（逐通道线性插值）
fn blend(fg: u32, dst: u32, a: u32) -> u32 {
    let inv = 255 - a;
    let ch = |f: u32, d: u32| (f * a + d * inv) / 255;
    let r = ch((fg >> 16) & 0xFF, (dst >> 16) & 0xFF);
    let g = ch((fg >> 8) & 0xFF, (dst >> 8) & 0xFF);
    let b = ch(fg & 0xFF, dst & 0xFF);
    (r << 16) | (g << 8) | b
}

/// AI 文字装载的 off_y 折算（唯一公式）：floor 语义与 CPU 画字的
/// `top as i64` 逐像素对齐——行顶是整数，trunc(行 + x) = 行 +
/// floor(x)；负分数偏移（高字形上探）按 `as i16` 向零截断会错
/// 1px（2026-09-05 对拍考题 spec_gpu_ai页文字实例 逮住）。
/// android_app 装载与考题软件合成共用这一份
pub fn ai_glyph_off_y(baseline_off: f32, ymin: f32, height: f32) -> i16 {
    (baseline_off - ymin - height).floor() as i16
}

/// 两色逐通道线性插值（t = 0..255，渐变图元用；A 档考题
/// spec_lerp_rgb_* 在 tests/termview_spec.rs）
pub fn lerp_rgb(c1: u32, c2: u32, t: u32) -> u32 {
    let f = |sh: u32| {
        let a = ((c1 >> sh) & 0xFF) as i64;
        let b = ((c2 >> sh) & 0xFF) as i64;
        (a + (b - a) * t as i64 / 255) as u32
    };
    (f(16) << 16) | (f(8) << 8) | f(0)
}

/// 圆角矩形 SDF（像素中心相对形状的有符号距离，负=内正=外；
/// iq 圆角盒公式）——AA 覆盖率与外发光衰减的同一把尺。
/// 快路：双轴都在直边区就不必 hypot（药丸键/描边大面填充的命根，
/// 全量 hypot 一帧多 ~百毫秒级，2026-08-31 实测量级估算）
fn rr_sdf(px: f32, py: f32, w: u32, h: u32, r: u32) -> f32 {
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let r = r.min(w / 2).min(h / 2) as f32;
    let qx = (px - hw).abs() - (hw - r);
    let qy = (py - hh).abs() - (hh - r);
    if qx <= 0.0 && qy <= 0.0 {
        qx.max(qy) - r
    } else {
        qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - r
    }
}

/// 圆角矩形覆盖率（0..=255，边界 1px 抗锯齿过渡；A 档考题
/// spec_rr_cover_* 在 tests/termview_spec.rs）
pub fn rr_cover(px: u32, py: u32, w: u32, h: u32, r: u32) -> u32 {
    let d = rr_sdf(px as f32 + 0.5, py as f32 + 0.5, w, h, r);
    ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u32
}

/// 换行布局（2026-08-31 移动端 textarea 全量复刻拍板）：给逐字宽度和行
/// 可用宽，返回每行起始字下标——放得下 = [0]（一行）；贪心断行（满即断，
/// 刚好放下不断）；超宽单字（比行还宽）独占一行不吞字（交右缘裁剪）；
/// 空表 = [0] 不炸。A 档纯逻辑，考题 spec_wrap_starts_* 在
/// tests/termview_spec.rs
pub fn wrap_starts(widths: &[f32], max_w: f32) -> Vec<usize> {
    let mut starts = vec![0usize];
    let mut acc = 0.0f32;
    for (i, w) in widths.iter().enumerate() {
        if i > *starts.last().unwrap() && acc + w > max_w {
            starts.push(i);
            acc = 0.0;
        }
        acc += w;
    }
    starts
}

/// 供 android_app：从候选路径建视图（主字体 + CJK 备用 + 默认 80x24 占位网格），
/// 返回 (视图, 主字体来源, CJK 字体来源)。主字体全灭返回 None。
/// 注：生产已不走这条路（BAR-021 起用 build_vendored 零探测），本函数保留
/// 给考题注入夹具与「探测链」行为的回归钉
pub fn build_from_candidates(candidates: &[&str]) -> Option<(TermView, String, Option<String>)> {
    let (path, font) = load_font(candidates)?;
    let (cjk_path, cjk_font) = match load_cjk_font(CJK_FONT_CANDIDATES) {
        Some((p, f)) => (Some(p), Some(f)),
        None => (None, None),
    };
    Some((
        TermView::new(font, cjk_font, 80, 24, CELL_W, CELL_H),
        path,
        cjk_path,
    ))
}

/// 生产默认构建（BAR-021）：零探测——主/CJK 字体都是编译期内嵌字节，
/// 启动全程不碰 /system/fonts。返回 (视图, "<内嵌主>", Some("<内嵌CJK>"))；
/// 内嵌字节解析失败（不可能，有考题钉）才返回 None。本函数不 panic。
pub fn build_vendored() -> Option<(TermView, String, Option<String>)> {
    let font =
        fontdue::Font::from_bytes(VENDORED_MAIN_FONT, fontdue::FontSettings::default()).ok()?;
    let cjk = fontdue::Font::from_bytes(VENDORED_CJK_FONT, fontdue::FontSettings::default()).ok();
    Some((
        TermView::new(font, cjk, BOOT_COLS, BOOT_ROWS, CELL_W, CELL_H),
        "<内嵌主>".to_string(),
        Some("<内嵌CJK>".to_string()),
    ))
}

// ---- trait 层（终端模拟器设计页 §2；插件化边界，方法体一行不动） ----

/// 终端模拟器对象面（服务键 `dyn TermEmuFactory` 产出的实例侧）。
/// `Send` 不含 `Sync`：独占可变持有——类型约束编码状态存活分层（评审裁决 1）。
///
/// 演化纪律（评审裁决 2 边界注记）：方法面 = android_app 现调集合，
/// 新增方法须有调用方先例；自由函数（grid_dims/paintable/颜色表）无状态，
/// 永不进 trait。
pub trait TermEmu: Send {
    fn feed(&mut self, bytes: &[u8]);
    fn resize_cells(&mut self, cols: u32, rows: u32);
    fn cell_size(&self) -> (u32, u32);
    /// 运行期改格尺寸（捏合缩放，android_app 双指手势调用方）
    fn set_cell_size(&mut self, cell_w: u32, cell_h: u32);
    fn render_into(&mut self, buf: &mut [u32], w: u32, h: u32);
    /// GPU 网格收集（期 1 第 2 层，android_app GLES 分支调用方）：格子
    /// 的纯数据镜像（颜色决策/几何裁剪与 render_into 同源）——GLES 后端
    /// grid_to_instances 的进料；CPU 路径不调
    fn gpu_cells(&mut self, w: u32, h: u32) -> Vec<crate::glyph_atlas::GpuCell>;
    /// 图集供墨（同上调用方）：字体路由（prefer_cjk）+ 光栅化 + 放置
    /// 偏移（xmin / baseline-ymin-h）；None = 空字形跳装载
    fn rasterize_for_atlas(&self, c: char) -> Option<(u8, fontdue::Metrics, Vec<u8>, i16, i16)>;
    /// 泛化供墨核心（字号参数化，android_app GLES AI 文字装载调用方）：
    /// 路由/tofu 记账同上，字号调用方定——终端与 AI 页（AI_PAGE_PX）
    /// 共用；off 归调用方按各自基线约定折算
    fn rasterize_for_atlas_px(
        &self,
        c: char,
        px: f32,
        px_cjk: f32,
    ) -> Option<(u8, fontdue::Metrics, Vec<u8>)>;
    /// AI 页文字 → GPU 字形收集（android_app GLES paint_under 调用方）：
    /// 布局与 render_ai_page 同源，画字语义对齐 draw_items_left；返回
    /// （布局读数, 字形列表——panel_off 已进行 y）
    #[allow(clippy::too_many_arguments)]
    fn ai_page_glyphs(
        &self,
        w: u32,
        h: u32,
        msgs: &[(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
        panel_off: i32,
    ) -> ((u32, u32), Vec<crate::glyph_atlas::AiGlyph>);
    /// AI 页行基线（相对行顶；AI 文字装载 off_y 折算的唯一尺子）
    fn ai_text_baseline_off(&self) -> f32;
    fn render_keybar(&self, buf: &mut [u32], w: u32, h: u32, ime_bottom: u32, mods: u8);
    /// AI 外显 chrome（ai-presence，android_app rasterize 调用方）：
    /// AI 页真对话渲染（page=AiFullscreen 时代替终端网格）/ 雾状光球 sprite。
    /// scroll_rows = 距底行数（期 0④ 视口）；bottom_inset = 键盘+输入栏
    /// 让位（追底追到栏带上沿）；live_tail = 末条思考相位中（思考活窗，
    /// 正文一出即折叠一行）；返回（总行数, 一屏行数）供调用方写回视口
    /// 状态机（眼手同尺）
    #[allow(clippy::too_many_arguments)]
    fn render_ai_page(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        msgs: &[(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
    ) -> (u32, u32);
    /// 全局输入栏 chrome（期 0 组件三，android_app rasterize 调用方）：
    /// 压底紧贴键盘（栏带 = 屏底 - inset - 栏高），任何会话页都画；
    /// sending = 发送钮图标态（▶ ↔ ⏸，跟 AI 运行态硬切）；
    /// caret_on = 光标闪烁相位（CARET_BLINK_MS 节拍，调用方算好传入）
    #[allow(clippy::too_many_arguments)]
    fn render_inputbar(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ime_bottom: u32,
        snap: &crate::input_bar::BarSnap,
        sending: bool,
        caret_on: bool,
    );
    /// 量输入栏文本折行数（android_app poll_input_bar 调用方：文本/宽度
    /// 变了先量行 set_lines 写回状态核，再 snap 再渲染——眼手同尺单源）
    fn bar_text_lines(&self, text: &str, buf_w: u32) -> u32;
    /// 点按定位换算（android_app 触摸 Field 调用方：文本区本地坐标 →
    /// 光标 char 下标，与渲染同几何）
    fn bar_cursor_at(
        &self,
        snap: &crate::input_bar::BarSnap,
        buf_w: u32,
        x_local: f64,
        y_local: f64,
    ) -> usize;
    /// 选择态屏幕几何（BAR-046）：锚点柄视觉中心 + 菜单气泡边界，触摸命中用
    fn bar_selection_geometry(
        &self,
        snap: &crate::input_bar::BarSnap,
        buf_w: u32,
        buf_h: u32,
        ime_bottom: u32,
    ) -> Option<crate::input_bar::BarSelectionGeometry>;
    #[allow(clippy::too_many_arguments)]
    fn render_orb(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        x: f64,
        y: f64,
        gain: f32,
        halo_gain: f32,
        // alpha_out：true = chrome 层半透写出（GLES over 层，BAR-066）；
        // false = 真背景饱和加（softbuffer 单层 / screendump）
        alpha_out: bool,
    );
    fn take_tofu_chars(&self) -> Vec<char>;
    fn scroll_lines(&mut self, lines: i32);
    fn scroll_to_bottom(&mut self);
    /// 当前视野纯文本导出（调试闸门 text-req 通道；跟随滚动位置，对齐「所见」）
    fn dump_text(&self) -> String;
    fn mouse_report_active(&self) -> bool;
    fn app_cursor_mode(&self) -> bool;
    fn font_probe(&self, c: char) -> (usize, usize, usize);
    /// 长按选择面（android_app 触摸状态机调用方）
    fn selection_active(&self) -> bool;
    fn select_word_at(&mut self, x: f64, y: f64);
    fn extend_selection(&mut self, x: f64, y: f64);
    fn clear_selection(&mut self);
    fn selected_text(&self) -> Option<String>;
    /// 选区边界/放大镜面（android_app 边界拖动手势调用方）
    fn hit_boundary(&self, x: f64, y: f64) -> Option<SelEnd>;
    fn move_selection_end(&mut self, which: SelEnd, x: f64, y: f64);
    fn render_magnifier(&self, buf: &mut [u32], w: u32, h: u32, x: f64, y: f64);
}

impl TermEmu for TermView {
    fn feed(&mut self, bytes: &[u8]) {
        TermView::feed(self, bytes)
    }
    fn resize_cells(&mut self, cols: u32, rows: u32) {
        TermView::resize_cells(self, cols, rows)
    }
    fn cell_size(&self) -> (u32, u32) {
        TermView::cell_size(self)
    }
    fn set_cell_size(&mut self, cell_w: u32, cell_h: u32) {
        TermView::set_cell_size(self, cell_w, cell_h)
    }
    fn render_into(&mut self, buf: &mut [u32], w: u32, h: u32) {
        TermView::render_into(self, buf, w, h)
    }
    fn gpu_cells(&mut self, w: u32, h: u32) -> Vec<crate::glyph_atlas::GpuCell> {
        TermView::collect_gpu_cells(self, w, h)
    }
    fn rasterize_for_atlas(&self, c: char) -> Option<(u8, fontdue::Metrics, Vec<u8>, i16, i16)> {
        TermView::rasterize_for_atlas(self, c)
    }
    fn rasterize_for_atlas_px(
        &self,
        c: char,
        px: f32,
        px_cjk: f32,
    ) -> Option<(u8, fontdue::Metrics, Vec<u8>)> {
        TermView::rasterize_for_atlas_px(self, c, px, px_cjk)
    }
    #[allow(clippy::too_many_arguments)]
    fn ai_page_glyphs(
        &self,
        w: u32,
        h: u32,
        msgs: &[(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
        panel_off: i32,
    ) -> ((u32, u32), Vec<crate::glyph_atlas::AiGlyph>) {
        TermView::ai_page_glyphs(
            self,
            w,
            h,
            msgs,
            scroll_rows,
            bottom_inset,
            live_tail,
            panel_off,
        )
    }
    fn ai_text_baseline_off(&self) -> f32 {
        TermView::ai_text_baseline_off(self)
    }
    fn render_keybar(&self, buf: &mut [u32], w: u32, h: u32, ime_bottom: u32, mods: u8) {
        TermView::render_keybar(self, buf, w, h, ime_bottom, mods)
    }
    #[allow(clippy::too_many_arguments)]
    fn render_ai_page(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        msgs: &[(bool, String, String)],
        scroll_rows: u32,
        bottom_inset: u32,
        live_tail: bool,
    ) -> (u32, u32) {
        TermView::render_ai_page(self, buf, w, h, msgs, scroll_rows, bottom_inset, live_tail)
    }
    fn render_inputbar(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ime_bottom: u32,
        snap: &crate::input_bar::BarSnap,
        sending: bool,
        caret_on: bool,
    ) {
        TermView::render_inputbar(self, buf, w, h, ime_bottom, snap, sending, caret_on)
    }
    fn bar_text_lines(&self, text: &str, buf_w: u32) -> u32 {
        TermView::bar_text_lines(self, text, buf_w)
    }
    fn bar_cursor_at(
        &self,
        snap: &crate::input_bar::BarSnap,
        buf_w: u32,
        x_local: f64,
        y_local: f64,
    ) -> usize {
        TermView::bar_cursor_at(self, snap, buf_w, x_local, y_local)
    }
    fn bar_selection_geometry(
        &self,
        snap: &crate::input_bar::BarSnap,
        buf_w: u32,
        buf_h: u32,
        ime_bottom: u32,
    ) -> Option<crate::input_bar::BarSelectionGeometry> {
        TermView::bar_selection_geometry(self, snap, buf_w, buf_h, ime_bottom)
    }
    #[allow(clippy::too_many_arguments)]
    fn render_orb(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        x: f64,
        y: f64,
        gain: f32,
        halo_gain: f32,
        alpha_out: bool,
    ) {
        TermView::render_orb(self, buf, w, h, x, y, gain, halo_gain, alpha_out)
    }
    fn take_tofu_chars(&self) -> Vec<char> {
        TermView::take_tofu_chars(self)
    }
    fn scroll_lines(&mut self, lines: i32) {
        TermView::scroll_lines(self, lines)
    }
    fn scroll_to_bottom(&mut self) {
        TermView::scroll_to_bottom(self)
    }
    fn dump_text(&self) -> String {
        TermView::dump_text(self)
    }
    fn mouse_report_active(&self) -> bool {
        TermView::mouse_report_active(self)
    }
    fn app_cursor_mode(&self) -> bool {
        TermView::app_cursor_mode(self)
    }
    fn font_probe(&self, c: char) -> (usize, usize, usize) {
        TermView::font_probe(self, c)
    }
    fn selection_active(&self) -> bool {
        TermView::selection_active(self)
    }
    fn select_word_at(&mut self, x: f64, y: f64) {
        TermView::select_word_at(self, x, y)
    }
    fn extend_selection(&mut self, x: f64, y: f64) {
        TermView::extend_selection(self, x, y)
    }
    fn clear_selection(&mut self) {
        TermView::clear_selection(self)
    }
    fn selected_text(&self) -> Option<String> {
        TermView::selected_text(self)
    }
    fn hit_boundary(&self, x: f64, y: f64) -> Option<SelEnd> {
        TermView::hit_boundary(self, x, y)
    }
    fn move_selection_end(&mut self, which: SelEnd, x: f64, y: f64) {
        TermView::move_selection_end(self, which, x, y)
    }
    fn render_magnifier(&self, buf: &mut [u32], w: u32, h: u32, x: f64, y: f64) {
        TermView::render_magnifier(self, buf, w, h, x, y)
    }
}

/// build 产物：终端实例 + 主/CJK 字体来源名（供调用方诊断上报）
pub type BuiltTerm = (Box<dyn TermEmu>, String, Option<String>);

/// 终端模拟器工厂服务（注册表式、独占绑定 v1）。build 瞬时返回：
/// 内嵌字体解析是毫秒级内存操作（BAR-021 起生产零文件 IO），不违反瞬时返回契约。
pub trait TermEmuFactory: Send + Sync {
    /// 建一台终端；Err = 字体全灭（调用方上报，不算插件失败——裁决 3）。
    /// Ok 附（主字体来源, CJK 字体来源）供调用方诊断上报（现状行为保持）
    fn build(&self) -> Result<BuiltTerm, String>;
}

/// 字体来源：Vendored = 生产（编译期内嵌，零探测，BAR-021）；
/// Probed = 考题注入夹具（按候选路径探测，host 无 /system/fonts）
pub enum FactoryFonts {
    Vendored,
    Probed(&'static [&'static str]),
}

/// alacritty 芯工厂：生产 = 内嵌字体直载；考题 = 候选表探测夹具
pub struct AlacrittyEmuFactory {
    fonts: FactoryFonts,
}

impl AlacrittyEmuFactory {
    /// 生产构造：编译期内嵌字体，零探测
    pub fn vendored() -> Self {
        AlacrittyEmuFactory {
            fonts: FactoryFonts::Vendored,
        }
    }

    /// 注入字体候选表（契约考题用夹具；host 无 /system/fonts）
    pub fn new(candidates: &'static [&'static str]) -> Self {
        AlacrittyEmuFactory {
            fonts: FactoryFonts::Probed(candidates),
        }
    }
}

impl TermEmuFactory for AlacrittyEmuFactory {
    fn build(&self) -> Result<BuiltTerm, String> {
        let built = match &self.fonts {
            FactoryFonts::Vendored => build_vendored(),
            FactoryFonts::Probed(candidates) => build_from_candidates(candidates),
        };
        match built {
            Some((tv, main, cjk)) => Ok((Box::new(tv), main, cjk)),
            None => Err("字体全灭——TermView 建不成".into()),
        }
    }
}
