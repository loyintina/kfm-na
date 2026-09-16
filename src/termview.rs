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

/// 终端卡片壳几何（2026-09-11 用户拍板：终端页与三面板同配方装修，
/// 四页同骨架——外缘距屏边 16 + 环粗 3 + 环内留白 = 网格原点）。
/// 取代 BAR-005 的 12px 裸边距与 BAR-010 的动态顶带：壳环自带圆角屏
/// 语义（环在 16px 处先挡一圈，文字再让一段净垫不贴环），顶带常量化
/// 不再跟格高走（捏合缩放不再挪动网格原点，眼手两把尺永不打架）
pub const TERM_CARD_PAD: u32 = 12;

/// 横向净垫（2026-09-11 晚用户实拍拍板：纵垫 12 之外左右再让一整格
/// 字宽——「直接贴上了显得太紧」）。= 12 + 18 = 30
pub const TERM_CARD_PAD_X: u32 = TERM_CARD_PAD + CELL_W;

/// 网格原点 X（= 卡片壳左边距 16+3+30=49）：BAR-005 语义由壳环继承
pub const MARGIN_X: u32 = AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + TERM_CARD_PAD_X;

/// AI 对话页排版尺（期 0④ 提升为模块级：手势 px→行换算与渲染同尺）
pub const AI_PAGE_MARGIN_X: u32 = 60;
pub const AI_PAGE_TOP: u32 = 48;
pub const AI_PAGE_BOTTOM: u32 = 48;
pub const AI_PAGE_LINE_H: u32 = 64;
pub const AI_PAGE_PX: f32 = 40.0;
/// 标签栏文字字号（宪法 §四，标定值 2026-09-12）：2 格行高（72）内
/// 容 24 = 内嵌像素体 12px 的整数倍，网格原生不虚化
pub const TAB_TEXT_PX: f32 = 36.0;
/// 双池框圆角半径（宪法 §五，2026-09-12 二标）：= 页环卡片框 36——
/// 池是通用卡片（与页环同尺）；功能光标是开口框（ui/cursor.rs），
/// 两者分家不同源（用户拍板：光标不是卡片）
pub const POOL_FRAME_R: u32 = 36;
/// 思考块文字色（期 0④½）：比正文暗的灰紫——能读到思考在流，但不抢戏
pub const AI_THINK_FG: u32 = 0x007E_7A9E;
/// 收流后思考块的折叠占位行（2026-09-04 用户拍板：输出完自动折叠——
/// 思考往往不重要但必须存在；全文随消息存档，展开查看是未来的活）
pub const AI_THINK_COLLAPSED: &str = "· 已思考 ·";
/// 网格底缘留白（= 壳几何纵尺 16+3+12=31）：卡片底环之上不再贴字
pub const MARGIN_Y: u32 = AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + TERM_CARD_PAD;

/// 顶边距（壳几何恒值 = 纵尺 31；BAR-010 的圆角屏语义由壳环继承）。
/// 保留常量名供旧调用点/考题引用——语义已从「边距+一整行」变为「壳纵边距」
pub const MARGIN_TOP: u32 = MARGIN_Y;

/// 顶边距（常量化，2026-09-11）：不再跟随格高——壳环位置固定，网格原点
/// 固定，缩放任一档下原点不动（旧动态版会随格高挪原点，眼手两尺打架
/// 的温床）。参数保留只为调用点零改动
pub const fn margin_top(_cell_h: u32) -> u32 {
    MARGIN_Y
}

/// 捏合缩放格尺寸钳制区间（A 档考题钉死）：10x20 = 还能认出字的下限
/// （真机 1260 屏 ≈116 列 127 行，Termux 级密）；上限 2026-09-11 晚
/// 放宽到 6 倍档（108x216 ≈ 一屏 12 列 11 行——用户对标 Termux
/// 「放大到很夸张」；再往上列数先崩到个位数没法用，钳制本来就是
/// 可读可用闸不是能力闸）
pub const CELL_W_MIN: u32 = 10;
pub const CELL_W_MAX: u32 = 108;
pub const CELL_H_MIN: u32 = 20;
pub const CELL_H_MAX: u32 = 216;

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
    paint_page_frame_ring(
        &mut frame,
        buf_w,
        buf_h,
        bottom_inset,
        0,
        panel_off,
        AI_PAGE_BG,
        AI_PAGE_FRAME_C1,
        AI_PAGE_FRAME_C2,
        false,
    );
    ai_page_fit(buf_h, bottom_inset)
}

/// 配置页底装修（面板栈 §五B，2026-09-10）：整页 CARD_PAGE_BG 深底 +
/// 边框环（配方与 AI 页同源 paint_page_frame_ring；2026-09-12 宪法
/// §2.2 起环色 = 召唤即随机的 accent 入参，不再固定青系）。
/// cfg_off_x = 面板刚体水平平移（+w=屏外右缘 → 0 靠泊）。
/// v1 = 空白骨架，无内容墨
pub fn paint_cfg_page_chrome(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    bottom_inset: u32,
    cfg_off_x: i32,
    accent: crate::ui::accent::AccentPair,
) {
    if buf_w == 0 || buf_h == 0 {
        return;
    }
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    // 整页底色 = 面板刚体矩形（全屏）与屏求交后画（X 向平移，左右裁剪）
    let px0 = cfg_off_x.clamp(0, buf_w as i32) as u32;
    let px1 = (buf_w as i32 + cfg_off_x).clamp(0, buf_w as i32) as u32;
    if px1 > px0 {
        frame.fill_rect(px0, 0, px1 - px0, buf_h, crate::ui::accent::CARD_PAGE_BG);
    }
    paint_page_frame_ring(
        &mut frame,
        buf_w,
        buf_h,
        bottom_inset,
        cfg_off_x,
        0,
        crate::ui::accent::CARD_PAGE_BG,
        accent.c1,
        accent.c2,
        true,
    );
}

/// 配置页分层判定（对照 panel_split，裁决语义同构）：
/// - 网格+快捷键行（下层可见）：cfg_off != 0；
/// - 配置页可见：cfg_off < w（off ∈ [0, +w]，=w 即完全屏外右缘）
pub fn cfg_split(cfg_off: i32, w: u32) -> (bool, bool) {
    (cfg_off != 0, cfg_off < w as i32)
}

/// 文件树页底装修（面板栈 §五B 三公民，2026-09-11）：整页 CARD_PAGE_BG
/// 深底 + 边框环（配方与配置页同源；2026-09-12 宪法 §2.2 起环色 =
/// 召唤即随机的 accent 入参，不再固定绿系）。
/// ft_off_x = 面板刚体水平平移（-w=屏外左缘 → 0 靠泊，与配置家镜像——
/// 底色求交公式 px0=off.clamp(0,w) / px1=(w+off).clamp(0,w) 对负偏移
/// 天然成立）。v1 = 空白骨架，无内容墨
pub fn paint_ft_page_chrome(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    bottom_inset: u32,
    ft_off_x: i32,
    accent: crate::ui::accent::AccentPair,
) {
    if buf_w == 0 || buf_h == 0 {
        return;
    }
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    // 整页底色 = 面板刚体矩形（全屏）与屏求交后画（X 向平移，左右裁剪）
    let px0 = ft_off_x.clamp(0, buf_w as i32) as u32;
    let px1 = (buf_w as i32 + ft_off_x).clamp(0, buf_w as i32) as u32;
    if px1 > px0 {
        frame.fill_rect(px0, 0, px1 - px0, buf_h, crate::ui::accent::CARD_PAGE_BG);
    }
    paint_page_frame_ring(
        &mut frame,
        buf_w,
        buf_h,
        bottom_inset,
        ft_off_x,
        0,
        crate::ui::accent::CARD_PAGE_BG,
        accent.c1,
        accent.c2,
        true,
    );
}

/// 文件树页分层判定（对照 cfg_split，镜像同构）：
/// - 网格+快捷键行（下层可见）：ft_off != 0；
/// - 文件树页可见：ft_off > -w（off ∈ [-w, 0]，=-w 即完全屏外左缘）
pub fn ft_split(ft_off: i32, w: u32) -> (bool, bool) {
    (ft_off != 0, ft_off > -(w as i32))
}

/// 解析页底装修（面板栈 §五B 四公民·三缘语义，2026-09-12）：整页
/// CARD_PAGE_BG 深底 + 边框环（配方与配置页同源，环色 = 召唤即随机
/// accent 入参）——右缘家符号约定完全相同：pt_off_x = 面板刚体水平
/// 平移（+w=屏外右缘 → 0 靠泊）。v1 = 空白骨架，无内容墨
pub fn paint_parser_page_chrome(
    buf: &mut [u32],
    buf_w: u32,
    buf_h: u32,
    bottom_inset: u32,
    pt_off_x: i32,
    accent: crate::ui::accent::AccentPair,
) {
    if buf_w == 0 || buf_h == 0 {
        return;
    }
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    // 整页底色 = 面板刚体矩形（全屏）与屏求交后画（X 向平移，左右裁剪）
    let px0 = pt_off_x.clamp(0, buf_w as i32) as u32;
    let px1 = (buf_w as i32 + pt_off_x).clamp(0, buf_w as i32) as u32;
    if px1 > px0 {
        frame.fill_rect(px0, 0, px1 - px0, buf_h, crate::ui::accent::CARD_PAGE_BG);
    }
    paint_page_frame_ring(
        &mut frame,
        buf_w,
        buf_h,
        bottom_inset,
        pt_off_x,
        0,
        crate::ui::accent::CARD_PAGE_BG,
        accent.c1,
        accent.c2,
        true,
    );
}

/// 解析页分层判定（右缘家，与 cfg_split 同构同尺）：
/// - 网格+快捷键行（下层可见）：pt_off != 0；
/// - 解析页可见：pt_off < w（off ∈ [0, +w]，=w 即完全屏外右缘）
pub fn pt_split(pt_off: i32, w: u32) -> (bool, bool) {
    cfg_split(pt_off, w)
}

/// 终端卡片壳底装修（2026-09-11 用户拍板「终端也包全屏卡片壳」）：
/// 与三面板同配方 paint_page_frame_ring，无色相碳灰环 + 近黑内芯底
/// （卡片感 = 壳内略亮于壳外纯黑）。无平移无动画——基座页恒靠泊；
/// 网格从 (MARGIN_X, MARGIN_Y) 起画，壳内芯留白带与右/下余量露出
/// TERM_CARD_BG。bottom_inset = 键盘 + 快捷键行 + 输入栏带（壳下缘
/// 停在快捷键行上沿之上 16px——与三面板停输入栏带上沿同尺）
pub fn paint_term_card_chrome(buf: &mut [u32], buf_w: u32, buf_h: u32, bottom_inset: u32) {
    if buf_w == 0 || buf_h == 0 {
        return;
    }
    // 小缓冲安全归 paint_page_frame_ring 的 i64 算术+早退（根修见彼处），
    // 这里不再钳 inset——环画不下时它自己退，内芯 fill 有 y1>m 闸
    let mut frame = Frame {
        buf,
        w: buf_w,
        h: buf_h,
    };
    let m = AI_PAGE_FRAME_MARGIN;
    let y1 = buf_h.saturating_sub(bottom_inset.saturating_add(m));
    if y1 > m && buf_w > 2 * m {
        frame.fill_rect(m, m, buf_w - 2 * m, y1 - m, TERM_CARD_BG);
    }
    paint_page_frame_ring(
        &mut frame,
        buf_w,
        buf_h,
        bottom_inset,
        0,
        0,
        TERM_CARD_BG,
        TERM_FRAME_C1,
        TERM_FRAME_C2,
        false,
    );
    // 设置钮（2026-09-12 配置池卡按钮入口）：画进终卡槽——面板靠泊时
    // 本槽整层隐（slot_visibility），「只在裸终端页出现」白拿零新逻辑
    crate::ui::gear::paint(frame.buf, buf_w, buf_h);
}

/// 页面边框环（2026-09-04 装修配方的唯一实体，09-05 平移参数化，
/// 09-10 双色相化+双轴平移供配置页复用）：页矩形（margin/inset/off）
/// 算好后转 paint_rect_ring——配方本体在矩形核里。
/// (off_x, off_y) 整体平移——环是面板装修，跟面板一起动。
/// 空态也画：框是页面装修不是内容
#[allow(clippy::too_many_arguments)]
fn paint_page_frame_ring(
    frame: &mut Frame<'_>,
    buf_w: u32,
    buf_h: u32,
    bottom_inset: u32,
    off_x: i32,
    off_y: i32,
    bg: u32,
    c1: u32,
    c2: u32,
    grad_fill: bool,
) {
    let fx0 = AI_PAGE_FRAME_MARGIN as i64 + i64::from(off_x);
    let fy0 = AI_PAGE_FRAME_MARGIN as i64 + i64::from(off_y);
    // i64 算术（2026-09-11 根修：终端卡片壳小缓冲实踩 u32 减法下溢——
    // buf_w/h 或 inset 小于边距时 (buf_w - MARGIN) 直接 panic；先转 i64
    // 再减，下面的早退检查才有意义，所有调用方共享这份安全）
    let fx1 = i64::from(buf_w) - i64::from(AI_PAGE_FRAME_MARGIN) + i64::from(off_x);
    let fy1 = i64::from(buf_h) - i64::from(bottom_inset) - i64::from(AI_PAGE_FRAME_MARGIN)
        + i64::from(off_y);
    // 页环不裁（clip 全宽——它自己就是边境）
    paint_rect_ring(
        frame,
        fx0,
        fy0,
        fx1,
        fy1,
        0,
        i64::MAX,
        bg,
        c1,
        c2,
        AI_PAGE_FRAME_R,
        grad_fill,
    );
}

/// 视口平移双代同画（十七修 §六「面与内容一体」）：借两枚 thread_local
/// 复用帧尺 temp 给闭包（旧代/新代各一，免逐帧 10MB 分配）。调用纪律：
/// 闭包内先 copy_frame 铺底（页环/池内芯是静物，随代整体平移），再画
/// 本代内容，最后 blit_shift 带偏移贴回主帧
fn pan_temps(w: u32, h: u32, f: impl FnOnce(&mut Vec<u32>, &mut Vec<u32>)) {
    thread_local! {
        static PAN_TEMP_A: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
        static PAN_TEMP_B: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    PAN_TEMP_A.with(|ta| {
        PAN_TEMP_B.with(|tb| {
            let mut a = ta.borrow_mut();
            let mut b = tb.borrow_mut();
            let n = (w as usize) * (h as usize);
            a.resize(n, 0);
            b.resize(n, 0);
            f(&mut a, &mut b);
        });
    });
}

/// 整帧铺底拷贝（双代 temp 的静物底：页环/池内芯不动部分随代平移）
fn copy_frame(src: &[u32], dst: &mut [u32]) {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
}

/// 水平移位不透明贴回：src[(y, x−dx)] → frame[(y, x)]，x 与 x−dx 都钳在
/// band 的 x 带内（带外 = 视口外，主帧静物原样保留）；y 带同理（无竖向
/// 移位，带即上下裁剪）。内容出视口缘断墨 = 旧代滑出、新代滑入的边界
fn blit_shift(frame: &mut Frame<'_>, src: &[u32], dx: i64, band: (i64, i64, i64, i64)) {
    let (x0, y0, x1, y1) = band;
    let (fw, fh) = (i64::from(frame.w), i64::from(frame.h));
    let (xa, xb) = (x0.max(0), x1.min(fw));
    let (ya, yb) = (y0.max(0), y1.min(fh));
    for y in ya..yb {
        let row = y as usize * fw as usize;
        for x in xa..xb {
            let sx = x - dx;
            if sx < x0 || sx >= x1 || sx < 0 || sx >= fw {
                continue; // 来源在视口带外 = 该处无内容滑到，主帧静物保留
            }
            frame.buf[row + x as usize] = src[row + sx as usize];
        }
    }
}

/// 页面级平移裁剪带（十九修 D8 函数化提取：涂装域 softbuffer 与合成域
/// GLES scissor 同吃一把尺，像素语义 = 十七/十八修原式样不变）。y 取
/// 两代池几何并集——旧代池高弹簧中几何可异，带必须罩住两代，否则
/// 合成域 band fill 擦不净旧帧
pub fn page_pan_band(
    w: u32,
    off: i64,
    old_up_y: i64,
    new_up_y: i64,
    old_low_b: i64,
    new_low_b: i64,
) -> (i64, i64, i64, i64) {
    let (ox, _oy) = crate::ui::tab_bar::content_origin();
    (
        i64::from(ox) + off,
        old_up_y.min(new_up_y),
        i64::from(w) - i64::from(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_W) + off,
        old_low_b.max(new_low_b),
    )
}

/// 上池级平移裁剪带（十八修 §七内容矩形：POOL_CONTENT_INSET 内缩，
/// 池框/左粗竖条一像素不进带——BAR-091 语义，合成域 scissor 同尺）
pub fn upper_pan_band(upper: &crate::ui::dual_pool::PoolRect, off: i64) -> (i64, i64, i64, i64) {
    (
        upper.x + crate::ui::cfg_page::POOL_CONTENT_INSET + off,
        upper.y + 12,
        upper.x + upper.w as i64 - crate::ui::cfg_page::POOL_CONTENT_INSET + off,
        upper.y + upper.h as i64 - 12,
    )
}

/// 圆角矩形边框环（2026-09-12 从页环抽核，配置卡标签栏光标框复用——
/// 宪法 §六 样式唯一来源，禁止逐卡手抄）：先外发光，再 135° 渐变外环，
/// 最后内芯填充（左缘让 9 = 3 倍粗，其余让 3）。**内芯两路（十二修
/// §三 渐变暗背景）**：grad_fill=true = 渐变暗底（frame_bg_rgb 同尺
/// 不透明直出——页环/池框/跳框卡/预览演示）；false = 平色 bg punch
/// （终端卡片壳/AI 页——基座与主题色页不动）。
/// clip_x0/x1 = X 向内容裁剪带（光标框横滚滑出内容带时不许污染页环带；
/// 页环自己传 [0, i64::MAX] 不裁）。空态也画：框是装修不是内容。
/// r_max = 调用方半径上限（页环 36 / 光标框 12——胶囊→小圆角偏方，
/// 2026-09-12 用户拍板），仍按短边一半钳
#[allow(clippy::too_many_arguments)]
fn paint_rect_ring(
    frame: &mut Frame<'_>,
    fx0: i64,
    fy0: i64,
    fx1: i64,
    fy1: i64,
    clip_x0: i64,
    clip_x1: i64,
    bg: u32,
    c1: u32,
    c2: u32,
    r_max: u32,
    grad_fill: bool,
) {
    let (fw, fh) = ((fx1 - fx0) as u32, (fy1 - fy0) as u32);
    if fw < 2 || fh < 2 {
        return;
    }
    // 半径按短边钳（小矩形 = 体育场端帽——标签栏光标框 36px 高实踩：
    // 早退拿满 R=36 判会把 1 格高的框整个吞掉）。页环尺寸下 rc==r_max
    // 恒成立，老行为不变
    let r = i64::from(r_max).min((fw / 2).min(fh / 2) as i64);
    if fx1 < fx0 + 2 * r || fy1 < fy0 + 2 * r {
        return;
    }
    let rc = r;
    let w = i64::from(AI_PAGE_FRAME_W);
    let spread = 14i64;
    let (gc, ga) = (c2, 64u32);
    let denom = ((fw - 1) + (fh - 1)).max(1);

    // BAR-103 渐变 LUT（2026-09-16 计时考题定罪：全页重烘 63ms =
    // 池框 29+页环 18+内容 15，文字非瓶颈——内芯/环带逐像素三重 lerp
    // + 每像素边界检查才是。环带色与内芯色都只是 s=lx+ly 的一元函数
    // （同一把 135° 尺同分母），一次建表 denom+1 项、内芯中带行切片
    // 直写 ≈ memset 速。逐像素等价钉：spec_bar103_渐变LUT_采样钉
    let ring_lut: Vec<u32> = (0..=denom as i64)
        .map(|s| ring_gradient_rgb(c1, c2, s, 0, denom as i64))
        .collect();
    let bg_lut: Vec<u32> = if grad_fill {
        (0..=denom as i64)
            .map(|s| frame_bg_rgb(c1, c2, s, 0, denom as i64))
            .collect()
    } else {
        Vec::new()
    };

    // 带切（2026-09-06 ras 40ms→8ms）：三种墨的可能墨域都只是矩形周边
    // 的薄带——整包围盒逐像素 SDF 的 95% 是零墨或被 punch 覆盖的废访。
    // 各层给「每行列跨度」安全超集，墨函数一字不动（配方钉判逐像素）。
    //   发光：角带行（±spread）全宽，中带行只左右缘条；
    //   渐变：角带行（rc+w）全宽，中带行左缘 3w/右缘 w 条；
    //   punch：中带是纯底色 fill_rect（一次调用），只有上下角带逐像素。
    let mut spans = Vec::with_capacity(2);
    let row_spans = |ly: i64, spans: &mut Vec<(i64, i64)>| {
        spans.clear();
        // 发光行带
        if ly >= -spread && ly < i64::from(fh) + spread {
            if ly < rc + spread || ly >= i64::from(fh) - rc - spread {
                spans.push((fx0 - spread - 1, fx0 + i64::from(fw) + spread + 1));
            } else {
                spans.push((fx0 - spread - 1, fx0 + 1));
                spans.push((fx0 + i64::from(fw) - 1, fx0 + i64::from(fw) + spread + 1));
            }
        }
        // 渐变行带
        if ly >= 0 && ly < i64::from(fh) {
            if ly < rc + w || ly >= i64::from(fh) - rc - w {
                spans.push((fx0, fx0 + i64::from(fw)));
            } else {
                spans.push((fx0, fx0 + w * 3));
                spans.push((fx0 + i64::from(fw) - w, fx0 + i64::from(fw)));
            }
        }
    };

    let y0 = (fy0 - spread).max(0);
    let y1 = (fy0 + i64::from(fh) + spread).min(i64::from(frame.h));
    let mut cur_row = i64::MIN;
    for ay in y0..y1 {
        let ly = ay - fy0;
        if cur_row != ly {
            cur_row = ly;
            row_spans(ly, &mut spans);
        }
        for (sx0, sx1) in &spans {
            let ax0 = (*sx0).max(0).max(fx0 - spread - 1).max(clip_x0);
            let ax1 = (*sx1).min(i64::from(frame.w)).min(clip_x1);
            for ax in ax0..ax1 {
                let lx = (ax - fx0) as u32;
                // 发光：真实 ly（可为负——矩形外的辉光带；钳 0 会把框外
                // 误判成框缘，铸成上下粗边，2026-09-06 装机实看 BAR-069）
                let d = rr_sdf(lx as f32 + 0.5, ly as f32 + 0.5, fw, fh, r as u32);
                if d > 0.0 {
                    let t = (1.0 - d / spread as f32).max(0.0);
                    let a = (ga as f32 * t * t) as u32;
                    if a > 0 {
                        frame.blend_px(ax as u32, ay as u32, gc, a);
                    }
                }
                // 渐变外环（135° 对角，t = lx+ly 归一）——只在矩形行内：
                // 框外行没有渐变墨（发光带不是边框）
                if ly >= 0 && ly < i64::from(fh) {
                    let cov = rr_cover(lx, ly as u32, fw, fh, rc as u32);
                    if cov > 0 {
                        let color = ring_lut[(i64::from(lx) + ly) as usize];
                        if cov == 255 {
                            frame.buf[ay as usize * frame.w as usize + ax as usize] = color;
                        } else {
                            frame.blend_px(ax as u32, ay as u32, color, cov);
                        }
                    }
                }
            }
        }
    }
    // 内芯填充：左缘 3 倍粗（x 让 3W，其余让 W）——grad_fill = 渐变暗底
    // 逐像素（同一把 135° 尺：lx/ly 相对框原点，denom 同环带）；否则
    // 中带纯底色 fill_rect（一次调用），只有上下角带逐像素
    let ix = fx0 + w * 3;
    let iy = fy0 + w;
    let iw = ((fx1 - w) - ix).max(0) as u32;
    let ih = ((fy1 - w) - iy).max(0) as u32;
    let punch_r = (r - w).min((iw / 2).min(ih / 2) as i64);
    if iw == 0 || ih == 0 {
        return;
    }
    if i64::from(ih) > 2 * punch_r {
        let my0 = (iy + punch_r).max(0);
        let my1 = (iy + i64::from(ih) - punch_r).min(i64::from(frame.h));
        if my1 > my0 {
            let rx0 = ix.max(0).max(clip_x0);
            let rx1 = (ix + i64::from(iw)).min(i64::from(frame.w)).min(clip_x1);
            if rx1 > rx0 {
                if grad_fill {
                    // 行切片直写（BAR-103）：s = (ax-fx0)+(ay-fy0) 沿行
                    // 单调 +1，iter_mut 免逐像素边界检查
                    for ay in my0..my1 {
                        let s_base = (rx0 - fx0 + (ay - fy0)) as usize;
                        let start = ay as usize * frame.w as usize + rx0 as usize;
                        let row = &mut frame.buf[start..start + (rx1 - rx0) as usize];
                        for (i, px) in row.iter_mut().enumerate() {
                            *px = bg_lut[s_base + i];
                        }
                    }
                } else {
                    frame.fill_rect(
                        rx0 as u32,
                        my0 as u32,
                        (rx1 - rx0) as u32,
                        (my1 - my0) as u32,
                        bg,
                    );
                }
            }
        }
    }
    let py0 = (iy).max(0);
    let py1 = (iy + i64::from(ih)).min(i64::from(frame.h));
    for ay in py0..py1 {
        let lyy = (ay - iy) as u32;
        let in_corner = lyy < punch_r as u32 || lyy >= ih - punch_r as u32;
        if !in_corner {
            continue; // 中带已填
        }
        let ax0 = ix.max(0).max(clip_x0);
        let ax1 = (ix + i64::from(iw)).min(i64::from(frame.w)).min(clip_x1);
        for ax in ax0..ax1 {
            let lx = (ax - ix) as u32;
            let cov = rr_cover(lx, lyy, iw, ih, punch_r as u32);
            if cov > 0 {
                let color = if grad_fill {
                    bg_lut[(ax - fx0 + (ay - fy0)) as usize]
                } else {
                    bg
                };
                if cov == 255 {
                    frame.buf[ay as usize * frame.w as usize + ax as usize] = color;
                } else {
                    frame.blend_px(ax as u32, ay as u32, color, cov);
                }
            }
        }
    }
}

/// 三级框行涂装（宪法 §五 池行条款：四版立、六修收窄、七修角部渐细、
/// 十一修渐变色归位、**十二修未选中去框+渐变暗底**，2026-09-14）——
/// **文件级共享组件**（§六 样式唯一来源：三级框禁止逐卡手抄；下池行/
/// 上池值框/下拉选项行三处共用这一份）。
/// **选中 = 全包框**：左粗竖线 10px + 三细边 3px **整框 135° 双色渐变
/// α255**（与页环**同向** c1→c2——§三 逐层反转：页环正 → 池框反 →
/// 三级框还原）。**角部渐细只渐形状不渐色**：非对称内芯配方（内芯左让
/// BAR_L、其余让 THIN、半径 R−THIN，角心偏移 = 厚度渐细来源），颜色
/// 全程渐变采样。
/// **未选中 = 无框**（十二修，用户拍板：「暗形态把所有框都取消，只留
/// 渐变暗背景」）：框墨全退，圆角剪影内只剩渐变暗底——α140 薄态与
/// 角弧 alpha 过渡（十一修）一并退役。
/// **内芯 = 渐变暗底**（十二修 §三：frame_bg_rgb 页尺同源不透明直出，
/// 取代 4% 白平填）——选中/未选中同吃。上池值框 = 传 sel=false 即
/// 无边框组件（十一修形态②「只有左竖线」退役）。
/// clip = Y 向裁剪带（上池滚动出池内缘断墨）。
#[allow(clippy::too_many_arguments)]
fn paint_row_frame_gradref(
    frame: &mut Frame<'_>,
    x: i64,
    y: i64,
    rw: u32,
    rh: u32,
    sel: bool,
    accent: crate::ui::accent::AccentPair,
    clip: (i64, i64),
    // 渐变参照（x 偏移, y 偏移, 分母）——BAR-096 拆层保真：小画布上的
    // 框必须采出「它在页上原位」的颜色（偏移=层在页上的原点、分母=
    // 页尺）；整页内绘制传 (0, 0, denom) 即恒等
    grad_ref: (i64, i64, i64),
) {
    const THIN: i64 = 3; // 细边宽（左粗：细 ≈ 3:1）
    const BAR_L: i64 = 10; // 左粗竖线宽（六修标定 10px）
    // §七 标定三版：三级框圆角 = 池框同尺 36px（12px 是标签光标专用），
    // 仍按短边一半钳（与 paint_rect_ring 同规）
    let r = (POOL_FRAME_R as i64).min((rw / 2).min(rh / 2) as i64);
    // 非对称内芯（选中全包框）：左让 BAR_L、其余让 THIN，半径 r−THIN——
    // 角心相对外角心偏 (BAR_L−THIN, 0)，角部厚度从左缘 BAR_L 平滑收到
    // THIN。未选中无内芯概念（剪影内全是渐变暗底）
    let (ix, iy, iw, ih, ir) = (
        x + BAR_L,
        y + THIN,
        rw.saturating_sub((BAR_L + THIN) as u32),
        rh.saturating_sub((2 * THIN) as u32),
        (r - THIN).max(0) as u32,
    );
    if iw < 2 || ih < 2 {
        return;
    }
    let (fw, fh) = (i64::from(frame.w), i64::from(frame.h));
    // BAR-103 渐变 LUT（同 paint_rect_ring）：行框内芯/环带色只是
    // s=(xx+rx)+(yy+ry) 的一元函数（原实现每像素 1 次整数除法 + 内芯
    // 每像素另付 2 次压暗 lerp——下池 6 行+上池 6 字段 ≈ 1.6M px 实
    // 测 15ms 的构成）。建表 denom+1 项，行切片直写免边界检查；
    // s 越界钳 [0, denom] 与原函数 max(0)/t.min(255) 语义全等
    let d = grad_ref.2.max(1);
    let ring_lut: Vec<u32> = (0..=d)
        .map(|s| ring_gradient_rgb(accent.c1, accent.c2, s, 0, d))
        .collect();
    let bg_lut: Vec<u32> = (0..=d)
        .map(|s| frame_bg_rgb(accent.c1, accent.c2, s, 0, d))
        .collect();
    for dy in 0..rh as i64 {
        let yy = y + dy;
        if yy < 0 || yy >= fh || yy < clip.0 || yy >= clip.1 {
            continue;
        }
        let x_start = x.max(0);
        let x_end = (x + i64::from(rw)).min(fw);
        if x_end <= x_start {
            continue;
        }
        let row_base = yy as usize * fw as usize;
        let row = &mut frame.buf[row_base + x_start as usize..row_base + x_end as usize];
        let s_row = yy + grad_ref.1 + grad_ref.0; // s = s_row + xx
        for xx in x_start..x_end {
            let dx = xx - x;
            if rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, rw, rh, r as u32) >= 0.0 {
                continue; // 外剪影外
            }
            let s = (s_row + xx).clamp(0, d) as usize;
            if sel && rr_sdf((xx - ix) as f32 + 0.5, (yy - iy) as f32 + 0.5, iw, ih, ir) >= 0.0 {
                // 选中框环带：颜色全程 = 渐变采样 α255（只渐形状不渐色）；
                // blend_px(·,·,·,255) 内联（blend α255 ≡ fg，仅保目标 α——
                // 行切片借用期不能再借 frame，语义逐比特一致）
                let dst = &mut row[(xx - x_start) as usize];
                *dst = (*dst & 0xFF00_0000) | ring_lut[s];
                continue;
            }
            // 内芯（或未选中整剪影）= 渐变暗底不透明直出（十二修 §三）
            row[(xx - x_start) as usize] = bg_lut[s];
        }
    }
}

/// 三级框行涂装（源签名薄包装）：渐变参照 = 画布自身尺——整页内绘制
/// 语义与历史逐像素一致；拆层绘制走 paint_row_frame_gradref 显式给参照
#[allow(clippy::too_many_arguments)]
fn paint_row_frame(
    frame: &mut Frame<'_>,
    x: i64,
    y: i64,
    rw: u32,
    rh: u32,
    sel: bool,
    accent: crate::ui::accent::AccentPair,
    denom: i64,
    clip: (i64, i64),
) {
    paint_row_frame_gradref(frame, x, y, rw, rh, sel, accent, clip, (0, 0, denom));
}

/// 均匀细框涂装（宪法 §五 十一修新立：**非池行场合的通用细框**，不属
/// 三级框——跳框关闭钮/预览展台/预览微缩件的栏框键格用）：
/// 四边 3px accent 渐变（135° 同尺，对称内芯）+ **渐变暗底内芯**
/// （十二修 §三，取代 4% 白填）；不挂左粗缘（左粗是选择语言的视觉
/// 载荷，§五 六修收窄条款）。圆角 = 池框同尺 36px 按短边一半钳
/// （与 paint_rect_ring 同规）
#[allow(clippy::too_many_arguments)]
fn paint_thin_frame(
    frame: &mut Frame<'_>,
    x: i64,
    y: i64,
    rw: u32,
    rh: u32,
    accent: crate::ui::accent::AccentPair,
    denom: i64,
    clip: (i64, i64),
) {
    const T: i64 = 3; // 细边宽
    let r = (POOL_FRAME_R as i64).min((rw / 2).min(rh / 2) as i64);
    let (ix, iy) = (x + T, y + T);
    let iw = rw.saturating_sub((2 * T) as u32);
    let ih = rh.saturating_sub((2 * T) as u32);
    if iw < 2 || ih < 2 {
        return;
    }
    let ir = (r - T).max(0) as u32;
    let (fw, fh) = (i64::from(frame.w), i64::from(frame.h));
    for dy in 0..rh as i64 {
        let yy = y + dy;
        if yy < 0 || yy >= fh || yy < clip.0 || yy >= clip.1 {
            continue;
        }
        for dx in 0..rw as i64 {
            let xx = x + dx;
            if xx < 0 || xx >= fw {
                continue;
            }
            if rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, rw, rh, r as u32) >= 0.0 {
                continue;
            }
            if rr_sdf((xx - ix) as f32 + 0.5, (yy - iy) as f32 + 0.5, iw, ih, ir) < 0.0 {
                // 内芯 = 渐变暗底不透明直出（十二修 §三，取代 4% 白填）
                frame.buf[yy as usize * fw as usize + xx as usize] =
                    frame_bg_rgb(accent.c1, accent.c2, xx, yy, denom);
                continue;
            }
            let grad = ring_gradient_rgb(accent.c1, accent.c2, xx, yy, denom);
            frame.blend_px(xx as u32, yy as u32, grad, 255);
        }
    }
}

/// 标签页块涂装（宪法 §四 八修新立、十一修入随机色体系、**十二修选中
/// 块均匀渐变**，2026-09-14；文件级共享组件，§六 样式唯一来源）：
/// **无边框色块标签**——上两角圆角 R=1 格、下缘直边（标签坐在行底，
/// 下接底线组件）。竖向分色：sel=true = **c1（顶）→c2（底）竖向均匀
/// 渐变满填 α255**（十二修推翻十一修两截硬切——实机判「硬切换」，
/// t = dy·255/(h−1) 逐行精确）；sel=false = **同一把 t 尺的均匀渐变
/// 薄态 α48 满块**（十三修推翻三段条带硬切——实机判「暗块依然硬切」，
/// 只降 alpha 不降连续性）。色源 = **该标签自己的双色**（pair 参数 =
/// 标签栏色列条目，每标签独立随机——§四十一修；不是页 accent）。
/// clip_x0/x1 = X 向内容裁剪带（横滚滑出不污染页环带）
#[allow(clippy::too_many_arguments)]
fn paint_tab_chip(
    frame: &mut Frame<'_>,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    sel: bool,
    pair: crate::ui::accent::AccentPair,
    clip_x0: i64,
    clip_x1: i64,
) {
    let r = (CELL_W as i64).min((w / 2).min(h / 2) as i64); // 上圆角 1 格
    for dy in 0..h as i64 {
        let yy = y + dy;
        if yy < 0 || yy >= i64::from(frame.h) {
            continue;
        }
        for dx in 0..w as i64 {
            let xx = x + dx;
            if xx < 0 || xx >= i64::from(frame.w) || xx < clip_x0 || xx >= clip_x1 {
                continue;
            }
            // 上两角圆角：角盒内按圆弧裁（采样 +0.5 与 rr_sdf 同规）；
            // 下缘直边不裁
            if dy < r {
                let in_tl = dx < r;
                let in_tr = dx >= w as i64 - r;
                if in_tl || in_tr {
                    let cx = if in_tl { r } else { w as i64 - r };
                    let (px, py) = (dx as f64 + 0.5 - cx as f64, dy as f64 + 0.5 - r as f64);
                    if (px * px + py * py).sqrt() > r as f64 {
                        continue; // 角外不画
                    }
                }
            }
            if sel {
                // 竖向均匀渐变（十二修）：t = dy·255/(h−1)，顶 c1 底 c2
                let t = (dy as u32 * 255) / (h - 1).max(1);
                frame.blend_px(xx as u32, yy as u32, lerp_rgb(pair.c1, pair.c2, t), 255);
            } else {
                // 未选中 = 同一把 t 尺的均匀渐变薄态 α48（十三修：三段
                // 条带硬切实机判「暗块依然硬切」退役——只降 alpha 不降
                // 连续性）
                let t = (dy as u32 * 255) / (h - 1).max(1);
                frame.blend_px(xx as u32, yy as u32, lerp_rgb(pair.c1, pair.c2, t), 48);
            }
        }
    }
}

/// 功能光标开口框涂装（宪法 §三/§四 三修立、四修标定、五修宽+色，
/// 2026-09-13；kfmv4 `renderer.ts _drawCursorBorder` 复刻 + NA 标定）：
/// ①绿青半透明底垫整个框体；②左强调线 8px 画在**框内缘**（上下跳过
/// 圆角区；kfmv4 的 1.65px 突出不移植——装修框左缘都在框内，光标同规，
/// 与双池左框逐像素一线）；③左上/左下圆角 R=12 线宽沿弧渐变
/// （角顶 8px→角尾 3px，SDF 弧带覆盖率抗锯齿）；④顶线/底线 3px 锚左
/// （行带铺满 3 行），长度由调用方喂（cursor.rs 随机机制产物，涂装
/// 不自算）。**无右边、无发光、无 punch**——开口框的观感本体就是
/// 「缺的那一边」。线色 = 本卡 accent 双色渐变（五修）：grad 渐变框
/// 必须与页环同源（同原点同分母），光标像从页环渐变布上剪下来。
/// clip_x0/x1 = X 向内容裁剪带（与标签文字同一条带，眼手同尺：
/// 带外看不见也点不着；左线已收进框内，无需额外放宽）
///
/// **封存复活（2026-09-13 十修）**：标签栏换案填色标签块后本涂装退役，
/// 十修跳框预览画板（Preview::OpenCursor）重新调用；文件树光标仍待用
#[allow(clippy::too_many_arguments)]
fn paint_open_cursor(
    frame: &mut Frame<'_>,
    x0: i64,
    y0: i64,
    w: i64,
    h: i64,
    top_w: i64,
    bot_w: i64,
    clip_x0: i64,
    clip_x1: i64,
    grad: RingGradient,
    bg: u32,
) {
    use crate::ui::cursor as cur;
    if w < 2 || h < 2 {
        return;
    }
    let (x1, y1) = (x0 + w, y0 + h);
    let (fh, fw) = (i64::from(frame.h), i64::from(frame.w));
    let la = cur::LINE_ALPHA;
    let r = cur::CORNER_R;
    // ①底垫（整框，α0.15 绿青——标签本身无框，光标落脚处才出现）
    let bx0 = x0.max(clip_x0).max(0);
    let bx1 = x1.min(clip_x1).min(fw);
    for ay in y0.max(0)..y1.min(fh) {
        for ax in bx0..bx1 {
            frame.blend_px(ax as u32, ay as u32, bg, cur::BG_ALPHA);
        }
    }
    // ②左强调线：框内缘 [x0, x0+8) 满 α，上下跳过圆角区
    let lx1 = (x0 + cur::EMPHASIS_W).min(clip_x1).min(fw);
    for cx in x0.max(clip_x0).max(0)..lx1 {
        for ay in (y0 + r).max(0)..(y1 - r).min(fh) {
            frame.blend_px(cx as u32, ay as u32, grad.sample(cx, ay), la);
        }
    }
    // ③圆角（弧心 = 框缘内收 R；top=左上 8→3，false=左下 3→8）
    paint_cursor_arc(frame, x0, y0, r, true, clip_x0, clip_x1, grad, la);
    paint_cursor_arc(frame, x0, y1, r, false, clip_x0, clip_x1, grad, la);
    // ④细线（3px 锚左，行带铺满 HAIR_W 行）
    let hair = |hx0: i64, len: i64, hy0: i64, frame: &mut Frame<'_>| {
        if len <= 0 {
            return;
        }
        let sx = (hx0 + r).max(clip_x0).max(0);
        let ex = (hx0 + r + len).min(clip_x1).min(fw);
        for hy in hy0..hy0 + cur::HAIR_W {
            if hy < 0 || hy >= fh {
                continue;
            }
            for ax in sx..ex {
                frame.blend_px(ax as u32, hy as u32, grad.sample(ax, hy), la);
            }
        }
    };
    hair(x0, top_w, y0, frame);
    hair(x0, bot_w, y1 - cur::HAIR_W, frame);
}

/// 开口光标圆角弧（paint_open_cursor 的件）：弧带 SDF 覆盖率，线宽
/// 沿角渐变。**弧带外缘贴框缘向内铺**（BAR-087：dist=R 满覆盖、
/// dist>R 零墨，与直线段框内缘贴边同源同尺，不跨框）。edge_y = 弧
/// 所在框缘（top=上缘，弧心 y0+R；否则下缘，弧心 y1−R）；弧只画
/// 圆心左侧象限（dx≤0），角度归一后左上弧 [π, 3π/2] 宽 8→3、左下弧
/// [π/2, π] 宽 3→8。线色 = grad 渐变框逐像素采样（五修，页环同尺）
#[allow(clippy::too_many_arguments)]
fn paint_cursor_arc(
    frame: &mut Frame<'_>,
    x0: i64,
    edge_y: i64,
    r: i64,
    top: bool,
    clip_x0: i64,
    clip_x1: i64,
    grad: RingGradient,
    la: u32,
) {
    use crate::ui::cursor as cur;
    let (cx, cy) = (
        (x0 + r) as f64,
        (if top { edge_y + r } else { edge_y - r }) as f64,
    );
    let rf = r as f64;
    let ext = rf + cur::EMPHASIS_W as f64 / 2.0 + 1.0;
    let ax0 = (cx - ext).floor() as i64;
    let ax1 = (cx + 1.0).ceil() as i64;
    let (ay0, ay1) = if top {
        ((cy - ext).floor() as i64, (cy + 1.0).ceil() as i64)
    } else {
        ((cy - 1.0).floor() as i64, (cy + ext).ceil() as i64)
    };
    let (fh, fw) = (i64::from(frame.h), i64::from(frame.w));
    for ay in ay0..ay1 {
        if ay < 0 || ay >= fh {
            continue;
        }
        for ax in ax0..ax1 {
            if ax < clip_x0 || ax >= clip_x1 || ax < 0 || ax >= fw {
                continue;
            }
            let dx = ax as f64 + 0.5 - cx;
            let dy = ay as f64 + 0.5 - cy;
            if dx > 0.0 || (top && dy > 0.0) || (!top && dy < 0.0) {
                continue;
            }
            // 角度归一 [0, 2π)：左上弧 π→3π/2 宽 8→3；左下弧 π/2→π 宽 3→8
            let mut ang = dy.atan2(dx);
            if ang < 0.0 {
                ang += 2.0 * std::f64::consts::PI;
            }
            let t = if top {
                (ang - std::f64::consts::PI) / std::f64::consts::FRAC_PI_2
            } else {
                (ang - std::f64::consts::FRAC_PI_2) / std::f64::consts::FRAC_PI_2
            };
            let (w9, w3) = (cur::EMPHASIS_W as f64, cur::HAIR_W as f64);
            let wpx = if top {
                w9 + (w3 - w9) * t
            } else {
                w3 + (w9 - w3) * t
            };
            let dist = (dx * dx + dy * dy).sqrt();
            // 弧带外缘贴框缘（BAR-087，用户实机截图像素实测）：dist=R 满
            // 覆盖、向内铺 wpx、内缘 0.5px 羽化、dist>R 零墨——与直线段
            // 同源同尺（发丝线行带 [y0, y0+3)、左线列带 [x0, x0+8) 都是
            // 框内缘贴边），旧居中弧带（dist=R 对半跨边）在角区外凸
            // 4px/上凸 1px，三条线在角上肉眼错位
            let inward = rf - dist;
            let cov = if inward < 0.0 {
                0.0
            } else {
                (wpx - inward + 0.5).clamp(0.0, 1.0)
            };
            if cov > 0.0 {
                frame.blend_px(
                    ax as u32,
                    ay as u32,
                    grad.sample(ax, ay),
                    (f64::from(la) * cov) as u32,
                );
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
    /// 字形位图缓存（BAR-102，2026-09-16 na-rec 实录定罪：槽涂装/CPU 画字
    /// 每字每次 font.rasterize 从轮廓重光栅——全页重烘一次 50-130ms，
    /// 压在动画起步/终点/Upper 逐帧关键帧上 = 120Hz 屏肉眼黑洞）。
    /// key = (字, 字号 bits, 字体 id)；字体进程期不换、px 档位是固定小集合，
    /// 无需失效；4096 封顶清表防膨胀。命中 = 零光栅零分配（Arc 克隆——
    /// TermEmu:Send 不要 Rc）
    glyph_cache: GlyphCache,
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

/// 字形缓存容器类型（BAR-102；clippy type_complexity 要求抽别名——
/// 手机 1.97 咬字段原位写法，同 AiRow 先例）
type GlyphCache = std::cell::RefCell<
    std::collections::HashMap<(char, u32, u8), std::sync::Arc<(fontdue::Metrics, Vec<u8>)>>,
>;

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
            glyph_cache: GlyphCache::default(),
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

    /// 把当前可见网格渲染进 XRGB 帧缓冲（满幅重绘）。
    /// buf 尺寸必须与 buf_w*buf_h 一致（调用方 softbuffer 保证；不一致只画放得下的部分）。
    /// 2026-09-11 终端卡片壳：清屏纯黑（壳外）→ 卡片壳内芯+碳灰环 → 网格
    /// 单元（默认底色的格不补色块 = 透出壳内芯 TERM_CARD_BG，与 GLES 路径
    /// 「clear 黑 + 卡片槽 + 非默认底实例」逐像素同构）。card_bottom_inset
    /// = 壳下缘让位高度（前台 = 键盘+快捷键行+输入栏带；后台值守倒帧无
    /// chrome 视野传 0）
    pub fn render_into(&mut self, buf: &mut [u32], buf_w: u32, buf_h: u32, card_bottom_inset: u32) {
        buf.fill(DEFAULT_BG);
        if buf_w == 0 || buf_h == 0 {
            return;
        }
        paint_term_card_chrome(buf, buf_w, buf_h, card_bottom_inset);
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
        // paint_page_frame_ring（GPU chrome 路径 paint_ai_page_chrome 共用
        // 这一份——修配方两处一起修）。off=0：CPU 路径不平移
        paint_page_frame_ring(
            &mut frame,
            buf_w,
            buf_h,
            bottom_inset,
            0,
            0,
            AI_PAGE_BG,
            AI_PAGE_FRAME_C1,
            AI_PAGE_FRAME_C2,
            false,
        );
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
            let g = self.rasterize_cached(f, c, px); // BAR-102：缓存光栅
            let (m, bmp) = (&g.0, &g.1);
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

    /// 光栅化带缓存（BAR-102）：热路径统一入口——draw_glyph / draw_label /
    /// draw_text_centered / draw_items_left_inset 四处每字每次
    /// f.rasterize 从轮廓重光栅，是全页重烘 50-130ms 的主账。命中即
    /// Arc 克隆返回，零光栅零分配。字体 id 按地址判别（0=主 1=备）：
    /// draw_glyph 走 prefer_cjk、槽路径走 pick_font，两族判据对同字
    /// 可能分歧（主字体有该字但 prefer_cjk 判全角归备），字体必须进
    /// key，否则 (c,px) 巧合同值时串字体 = 拿错位图
    pub(crate) fn rasterize_cached(
        &self,
        f: &fontdue::Font,
        c: char,
        px: f32,
    ) -> std::sync::Arc<(fontdue::Metrics, Vec<u8>)> {
        let fid = if std::ptr::eq(f, &self.font) {
            0u8
        } else {
            1u8
        };
        let key = (c, px.to_bits(), fid);
        if let Some(g) = self.glyph_cache.borrow().get(&key) {
            return std::sync::Arc::clone(g);
        }
        let g = std::sync::Arc::new(f.rasterize(c, px));
        let mut cache = self.glyph_cache.borrow_mut();
        if cache.len() >= 4096 {
            cache.clear(); // 封顶清表：px 档位×常用字符远小于此，清了重建
        }
        cache.insert(key, std::sync::Arc::clone(&g));
        g
    }

    /// 考题专用通道（BAR-102 钉）：两次取同一字形——返回 (第二次是否
    /// 命中同一份 Arc, 缓存位图与直调 rasterize 是否逐字节一致)
    #[doc(hidden)]
    pub fn spec_glyph_cache_probe(&self, c: char, px: f32) -> (bool, bool) {
        let direct = self.font.rasterize(c, px);
        let g1 = self.rasterize_cached(&self.font, c, px);
        let g2 = self.rasterize_cached(&self.font, c, px);
        (
            std::sync::Arc::ptr_eq(&g1, &g2),
            g1.1 == direct.1 && g1.0.width == direct.0.width && g1.0.height == direct.0.height,
        )
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

    /// 文本实量宽 px（十四修字段框动态宽度：涂装/触摸命中/考题三方
    /// 同一条尺——integration test 是外部 crate 只能走 pub 口）
    pub fn text_width(&self, text: &str, px: f32) -> u32 {
        self.measure_items(text, px)
            .iter()
            .map(|it| it.2)
            .sum::<f32>()
            .ceil() as u32
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

    /// draw_text_left 全参版（四版配置页用）：显式内缩 + 纵裁剪带
    /// （池内滚动内容出池内缘即断墨——框/文字同一裁剪带，眼手同尺）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_text_left_ex(
        &self,
        frame: &mut Frame<'_>,
        text: &str,
        cx: u32,
        cw: u32,
        cy: u32,
        rh: u32,
        px: f32,
        fg: u32,
        inset: f32,
        clip_y: Option<(i32, i32)>,
    ) {
        let items = self.measure_items(text, px);
        self.draw_items_left_inset(frame, &items, cx, cw, cy, rh, px, fg, clip_y, inset);
    }

    /// 居中画一行文字（BAR-046 选择菜单按钮标签，2026-09-03）：水平居中
    /// 于 (cx,cw)，垂直居中于 (cy,ch)，右缘裁剪 + clip_x0 左缘裁剪
    /// （2026-09-12 标签栏横滚滑出内容带左缘时不许污染页环带——坐标
    /// 改 i64 收负 x；菜单老调用方传 cx 当 clip_x0 = 行为不变）。
    /// 与 draw_items_left 同光栅化路径，只是起笔 = 格心 - 文本半宽、
    /// 无 18 内缩。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_text_centered(
        &self,
        frame: &mut Frame<'_>,
        text: &str,
        cx: i64,
        cy: i64,
        cw: u32,
        ch: u32,
        px: f32,
        fg: u32,
        clip_x0: i64,
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
        let clip_right = cx + cw as i64;
        let baseline = cy as f32 + (ch as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent;
        for (f, c, adv) in items {
            if pen_x + adv >= clip_right as f32 {
                break; // 格内装不下就停（与 draw_items_left 同判据）
            }
            let g = self.rasterize_cached(f, c, px); // BAR-102：缓存光栅
            let (m, bmp) = (&g.0, &g.1);
            let top = baseline - m.ymin as f32 - m.height as f32;
            for gy in 0..m.height as u32 {
                let y = top as i64 + i64::from(gy);
                if y < 0 || y >= i64::from(frame.h) {
                    continue;
                }
                for gx in 0..m.width as u32 {
                    let x = (pen_x + m.xmin as f32) as i64 + i64::from(gx);
                    if x < clip_x0 || x >= clip_right {
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

    /// 标签栏**层**涂装（BAR-096 拆槽；原整页版退役）：层画布 =
    /// 屏宽 × 标签区高（TAB_LAYER_H），y 原点 = 标签行顶
    /// （content_origin().1）——内容与整页版逐像素等价（仅 y 平移一个
    /// 常量），**游标滑行只脏这一层**（0.65MB vs 配置槽 14MB ≈ 省 22 倍，
    /// 帧饥饿根治件一）。恒靠泊位（cfg_off 在合成期 placement）。
    ///
    /// 主题宪法 §四，2026-09-13 八修换案、十一修随机色体系）：**填色
    /// 标签页组件**（paint_tab_chip：无边框色块、上两角圆角下缘直边；
    /// **每标签独立双色吃快照色列**——选中 = 上 2/3 c1 + 下 1/3 c2 满填
    /// 两截短渐变 + 文字换深色反差，未选中 = 上/下 1/3 条带薄态 + 中
    /// 1/3 6% 白底 + 文字 0.5 白）+ 标签行下缘紧挨一条**池区同宽的
    /// 1px 细线**（§四 底线组件两模式之「配合标签」：整根 = 选中标签
    /// c2 纯色 α255；色列空 = 兜底 accent.c2）。
    /// 选中块 x 吃快照 cursor_x（弹簧滑块；文字不随弹簧，各就各位）。
    /// 空态也画底线：装修不是内容（与页环同规）
    pub(crate) fn paint_tab_bar_layer(
        &self,
        buf: &mut [u32],
        cw: u32,
        ch: u32,
        y_shift: i64,
        snap: &crate::ui::tab_bar::TabBarSnap,
        accent: crate::ui::accent::AccentPair,
    ) {
        if cw == 0 || ch == 0 {
            return;
        }
        let mut frame = Frame { buf, w: cw, h: ch };
        let (ox, oy_abs) = crate::ui::tab_bar::content_origin();
        // y_shift = 绘制缓冲的 y 原点相对标签行顶的位置：GLES 层缓冲从
        // 标签行顶起算（传内容原点 55），softbuffer 兜底整页缓冲传 0
        let oy = y_shift;
        // 内容带（x 向）——层恒靠泊，off=0；带外的东西用户看不见也点不着
        let clip_l = i64::from(ox);
        let clip_r = i64::from(cw) - i64::from(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_W);
        let rects = crate::ui::tab_bar::rects_of(&snap.tabs, snap.scroll_px);
        // 标签块先画（装修在文字之下）：选中块随弹簧 x，未选中各就各位；
        // 色源 = 快照色列该标签自己的双色（缺位兜底页 accent，§四 十一修）
        for (i, r) in rects.iter().enumerate() {
            let cx = if i == snap.selected {
                snap.cursor_x as i64
            } else {
                r.x
            };
            let pair = snap.colors.get(i).copied().unwrap_or(accent);
            paint_tab_chip(
                &mut frame,
                cx,
                r.y - oy,
                r.w,
                r.h,
                i == snap.selected,
                pair,
                clip_l,
                clip_r,
            );
        }
        // 底线：1px 细线，池区同宽，紧挨标签行下缘（空态也画）——
        // 配合标签模式：整根 = 选中标签 c2 纯色 α255（色列空 = accent.c2）。
        // 层宽 = 屏宽，池区几何仍需屏高—底账（bottom_inset 由壳层喂）
        let line_c = snap
            .colors
            .get(snap.selected)
            .map(|p| p.c2)
            .unwrap_or(accent.c2);
        // 底线世界坐标 = 内容原点 y + 标签行高；绘制 y 再减 y_shift
        // （层版：55+110−55 = 110；整页版：165−0 = 165——两版等价）
        let uy = i64::from(oy_abs) + i64::from(crate::ui::tab_bar::TAB_ROW_H) - y_shift;
        if uy < i64::from(ch) {
            // 底线 x 起止吃池区左右内缘（层坐标 = 线坐标 − oy 无关 x）
            let pa = snap.line_span.unwrap_or((clip_l, clip_r)); // 壳层喂池区带；缺省=内容带
            for ax in pa.0..pa.1 {
                if ax < 0 || ax >= i64::from(cw) || ax < clip_l || ax >= clip_r {
                    continue;
                }
                frame.blend_px(ax as u32, uy as u32, line_c, 255);
            }
        }
        // 文字：选中块上深色（浅底反差），未选中 0.5 白；文字不随弹簧
        for (i, r) in rects.iter().enumerate() {
            let fg = if i == snap.selected {
                crate::ui::accent::CARD_PAGE_BG
            } else {
                0x0080_8080
            };
            self.draw_text_centered(
                &mut frame,
                &snap.tabs[i],
                r.x,
                r.y - oy,
                r.w,
                r.h,
                TAB_TEXT_PX,
                fg,
                clip_l,
            );
        }
    }

    /// 下池光标**层**涂装（BAR-096 拆槽，帧饥饿根治件二）：层画布 =
    /// 池内容宽 × 下池行高，内容 = 选中全包框满态铺满整层——
    /// **框形状与行内容无关**，故层内容只随 accent/宽变（一次烘焙），
    /// 位置（行号小数 → 像素 y）全部进合成期 placement：滑行逐帧零重烘
    /// （原配置槽每帧 14MB 重光栅+上传）。
    /// 渐变保真：grad_ref 吃「框在页上原位」的页坐标与页分母 → 与整页内
    /// 绘制逐像素同色（BAR-096 保真条；颜色不随层画布尺漂移）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_lower_cursor_layer(
        &self,
        buf: &mut [u32],
        cw: u32,
        ch: u32,
        page_x: i64,
        page_y: i64,
        page_denom: i64,
        accent: crate::ui::accent::AccentPair,
        row: Option<&crate::ui::cfg_page::RowView>,
    ) {
        if cw < 4 || ch < 4 {
            return;
        }
        let mut frame = Frame { buf, w: cw, h: ch };
        paint_row_frame_gradref(
            &mut frame,
            0,
            0,
            cw,
            ch,
            true,
            accent,
            (0, i64::from(ch)),
            (page_x, page_y, page_denom),
        );
        // BAR-096 拆层修（回归 BAR-089 的历史修法）：**选中行文字必须画在
        // 框芯之上**——框芯是不透明渐变暗底（mark_chrome_alpha：非纯黑即
        // 不透明），层画在配置槽之上 → 不在此补文字，accent 亮时选中行文字
        // 被框芯盖没（用户真机实录「框把文字盖住」；redroid 那次看着正常
        // 只因 accent 暗、框芯恰为纯黑被判透明）。尺与 paint_pool_lower 同源
        // （title_band 90 / title 36 / meta 30 / 内缩 27 / faux-bold 双画）
        if let Some(row) = row {
            let title_fg = 0x00D9_D9D9;
            let meta_fg = 0x0080_8080;
            let title_band = 90u32;
            let text_inset = 27.0;
            self.draw_text_left_ex(
                &mut frame, &row.title, 0, cw, 0, title_band, 36.0, title_fg, text_inset, None,
            );
            self.draw_text_left_ex(
                &mut frame, &row.title, 1, cw, 0, title_band, 36.0, title_fg, text_inset, None,
            );
            if !row.meta.is_empty() && ch > title_band {
                self.draw_text_left_ex(
                    &mut frame,
                    &row.meta,
                    0,
                    cw,
                    title_band,
                    ch - title_band,
                    30.0,
                    meta_fg,
                    text_inset,
                    None,
                );
            }
        }
    }

    /// 双池涂装（宪法 §五）：上池/下池两枚二级卡片框，paint_rect_ring
    /// 同配方；内卡渐变反转 c2→c1（§三 多级嵌套逐层反转，外壳页环是
    /// c1→c2）。骨架期无内容——框即全部（空池也画：A2 占位条款）
    pub(crate) fn paint_cfg_dual_pool_impl(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        snap: &crate::ui::dual_pool::DualPoolSnap,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let mut frame = Frame { buf, w, h };
        let off = i64::from(cfg_off_x);
        let (ox, _oy) = crate::ui::tab_bar::content_origin();
        // 内容裁剪带与标签栏同源（眼手同尺：带外看不见也点不着）
        let clip_l = i64::from(ox) + off;
        let clip_r =
            i64::from(w) - i64::from(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_W) + off;
        for r in [&snap.upper, &snap.lower] {
            if r.w < 2 || r.h < 2 {
                continue;
            }
            let x0 = r.x + off;
            let y0 = r.y;
            paint_rect_ring(
                &mut frame,
                x0,
                y0,
                x0 + i64::from(r.w),
                y0 + i64::from(r.h),
                clip_l,
                clip_r,
                crate::ui::accent::CARD_PAGE_BG,
                accent.c2,
                accent.c1,
                POOL_FRAME_R,
                true,
            );
        }
    }

    /// 双池内容涂装（宪法 §五 目录语义四版，2026-09-13 修宪落地）：
    /// 下池 = 子目录行表（4.5 格左粗三边细框行，选中 = 三边 accent 渐变
    /// 描边 3px）；上池 = 字段框行表（4 格，标签列 + 值框；首行 = 下拉
    /// 行，值框 + 右缘下拉三角）；下拉开着 = 触发器下方下弹 panel
    /// （§六：96% 近黑底，选中项 accent 描边——✓ 标记留 v1b，内嵌像素
    /// 体字库无此 glyph 待验）。**上池像素滚动**（§五 滚动条款兑现）：
    /// 行矩形吃 snap.upper_scroll，出池内缘裁剪带断墨（框/文字/三角
    /// 同一带）；off≠0 的过渡帧里行左缘出屏即整行不画（softbuffer/
    /// 值守兜底路径的取舍，GLES 主路径烘焙恒 off=0 不受影响）。
    /// 十七修 §六「面与内容一体」：page.pan = Some 时双代同画——
    /// Page 域 = 双池框+内容整体平移（本函数自带框，调用方须跳过
    /// paint_cfg_dual_pool）；Upper 域 = 仅上池内容平移（框/下池
    /// 照常）。十八修 §七：平移距 = 视口宽 + 留隙 G（内容轴隐藏
    /// 布局「旧代 | 隙 | 新代」），Upper 域裁剪带 = 上池内容矩形
    /// （池框一像素不进带），曲线 ease-in-out cubic
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_cfg_pool_content_impl(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ps: &crate::ui::dual_pool::DualPoolSnap,
        page: &crate::ui::cfg_page::CfgPageSnap,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
        now_ms: u64,
        pan_upper_hold: bool,
        // BAR-096 拆层：true = 选中全包框不由本画布画（改由下池光标层
        // 合成期 placement——GLES 路径）；softbuffer 兜底传 false
        skip_cursor: bool,
    ) {
        use crate::ui::cfg_page as cp;
        if w == 0 || h == 0 {
            return;
        }
        let mut frame = Frame { buf, w, h };
        let off = i64::from(cfg_off_x);

        match page.pan.as_ref().map(|p| p.scope) {
            Some(cp::PanScope::Page) => {
                let pan = page.pan.as_ref().unwrap();
                // 页面级：旧代（冻结快照，含双池框/旧页色）带偏移出，
                // 新代（活态）带偏移进；视口 = 页内容裁剪带（页环不动）。
                // 十八修 §七留隙律：平移距 = 视口宽 + G（PAN_GAP_PAGE）——
                // 内容轴上恒为「旧代 | 隙 G | 新代」隐藏布局，双代缘距
                // 全程 = G（贴挤 = 元素替换读感，非视口平移）
                let pw = ps.upper.w as i64;
                let travel = pw + cp::PAN_GAP_PAGE;
                let (d_old, d_new) = cp::pan_offsets(pan.dir, pan.t, travel);
                let band = page_pan_band(
                    w,
                    off,
                    pan.old.pool.upper.y,
                    ps.upper.y,
                    pan.old.pool.lower.y + pan.old.pool.lower.h as i64,
                    ps.lower.y + ps.lower.h as i64,
                );
                pan_temps(w, h, |a, b| {
                    copy_frame(frame.buf, a);
                    copy_frame(frame.buf, b);
                    {
                        let mut fa = Frame { buf: a, w, h };
                        self.paint_pool_frames(&mut fa, &pan.old.pool, pan.old.accent, off);
                        self.paint_pool_lower(
                            &mut fa,
                            &pan.old.pool.lower,
                            &pan.old.rows,
                            pan.old.cursor_row,
                            pan.old.accent,
                            off,
                            skip_cursor,
                        );
                        self.paint_pool_upper(
                            &mut fa,
                            &pan.old.pool.upper,
                            &pan.old.upper,
                            pan.old.upper_scroll,
                            pan.old.accent,
                            off,
                            0.0,
                        );
                    }
                    {
                        let mut fb = Frame { buf: b, w, h };
                        self.paint_pool_frames(&mut fb, ps, accent, off);
                        self.paint_pool_lower(
                            &mut fb,
                            &ps.lower,
                            &page.rows,
                            page.cursor_row,
                            accent,
                            off,
                            skip_cursor,
                        );
                        self.paint_pool_upper(
                            &mut fb,
                            &ps.upper,
                            &page.upper,
                            page.upper_scroll,
                            accent,
                            off,
                            page.dropdown_progress,
                        );
                    }
                    blit_shift(&mut frame, a, d_old, band);
                    blit_shift(&mut frame, b, d_new, band);
                });
            }
            Some(cp::PanScope::Upper) => {
                let pan = page.pan.as_ref().unwrap();
                // 上池级：双池框/下池照常（当前态），仅上池内容双代平移。
                // 十八修 §七：①裁剪带 = 上池**内容矩形**（POOL_CONTENT_
                // INSET 内缩）——池框/左粗竖条一像素不进带（旧带左缘
                // x+4 吞了 9px 粗条右半 = 框随内容滑，用户实机判「视觉
                // 断裂」）；②留隙律同 Page 域（G = PAN_GAP_UPPER）
                self.paint_pool_lower(
                    &mut frame,
                    &ps.lower,
                    &page.rows,
                    page.cursor_row,
                    accent,
                    off,
                    skip_cursor,
                );
                let band = upper_pan_band(&ps.upper, off);
                let travel = (band.2 - band.0) + cp::PAN_GAP_UPPER;
                let (d_old, d_new) = cp::pan_offsets(pan.dir, pan.t, travel);
                pan_temps(w, h, |a, b| {
                    copy_frame(frame.buf, a);
                    copy_frame(frame.buf, b);
                    {
                        let mut fa = Frame { buf: a, w, h };
                        self.paint_pool_upper(
                            &mut fa,
                            &pan.old.pool.upper,
                            &pan.old.upper,
                            pan.old.upper_scroll,
                            pan.old.accent,
                            off,
                            0.0,
                        );
                    }
                    {
                        let mut fb = Frame { buf: b, w, h };
                        self.paint_pool_upper(
                            &mut fb,
                            &ps.upper,
                            &page.upper,
                            page.upper_scroll,
                            accent,
                            off,
                            page.dropdown_progress,
                        );
                    }
                    blit_shift(&mut frame, a, d_old, band);
                    blit_shift(&mut frame, b, d_new, band);
                });
            }
            None => {
                self.paint_pool_lower(
                    &mut frame,
                    &ps.lower,
                    &page.rows,
                    page.cursor_row,
                    accent,
                    off,
                    skip_cursor,
                );
                // 十九修 D8：Upper 平移 hold 期上池行不进主画布——带内
                // 静物=池内芯渐变（vc425 涂装域同语义：行只活在双代里，
                // 隙底透出的是内芯不是定格行）
                if !pan_upper_hold {
                    self.paint_pool_upper(
                        &mut frame,
                        &ps.upper,
                        &page.upper,
                        page.upper_scroll,
                        accent,
                        off,
                        page.dropdown_progress,
                    );
                }
            }
        }

        self.paint_dropdown_panel(&mut frame, ps, page, accent, off);

        // ---- 跳框（宪法 §六 跳框条款，九修）：模态盖在配置页最上层——
        // 压暗层 + 居中卡 + 关闭钮。本体在 paint_modal_impl（§六 样式
        // 唯一来源：压暗层/字段排版之外的涂装原语全复用共享件）
        if let Some(mi) = page.modal {
            self.paint_modal_impl(frame.buf, w, h, mi, cfg_off_x, accent, now_ms);
        }
    }

    /// 双池框涂装（十七修从 paint_cfg_dual_pool_impl 抽出的 Frame 版——
    /// Page 域平移时双池框随内容进 temp 双代同画；内卡反转 c2→c1）
    fn paint_pool_frames(
        &self,
        frame: &mut Frame<'_>,
        ps: &crate::ui::dual_pool::DualPoolSnap,
        accent: crate::ui::accent::AccentPair,
        off: i64,
    ) {
        let (ox, _oy) = crate::ui::tab_bar::content_origin();
        let clip_l = i64::from(ox) + off;
        let clip_r =
            i64::from(frame.w) - i64::from(AI_PAGE_FRAME_MARGIN + AI_PAGE_FRAME_W + CELL_W) + off;
        for r in [&ps.upper, &ps.lower] {
            if r.w < 2 || r.h < 2 {
                continue;
            }
            let x0 = r.x + off;
            let y0 = r.y;
            paint_rect_ring(
                frame,
                x0,
                y0,
                x0 + i64::from(r.w),
                y0 + i64::from(r.h),
                clip_l,
                clip_r,
                crate::ui::accent::CARD_PAGE_BG,
                accent.c2,
                accent.c1,
                POOL_FRAME_R,
                true,
            );
        }
    }

    /// 下池块（十七修抽出）：行循环（未选中态）→ 光标选中框 → 文字三遍
    /// 顺序不动（BAR-089 先框后字钉保持）
    #[allow(clippy::too_many_arguments)]
    fn paint_pool_lower(
        &self,
        frame: &mut Frame<'_>,
        lower: &crate::ui::dual_pool::PoolRect,
        rows: &[crate::ui::cfg_page::RowView],
        cursor_row: f32,
        accent: crate::ui::accent::AccentPair,
        off: i64,
        // BAR-096 拆层：true = 选中全包框不由本画布画（下池光标层承担）
        skip_cursor: bool,
    ) {
        use crate::ui::cfg_page as cp;
        let title_fg = 0x00D9_D9D9; // 0.85 白（§2.3 标题档）
        let meta_fg = 0x0080_8080; // 0.5 白（次级档）
        let px_title = 36.0;
        let px_meta = 30.0;
        let text_inset = 27.0;
        let denom = ((frame.w - 1) + (frame.h - 1)).max(1) as i64;
        let no_clip = (0, i64::from(frame.h));

        // ---- 下池：子目录行表（4.5 格框行，左粗条恒在；选中框单独滑行）----
        // 十五修 §五：行本体恒按未选中画——选中全包框在循环后按光标弹簧
        // 瞬时值（行号小数）单独落墨，内容与页色即时切换不等光标。
        // 2026-09-14 实踩修：选中框内芯不透明渐变暗底，文字必须最后画——
        // 「行循环画字→选中框后画」= 选中行文字被框芯盖没（用户实机抓）
        for i in 0..rows.len() {
            let r = cp::lower_row_rect(i, lower);
            if r.y + r.h as i64 > lower.y + lower.h as i64 {
                break; // 出池底的行不画（下池宪法不滚动——内容少恒撑满）
            }
            let rx = r.x + off;
            if rx < 0 {
                continue; // 过渡帧行左缘出屏整行不画
            }
            paint_row_frame(frame, rx, r.y, r.w, r.h, false, accent, denom, no_clip);
        }

        // 十五修 §五：选中全包框吃光标缓动瞬时值（行号小数 → 像素）——
        // 与 lower_row_rect 同一份几何（内缩/步进同源），只是 y 吃滑行值。
        // BAR-096 拆层：GLES 路径（skip_cursor）不在此画——下池光标层
        // 合成期 placement 承担，滑行不再脏本画布
        if !rows.is_empty() && !skip_cursor {
            let stride = cp::LOWER_ROW_H as i64 + cp::ROW_GAP;
            let cy = lower.y + cp::POOL_CONTENT_INSET + (cursor_row * stride as f32).round() as i64;
            let crx = lower.x + cp::POOL_CONTENT_INSET + off;
            if crx >= 0 && cy + cp::LOWER_ROW_H as i64 <= lower.y + lower.h as i64 {
                paint_row_frame(
                    frame,
                    crx,
                    cy,
                    lower.w.saturating_sub((cp::POOL_CONTENT_INSET * 2) as u32),
                    cp::LOWER_ROW_H,
                    true,
                    accent,
                    denom,
                    no_clip,
                );
            }
        }

        // 文字最后一遍（盖过选中框内芯）：标题 faux-bold（像素体无粗体
        // 档：双画偏 1px）+ meta，行内垂直分布
        for (i, row) in rows.iter().enumerate() {
            let r = cp::lower_row_rect(i, lower);
            if r.y + r.h as i64 > lower.y + lower.h as i64 {
                break;
            }
            let rx = r.x + off;
            if rx < 0 {
                continue;
            }
            let (rx, ry) = (rx as u32, r.y as u32);
            let title_band = 90; // 四版 ×1.5（60 → 90）
            self.draw_text_left_ex(
                frame, &row.title, rx, r.w, ry, title_band, px_title, title_fg, text_inset, None,
            );
            self.draw_text_left_ex(
                frame,
                &row.title,
                rx + 1,
                r.w,
                ry,
                title_band,
                px_title,
                title_fg,
                text_inset,
                None,
            );
            if !row.meta.is_empty() {
                self.draw_text_left_ex(
                    frame,
                    &row.meta,
                    rx,
                    r.w,
                    ry + title_band,
                    r.h - title_band,
                    px_meta,
                    meta_fg,
                    text_inset,
                    None,
                );
            }
        }
    }

    /// 上池块（十七修抽出）：字段框行表（标签列 + 值框；首行 = 下拉行）。
    /// dd_progress = 下拉开合进度——▼ 三角旋转角（十七修 §六④：展开
    /// 180° 旋到 ▲，两段时序Ⅰ段冻结在 180°）
    #[allow(clippy::too_many_arguments)]
    fn paint_pool_upper(
        &self,
        frame: &mut Frame<'_>,
        upper: &crate::ui::dual_pool::PoolRect,
        rows: &[crate::ui::cfg_page::UpperRow],
        scroll: i64,
        accent: crate::ui::accent::AccentPair,
        off: i64,
        dd_progress: f32,
    ) {
        use crate::ui::cfg_page as cp;
        let title_fg = 0x00D9_D9D9;
        let meta_fg = 0x0080_8080;
        let px_title = 36.0;
        let px_meta = 30.0;
        let denom = ((frame.w - 1) + (frame.h - 1)).max(1) as i64;

        // ---- 上池：字段框行表（标签列 + 值框；首行 = 下拉行）----
        // 四版滚动：行矩形吃 upper_scroll；出池内缘裁剪带断墨
        // （框/文字/下拉三角同一带），完全滚出池顶的行跳过
        let uclip = (upper.y + 12, upper.y + upper.h as i64 - 12);
        for (i, ur) in rows.iter().enumerate() {
            let r = cp::upper_row_rect(i, upper, scroll);
            if r.y + r.h as i64 <= uclip.0 {
                continue; // 完全滚出池顶
            }
            if r.y >= uclip.1 {
                break; // 出池底（行有序，后续更靠下）
            }
            let rx = r.x + off;
            if rx < 0 {
                continue;
            }
            let clip32 = Some((uclip.0 as i32, uclip.1 as i32));
            // 十四修动态宽度：实量宽喂几何（与触摸命中同一条
            // measure_items 尺——眼手同尺）；标签块锚左、值框锚右
            let l_items = self.measure_items(&ur.label, px_title);
            let v_items = self.measure_items(&ur.value, px_meta);
            let lw = l_items.iter().map(|it| it.2).sum::<f32>().ceil() as u32;
            let vw = v_items.iter().map(|it| it.2).sum::<f32>().ceil() as u32;
            let lb = cp::field_label_rect(&r, lw);
            let vb = cp::field_value_rect(&r, lb.w, vw, ur.is_dropdown);
            // 标签列背衬（十三修 §五）：圆角 36 无边框块（与值框同高
            // 对齐）= 渐变暗底不透明直写 + 8% 白提亮（blend 白 α20）——
            // 去框后标签列背景辨识度靠这一层；十四修：宽随标签文字动态
            {
                let lw2 = lb.w;
                let lr = (POOL_FRAME_R as i64).min((lw2 / 2).min(vb.h / 2) as i64) as u32;
                let fw = i64::from(frame.w);
                // BAR-103：背衬色只是 s=xx+yy 的一元函数，LUT + 行切片
                let bg_lut: Vec<u32> = (0..=denom)
                    .map(|s| frame_bg_rgb(accent.c1, accent.c2, s, 0, denom))
                    .collect();
                for dy in 0..vb.h as i64 {
                    let yy = vb.y + dy;
                    if yy < 0 || yy >= i64::from(frame.h) || yy < uclip.0 || yy >= uclip.1 {
                        continue;
                    }
                    let x_start = rx.max(0);
                    let x_end = (rx + i64::from(lw2)).min(fw);
                    if x_end <= x_start {
                        continue;
                    }
                    let row_base = yy as usize * fw as usize;
                    for xx in x_start..x_end {
                        let dx = xx - rx;
                        if rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, lw2, vb.h, lr) >= 0.0 {
                            continue;
                        }
                        let s = (xx + yy).clamp(0, denom) as usize;
                        frame.buf[row_base + xx as usize] = bg_lut[s];
                        frame.blend_px(xx as u32, yy as u32, 0x00FF_FFFF, 20);
                    }
                }
            }
            // 标签文字（title 档 36px 亮——六修字档反转（色）+ 七修补齐
            // （号）；十四修：逐行左对齐，超长贪心换行 ≤2 行）
            self.draw_field_lines(
                frame,
                &l_items,
                rx as u32,
                lb.w,
                vb.y as u32,
                vb.h,
                px_title,
                title_fg,
                clip32,
                true,
            );
            // 值框（十二修无边框化：渐变暗底圆角块、零框墨——十一修
            // 形态②「只有左竖线」实机判丑退役；十三修：下拉行值框 =
            // 三级框全包框——触发器是选择控件）+ 值文本
            paint_row_frame(
                frame,
                vb.x + off,
                vb.y,
                vb.w,
                vb.h,
                ur.is_dropdown,
                accent,
                denom,
                uclip,
            );
            // 值文本逐行右对齐（十四修）；右缘呼吸位 = 文内边距 1.5 格，
            // 下拉行再让 ▼ 三角位 45px（kfmv4 实证：文字不贴框缘）；
            // 字档反转：值 = meta 档 30px 灰
            let tri_pad = if ur.is_dropdown {
                cp::FIELD_TRIANGLE_PAD
            } else {
                0
            };
            self.draw_field_lines(
                frame,
                &v_items,
                (vb.x + off) as u32,
                vb.w.saturating_sub(tri_pad),
                vb.y as u32,
                vb.h,
                px_meta,
                meta_fg,
                clip32,
                false,
            );
            if ur.is_dropdown {
                // 右缘下拉三角（十七修 §六④：矢量三角随开合进度旋转——
                // θ = progress×180°，关 ▼ → 开 ▲；基三角 17×9 同尺，
                // 逐像素逆旋转采样，uclip 同带）
                let cx = (vb.x + off) as f32 + vb.w as f32 - 37.0;
                let cy = vb.y as f32 + vb.h as f32 / 2.0;
                let theta = dd_progress * std::f32::consts::PI;
                let (sn, cs) = theta.sin_cos();
                for py in -12i32..=12 {
                    for px in -12i32..=12 {
                        let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                        let lx = fx * cs + fy * sn;
                        let ly = -fx * sn + fy * cs;
                        let inside =
                            (-4.5..=4.5).contains(&ly) && lx.abs() <= 8.5 * (4.5 - ly) / 9.0;
                        if !inside {
                            continue;
                        }
                        let xx = cx as i64 + i64::from(px);
                        let yy = cy as i64 + i64::from(py);
                        if xx < 0
                            || xx >= i64::from(frame.w)
                            || yy < uclip.0
                            || yy >= uclip.1
                            || yy < 0
                            || yy >= i64::from(frame.h)
                        {
                            continue;
                        }
                        frame.blend_px(xx as u32, yy as u32, 0x00D9_D9D9, 255);
                    }
                }
            }
        }
    }

    /// 下拉 panel 块（十七修抽出；§六）：整面圆角无边框深底（近黑
    /// α252）+ 选中行均匀细框。**抽屉随面**（十七修 §六③）：选项行与
    /// 选中细框是钉在面板全高刚体上的——y 偏移 = 当前高−全高，展开
    /// = 刚体从触发器后滑出（下方选项先入场），收起 = 整体滑回（上方
    /// 先没入）；帘幕式（内容钉顶只裁底）退役。BAR-090：面板宽 =
    /// max(触发器宽, 选项最长文+双侧边距)，右缘钳上池内容内缘
    fn paint_dropdown_panel(
        &self,
        frame: &mut Frame<'_>,
        ps: &crate::ui::dual_pool::DualPoolSnap,
        page: &crate::ui::cfg_page::CfgPageSnap,
        accent: crate::ui::accent::AccentPair,
        off: i64,
    ) {
        use crate::ui::cfg_page as cp;
        if page.dropdown_progress <= 0.001 {
            return;
        }
        let title_fg = 0x00D9_D9D9;
        let px_title = 36.0;
        let px_meta = 30.0;
        let text_inset = 27.0;
        let denom = ((frame.w - 1) + (frame.h - 1)).max(1) as i64;
        let scroll = page.upper_scroll;
        // 十四修：触发器几何吃首行实量宽（与字段行涂装同尺）
        let (lw, vw) = match page.upper.first() {
            Some(ur) => (
                self.text_width(&ur.label, px_title),
                self.text_width(&ur.value, px_meta),
            ),
            None => (0, 0),
        };
        let dd = page.upper.first().is_none_or(|ur| ur.is_dropdown);
        let t = cp::trigger_rect(&ps.upper, scroll, dd, lw, vw);
        let max_h = frame.h.saturating_sub(t.y.max(0) as u32 + t.h + 40);
        // BAR-090：选项最长文实量宽 + 双侧文内边距 = 内容最小宽
        let cw = page
            .options
            .iter()
            .map(|o| self.text_width(o, px_title))
            .max()
            .unwrap_or(0)
            + cp::FIELD_TEXT_INSET * 2;
        let pr =
            cp::dropdown_panel_rect(page.options.len(), &ps.upper, max_h, scroll, dd, lw, vw, cw);
        let full_h = pr.h; // 全高（progress=1 的高）——抽屉刚体尺
        let pr = crate::ui::dual_pool::PoolRect {
            h: ((pr.h as f32) * page.dropdown_progress).round() as u32,
            ..pr
        };
        // 抽屉随面：刚体（全高坐标系）随面板当前高上移——展开时下方
        // 选项先露，收起时上方先没入面板顶缘
        let drawer_dy = pr.h as i64 - full_h as i64;
        let panel_clip = (pr.y, pr.y + pr.h as i64);
        let px0 = pr.x + off;
        if px0 < 0 {
            return;
        }
        let prr = (POOL_FRAME_R as i64).min((pr.w / 2).min(pr.h / 2) as i64) as u32;
        let (fw, fh) = (i64::from(frame.w), i64::from(frame.h));
        for dy in 0..pr.h as i64 {
            let yy = pr.y + dy;
            if yy < 0 || yy >= fh {
                continue;
            }
            for dx in 0..pr.w as i64 {
                let xx = px0 + dx;
                if xx < 0 || xx >= fw {
                    continue;
                }
                if rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, pr.w, pr.h, prr) >= 0.0 {
                    continue;
                }
                frame.blend_px(xx as u32, yy as u32, 0, 252);
            }
        }
        // 选中行均匀细框（两段时序 2026-09-14：吃 option_sel_f
        // 滑行瞬时值——Ⅰ段可见滑动，滑到才收面板）；随面板
        // 当前高裁剪（Ⅱ段收起中行出底即断墨）。**先框后字**——
        // 细框内芯不透明渐变暗底，后画会盖没选项文字（下池
        // 选中行同款实踩）
        let sel_y = pr.y + (page.option_sel_f * cp::FIELD_ROW_H as f32).round() as i64 + drawer_dy;
        if sel_y >= pr.y && sel_y + cp::FIELD_ROW_H as i64 <= pr.y + pr.h as i64 {
            paint_thin_frame(
                frame,
                px0,
                sel_y,
                pr.w,
                cp::FIELD_ROW_H,
                accent,
                denom,
                panel_clip,
            );
        }
        let clip32 = Some((panel_clip.0 as i32, panel_clip.1 as i32));
        for (i, opt) in page.options.iter().enumerate() {
            let iy = pr.y + (i as i64) * cp::FIELD_ROW_H as i64 + drawer_dy;
            if iy + cp::FIELD_ROW_H as i64 <= pr.y {
                continue; // 整体没入面板顶缘之上（收起中上方先没）
            }
            if iy >= pr.y + pr.h as i64 {
                break; // 出面板底（行有序，后续更靠下）
            }
            if iy < 0 {
                continue;
            }
            self.draw_text_left_ex(
                frame,
                opt,
                px0 as u32,
                pr.w.saturating_sub(27),
                iy as u32,
                cp::FIELD_ROW_H,
                px_title,
                title_fg,
                text_inset,
                clip32,
            );
        }
    }

    /// 跳框涂装（宪法 §六 跳框条款，2026-09-13 九修新立；kfmv4 跳框三
    /// 截图实证）：压暗层 α150 盖本页区（随面板平移）+ 居中卡
    /// （paint_rect_ring 池框同尺 R36、内卡反转 c2→c1）+ 标题 36px
    /// faux-bold 亮居中 + 1px 渐变分隔线（c2→c1，池内容同一把 135°
    /// 尺）+ 字段区（题注 30px 灰在上、内容 36px 亮在下——有意反向，
    /// 题注是内容的脚注）+ 底部全宽「关闭」钮（paint_thin_frame 均匀
    /// 细框——非池行场合，十一修起不属三级框）。几何/折行全吃 ui/modal.rs（眼手同尺）；
    /// 卡高封顶截断：画出关闭钮上缘的内容断墨（v1 不滚动）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_modal_impl(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        mi: usize,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
        now_ms: u64,
    ) {
        use crate::ui::modal as md;
        if w == 0 || h == 0 {
            return;
        }
        let comps = crate::ui::comp_registry::COMPONENTS;
        if comps.is_empty() {
            return;
        }
        let entry = &comps[mi.min(comps.len() - 1)];
        let mut frame = Frame { buf, w, h };
        let off = i64::from(cfg_off_x);
        let title_fg = 0x00D9_D9D9; // 0.85 亮
        let meta_fg = 0x0080_8080; // 0.5 灰
        let denom = ((w - 1) + (h - 1)).max(1) as i64; // 池内容同一把渐变尺

        // 压暗层：本页可见区整层混黑 α150（随面板平移，左右求交）
        let dx0 = off.clamp(0, i64::from(w)) as u32;
        let dx1 = (i64::from(w) + off).clamp(0, i64::from(w)) as u32;
        for yy in 0..h {
            for xx in dx0..dx1 {
                frame.blend_px(xx, yy, 0x0000_0000, 150);
            }
        }

        // 居中卡（几何 = modal.rs；折行吃 content_cells 同尺）
        let fields = md::fields_of(entry, md::content_cells(w));
        let card = md::card_rect(w, h, &fields);
        let cx0 = card.x + off;
        if cx0 < 0 {
            return; // 过渡帧卡左缘出屏整卡不画（与池行同规取舍）
        }
        let cx1 = cx0 + i64::from(card.w);
        paint_rect_ring(
            &mut frame,
            cx0,
            card.y,
            cx1,
            card.y + i64::from(card.h),
            0,
            i64::MAX,
            crate::ui::accent::CARD_PAGE_BG,
            accent.c2,
            accent.c1,
            POOL_FRAME_R,
            true,
        );

        // 内容断墨线：关闭钮上缘（含钮前隙）——超出的内容不画
        let btn = md::close_btn_rect(&card);
        let ink_bottom = btn.y - i64::from(md::MODAL_FIELD_GAP);
        let text_x = (cx0 + md::MODAL_PAD_X) as u32;
        let text_w = (card.w as i64 - md::MODAL_PAD_X * 2).max(0) as u32;

        // 标题（2 格带居中，faux-bold 双画偏 1px）
        let title_y = card.y + i64::from(md::MODAL_PAD_Y);
        self.draw_text_centered(
            &mut frame,
            entry.name,
            cx0,
            title_y,
            card.w,
            md::MODAL_TITLE_H,
            36.0,
            title_fg,
            cx0,
        );
        self.draw_text_centered(
            &mut frame,
            entry.name,
            cx0 + 1,
            title_y,
            card.w,
            md::MODAL_TITLE_H,
            36.0,
            title_fg,
            cx0,
        );

        // 分隔线：标题下 0.5 格处 1px 渐变细线，卡内宽，c2→c1
        let line_y = title_y + i64::from(md::MODAL_TITLE_H) + i64::from(md::MODAL_FIELD_GAP);
        if line_y >= 0 && line_y < i64::from(h) {
            for ax in (cx0 + md::MODAL_PAD_X)..(cx1 - md::MODAL_PAD_X) {
                if ax < 0 || ax >= i64::from(w) {
                    continue;
                }
                let c = ring_gradient_rgb(accent.c2, accent.c1, ax, line_y, denom);
                frame.blend_px(ax as u32, line_y as u32, c, 255);
            }
        }

        // 预览画板（宪法 §六 跳框预览画板条款，十修）：展台框（均匀细框
        // paint_thin_frame——非池行场合，十一修起不属三级框）+ 组件实
        // 涂装微缩实时渲染（种类 = 条目 preview 维，原语全复用共享件——
        // §六 样式唯一来源）
        let prev = md::preview_rect(&card);
        paint_thin_frame(
            &mut frame,
            prev.x + off,
            prev.y,
            prev.w,
            prev.h,
            accent,
            denom,
            (0, i64::from(h)),
        );
        let prev_screen = crate::ui::dual_pool::PoolRect {
            x: prev.x + off,
            ..prev.clone()
        };
        self.paint_preview_impl(
            &mut frame,
            entry.preview,
            &prev_screen,
            accent,
            denom,
            now_ms,
        );

        // 字段区：题注（30px 灰）在上 + 内容行（36px 亮）在下
        let mut pen = md::fields_top(&card);
        for f in &fields {
            if pen + i64::from(md::MODAL_LABEL_H) > ink_bottom {
                break;
            }
            self.draw_text_left_ex(
                &mut frame,
                &f.label,
                text_x,
                text_w,
                pen as u32,
                md::MODAL_LABEL_H,
                30.0,
                meta_fg,
                0.0,
                None,
            );
            pen += i64::from(md::MODAL_LABEL_H);
            for line in &f.lines {
                if pen + i64::from(md::MODAL_LINE_H) > ink_bottom {
                    break;
                }
                self.draw_text_left_ex(
                    &mut frame,
                    line,
                    text_x,
                    text_w,
                    pen as u32,
                    md::MODAL_LINE_H,
                    36.0,
                    title_fg,
                    0.0,
                    None,
                );
                pen += i64::from(md::MODAL_LINE_H);
            }
            pen += i64::from(md::MODAL_FIELD_GAP);
        }

        // 关闭钮：卡底全内宽 3 格，均匀细框（paint_thin_frame——非池行
        // 场合不属三级框，十一修）+ 居中 36px 亮字
        paint_thin_frame(
            &mut frame,
            btn.x + off,
            btn.y,
            btn.w,
            btn.h,
            accent,
            denom,
            (0, i64::from(h)),
        );
        self.draw_text_centered(
            &mut frame,
            "关闭",
            btn.x + off,
            btn.y,
            btn.w,
            btn.h,
            36.0,
            title_fg,
            btn.x + off,
        );
    }

    /// 跳框预览画板涂装（宪法 §六 跳框预览画板条款，2026-09-13 十修）：
    /// 按条目的 preview 维把**组件实涂装微缩实时渲染**进展台——原语全
    /// 复用共享件（paint_rect_ring/paint_row_frame/paint_tab_chip/
    /// paint_open_cursor/orb sprite/gear paint_at），动效引擎类画函数
    /// 曲线/示意图。r = 展台矩形（屏坐标，含面板 off）；展品画在展台
    /// 内缩 1 格的内区，纵向按展台裁剪。
    /// **十四修：动效引擎四件 = 动画演示**——静态底图上叠动画层：
    /// 白球（r10 α220）= 手指触摸，拖动/点触驱动展品按引擎实函数
    /// 运动；相位 = now_ms % 2400 循环（相位表即考题钉，见
    /// termview_spec 动效预览四条）
    #[allow(clippy::too_many_lines)]
    fn paint_preview_impl(
        &self,
        frame: &mut Frame<'_>,
        pv: crate::ui::comp_registry::Preview,
        r: &crate::ui::dual_pool::PoolRect,
        accent: crate::ui::accent::AccentPair,
        denom: i64,
        now_ms: u64,
    ) {
        use crate::ui::comp_registry::Preview;
        let grad = RingGradient {
            c1: accent.c1,
            c2: accent.c2,
            x0: 0,
            y0: 0,
            denom,
        };
        let clip = (r.y, r.y + i64::from(r.h)); // 展品不出展台（纵向裁剪带）
        // 展台内区：四向各让 1 格
        let ix = r.x + i64::from(CELL_W);
        let iy = r.y + i64::from(CELL_H / 2);
        let iw = r.w.saturating_sub(CELL_W * 2);
        let ih = r.h.saturating_sub(CELL_H);
        if iw < 8 || ih < 8 {
            return;
        }
        let icx = ix + i64::from(iw) / 2; // 内区心 x
        let title_fg = 0x00D9_D9D9;
        let meta_fg = 0x0080_8080;
        // 白球 = 手指（十四修 §六 动效预览动画条款）：r10 覆盖率圆，
        // alpha 入参（0 = 不画）；相位表见各动画臂与考题钉
        fn finger_ball(frame: &mut Frame<'_>, cx: i64, cy: i64, alpha: u32) {
            if alpha == 0 {
                return;
            }
            for dy in -10..=10i64 {
                for dx in -10..=10i64 {
                    if dx * dx + dy * dy <= 100 {
                        let (xx, yy) = (cx + dx, cy + dy);
                        if xx >= 0 && xx < i64::from(frame.w) && yy >= 0 && yy < i64::from(frame.h)
                        {
                            frame.blend_px(xx as u32, yy as u32, 0x00FF_FFFF, alpha);
                        }
                    }
                }
            }
        }
        let at = now_ms % 2400; // 动画相位（循环 2400ms，十四修拍板）
        match pv {
            Preview::Ring => {
                // 小页环：池框同配方（c1→c2 外壳向），宽 ≈ 内区 2/3
                let rw = (i64::from(iw) * 2 / 3) as u32;
                let x0 = icx - i64::from(rw) / 2;
                paint_rect_ring(
                    frame,
                    x0,
                    iy,
                    x0 + i64::from(rw),
                    iy + i64::from(ih),
                    0,
                    i64::MAX,
                    crate::ui::accent::CARD_PAGE_BG,
                    accent.c1,
                    accent.c2,
                    24,
                    true,
                );
            }
            Preview::RowFrame => {
                // 选中 + 未选中各一条（行高 2 格微缩，间距 0.5 格）
                let rh = CELL_H * 2;
                let gap = CELL_H / 2;
                let y0 = iy + (i64::from(ih) - i64::from(rh * 2 + gap)) / 2;
                for (k, sel) in [true, false].into_iter().enumerate() {
                    paint_row_frame(
                        frame,
                        ix,
                        y0 + k as i64 * i64::from(rh + gap),
                        iw,
                        rh,
                        sel,
                        accent,
                        denom,
                        clip,
                    );
                }
            }
            Preview::ThinFrame => {
                // 均匀细框两条（一高一矮演示圆角按短边钳；非池行通用件）
                let th = 54u32;
                let gap = 18i64;
                let y0 = iy + (i64::from(ih) - (2 * i64::from(th) + gap)) / 2;
                paint_thin_frame(frame, ix, y0, iw, th, accent, denom, clip);
                paint_thin_frame(
                    frame,
                    ix,
                    y0 + i64::from(th) + gap,
                    iw,
                    th / 2,
                    accent,
                    denom,
                    clip,
                );
            }
            Preview::ModalMini => {
                // 自指：迷你页底 + 压暗 + 小卡 + 小关闭钮
                for dy in 0..ih as i64 {
                    for dx in 0..iw as i64 {
                        frame.blend_px((ix + dx) as u32, (iy + dy) as u32, 0x00FF_FFFF, 8);
                    }
                }
                for dy in 0..ih as i64 {
                    for dx in 0..iw as i64 {
                        frame.blend_px((ix + dx) as u32, (iy + dy) as u32, 0x0000_0000, 120);
                    }
                }
                let cw = i64::from(iw) * 2 / 3;
                let ch = i64::from(ih) * 3 / 4;
                let cx0 = icx - cw / 2;
                let cy0 = iy + (i64::from(ih) - ch) / 2;
                paint_rect_ring(
                    frame,
                    cx0,
                    cy0,
                    cx0 + cw,
                    cy0 + ch,
                    0,
                    i64::MAX,
                    crate::ui::accent::CARD_PAGE_BG,
                    accent.c2,
                    accent.c1,
                    18,
                    true,
                );
                let bw = cw - 2 * i64::from(CELL_W);
                paint_thin_frame(
                    frame,
                    cx0 + i64::from(CELL_W),
                    cy0 + ch - i64::from(CELL_H / 2) - 24,
                    bw as u32,
                    24,
                    accent,
                    denom,
                    clip,
                );
            }
            Preview::TabChip => {
                // 选中块 + 未选中块 + 底线（标签行微缩语境；十一修随机色
                // 体系：选中块吃页 accent（≡ 选中标签双色），未选中块刻意
                // 吃 FALLBACK 演示「每标签独立双色」；底线 = 配合标签模式
                // 纯色 = 选中标签 c2，与选中块下 1/3 同色一体）
                let ch = 54u32;
                let y0 = iy + (i64::from(ih) - i64::from(ch)) / 2 - 9;
                let w0 = 10 * CELL_W; // 「选中」2 字 + padding 微缩定宽
                paint_tab_chip(
                    frame,
                    icx - i64::from(w0) - 9,
                    y0,
                    w0,
                    ch,
                    true,
                    accent,
                    0,
                    i64::MAX,
                );
                paint_tab_chip(
                    frame,
                    icx + 9,
                    y0,
                    w0,
                    ch,
                    false,
                    crate::ui::accent::FALLBACK,
                    0,
                    i64::MAX,
                );
                self.draw_text_centered(
                    frame,
                    "选中",
                    icx - i64::from(w0) - 9,
                    y0,
                    w0,
                    ch,
                    30.0,
                    crate::ui::accent::CARD_PAGE_BG,
                    icx - i64::from(w0) - 9,
                );
                self.draw_text_centered(frame, "未选", icx + 9, y0, w0, ch, 30.0, meta_fg, icx + 9);
                // 底线：配合标签模式 = 选中标签 c2 纯色 α255，内区同宽
                let uy = y0 + i64::from(ch);
                for ax in ix..(ix + i64::from(iw)) {
                    if ax < 0 || ax >= i64::from(frame.w) || uy < 0 || uy >= i64::from(frame.h) {
                        continue;
                    }
                    frame.blend_px(ax as u32, uy as u32, accent.c2, 255);
                }
            }
            Preview::Underline => {
                let uy = iy + i64::from(ih) / 2;
                for ax in ix..(ix + i64::from(iw)) {
                    if ax < 0 || ax >= i64::from(frame.w) || uy < 0 || uy >= i64::from(frame.h) {
                        continue;
                    }
                    let c = ring_gradient_rgb(accent.c2, accent.c1, ax, uy, denom);
                    frame.blend_px(ax as u32, uy as u32, c, 255);
                }
            }
            Preview::Dropdown => {
                // 十三修：触发器（三级框+▼）+ 圆角深底下弹 panel 两项
                // （第二项选中 = 均匀细框）
                let th = 54u32;
                let y0 = iy + 6;
                paint_row_frame(frame, ix, y0, iw, th, true, accent, denom, clip);
                self.draw_text_left_ex(
                    frame,
                    "选项甲",
                    ix as u32,
                    iw.saturating_sub(72),
                    y0 as u32,
                    th,
                    30.0,
                    meta_fg,
                    18.0,
                    None,
                );
                let tri_cx = ix + i64::from(iw) - 27;
                let tri_y = y0 + i64::from(th) / 2 - 4;
                for dy in 0..9u32 {
                    let half_w = 8 - dy;
                    for dx in 0..(half_w * 2 + 1) as i64 {
                        let xx = tri_cx - i64::from(half_w) + dx;
                        let yy = tri_y + i64::from(dy);
                        if xx >= 0 && xx < i64::from(frame.w) && yy >= 0 && yy < i64::from(frame.h)
                        {
                            frame.blend_px(xx as u32, yy as u32, title_fg, 255);
                        }
                    }
                }
                let py0 = y0 + i64::from(th);
                let prr = (POOL_FRAME_R as i64).min((iw / 2).min(th) as i64) as u32;
                for dy in 0..(th * 2) as i64 {
                    for dx in 0..iw as i64 {
                        let (xx, yy) = (ix + dx, py0 + dy);
                        if xx >= 0
                            && xx < i64::from(frame.w)
                            && yy >= 0
                            && yy < i64::from(frame.h)
                            && rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, iw, th * 2, prr) < 0.0
                        {
                            frame.blend_px(xx as u32, yy as u32, 0x0000_0000, 252);
                        }
                    }
                }
                // 选中项（第二项）= 均匀细框；未选中项纯深底
                let oy1 = py0 + i64::from(th);
                paint_thin_frame(frame, ix, oy1, iw, th, accent, denom, clip);
            }
            Preview::FieldLabel => {
                // 标签列（36 亮 + 十三修背衬：渐变暗底+8% 白提亮）+
                // 值框（30 灰）mini 行
                let y0 = iy + (i64::from(ih) - i64::from(CELL_H * 2)) / 2;
                {
                    // 背衬块：与值框同高对齐（CELL_H 高，圆角钳半高）
                    let (bx, by, bw, bh) = (ix, y0 + i64::from(CELL_H / 2), 216u32, CELL_H);
                    let br = (POOL_FRAME_R as i64).min((bw / 2).min(bh / 2) as i64) as u32;
                    for dy in 0..bh as i64 {
                        let yy = by + dy;
                        if yy < clip.0 || yy >= clip.1 {
                            continue;
                        }
                        for dx in 0..bw as i64 {
                            let xx = bx + dx;
                            if xx < 0 || xx >= i64::from(frame.w) {
                                continue;
                            }
                            if rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, bw, bh, br) >= 0.0 {
                                continue;
                            }
                            frame.buf[yy as usize * i64::from(frame.w) as usize + xx as usize] =
                                frame_bg_rgb(accent.c1, accent.c2, xx, yy, denom);
                            frame.blend_px(xx as u32, yy as u32, 0x00FF_FFFF, 20);
                        }
                    }
                }
                self.draw_text_left_ex(
                    frame,
                    "标签",
                    ix as u32,
                    216,
                    y0 as u32,
                    CELL_H * 2,
                    36.0,
                    title_fg,
                    18.0,
                    None,
                );
                paint_row_frame(
                    frame,
                    ix + 216,
                    y0 + i64::from(CELL_H / 2),
                    iw.saturating_sub(216),
                    CELL_H,
                    false,
                    accent,
                    denom,
                    clip,
                );
                self.draw_text_left_ex(
                    frame,
                    "值",
                    (ix + 216) as u32,
                    iw.saturating_sub(216 + 18),
                    (y0 + i64::from(CELL_H / 2)) as u32,
                    CELL_H,
                    30.0,
                    meta_fg,
                    18.0,
                    None,
                );
            }
            Preview::OpenCursor => {
                // 封存件复活展出：开口框（左强调线+顶底发丝+绿青底垫）
                let cw = i64::from(iw) * 2 / 3;
                let chh = i64::from(CELL_H * 2);
                let cx0 = icx - cw / 2;
                let cy0 = iy + (i64::from(ih) - chh) / 2;
                paint_open_cursor(
                    frame,
                    cx0,
                    cy0,
                    cw,
                    chh,
                    cw * 2 / 5,
                    cw * 3 / 5,
                    0,
                    i64::MAX,
                    grad,
                    0x002E_D5A3,
                );
            }
            Preview::Orb => {
                // 真渲染：sprite 加法合成微缩（rs 48 = 半屏光球的 3/4）
                let sprite = crate::ui::orb::build_orb_sprite(48.0, 1.0);
                crate::ui::orb::blit_orb_sprite(
                    frame.buf,
                    frame.w,
                    frame.h,
                    &sprite,
                    icx as f64,
                    (iy + i64::from(ih) / 2) as f64,
                    1.0,
                );
            }
            Preview::InputBar => {
                // 栏框 + 占位灰字 + 发送钮（accent 实底 + ▶ 三角像素）
                let bh = 54u32;
                let y0 = iy + (i64::from(ih) - i64::from(bh)) / 2;
                let sw = 54u32;
                paint_thin_frame(
                    frame,
                    ix,
                    y0,
                    iw.saturating_sub(sw + 12),
                    bh,
                    accent,
                    denom,
                    clip,
                );
                self.draw_text_left_ex(
                    frame,
                    "输入消息…",
                    ix as u32,
                    iw.saturating_sub(sw + 12),
                    y0 as u32,
                    bh,
                    30.0,
                    meta_fg,
                    18.0,
                    None,
                );
                let sx0 = ix + i64::from(iw) - i64::from(sw);
                for dy in 0..bh as i64 {
                    for dx in 0..sw as i64 {
                        let (xx, yy) = (sx0 + dx, y0 + dy);
                        if xx >= 0
                            && xx < i64::from(frame.w)
                            && yy >= 0
                            && yy < i64::from(frame.h)
                            && rr_sdf(dx as f32 + 0.5, dy as f32 + 0.5, sw, bh, 18) < 0.0
                        {
                            let c = grad.sample(xx, yy);
                            frame.blend_px(xx as u32, yy as u32, c, 255);
                        }
                    }
                }
                let tcy = y0 + i64::from(bh) / 2;
                let tcx = sx0 + i64::from(sw) / 2 - 4;
                for dy in 0..15i64 {
                    let hw = (15 - dy) / 2;
                    for dx in 0..=hw {
                        let (xx, yy) = (tcx + dy * 2 / 2 + dx, tcy - 7 + dy);
                        if xx >= 0 && xx < i64::from(frame.w) && yy >= 0 && yy < i64::from(frame.h)
                        {
                            frame.blend_px(xx as u32, yy as u32, 0x00FF_FFFF, 255);
                        }
                    }
                }
            }
            Preview::Keybar => {
                // 两排键格微缩（5 列 ×2 排，键 = 渐变均匀细框圆角格，
                // paint_thin_frame——十一修起非池行细框的共享件）
                let cols = 5u32;
                let kg = 12u32;
                let kw = iw.saturating_sub((cols - 1) * kg) / cols;
                let kh = 48u32;
                let rows_y = iy + (i64::from(ih) - i64::from(kh * 2 + kg)) / 2;
                for row in 0..2u32 {
                    for col in 0..cols {
                        paint_thin_frame(
                            frame,
                            ix + i64::from(col * (kw + kg)),
                            rows_y + i64::from(row * (kh + kg)),
                            kw,
                            kh,
                            accent,
                            denom,
                            clip,
                        );
                    }
                }
            }
            Preview::Gear => {
                crate::ui::gear::paint_at(
                    frame.buf,
                    frame.w,
                    frame.h,
                    icx as u32,
                    (iy + i64::from(ih) / 2) as u32,
                );
            }
            Preview::CurveSpring => {
                // spring_pos 响应曲线（0→100 目标，过冲可见）+ 目标虚线
                let span_ms = 600u32;
                let target_y = iy + i64::from(ih) - i64::from(ih) * 100 / 130;
                for ax in ix..(ix + i64::from(iw)) {
                    if ax % 6 < 3 && ax >= 0 && ax < i64::from(frame.w) && target_y >= 0 {
                        frame.blend_px(ax as u32, target_y as u32, 0x00FF_FFFF, 40);
                    }
                }
                let mut prev: Option<(i64, i64)> = None;
                for step in 0..iw {
                    let t = u64::from(step) * u64::from(span_ms) / u64::from(iw);
                    let pos = crate::ui::fx_spring::spring_pos(0.0, 100.0, t);
                    let px_x = ix + i64::from(step);
                    let px_y = iy + i64::from(ih) - (pos / 130.0 * ih as f32) as i64;
                    if let Some((_, ly)) = prev {
                        // 折线补间（逐列画点，斜率大处竖向连点）
                        let (ya, yb) = if ly <= px_y { (ly, px_y) } else { (px_y, ly) };
                        for yy in ya..=yb {
                            if px_x >= 0 && px_x < i64::from(frame.w) && yy >= clip.0 && yy < clip.1
                            {
                                let c = grad.sample(px_x, yy);
                                frame.blend_px(px_x as u32, yy as u32, c, 255);
                            }
                        }
                    }
                    prev = Some((px_x, px_y));
                }
                // 动画层（十四修 §六）：白球在曲线原点淡入→按住→淡出；
                // 响应点 300 起沿 spring_pos 实曲线骑行，900 停终点
                let origin_y = iy + i64::from(ih);
                let ball_a = if at < 200 {
                    at as u32 * 220 / 200
                } else if at < 300 {
                    220
                } else if at < 500 {
                    220 - (at - 300) as u32 * 220 / 200
                } else {
                    0
                };
                finger_ball(frame, ix, origin_y, ball_a);
                if at >= 300 {
                    let prog = (at - 300).min(600);
                    let pos = crate::ui::fx_spring::spring_pos(0.0, 100.0, prog);
                    let dx = ix + i64::from(iw) * prog as i64 / 600;
                    let dyy = origin_y - (pos / 130.0 * ih as f32) as i64;
                    for ddy in -6..=6i64 {
                        for ddx in -6..=6i64 {
                            if ddx * ddx + ddy * ddy <= 36 {
                                let (xx, yy) = (dx + ddx, dyy + ddy);
                                if xx >= 0 && xx < i64::from(frame.w) && yy >= clip.0 && yy < clip.1
                                {
                                    frame.blend_px(xx as u32, yy as u32, 0x00FF_FFFF, 255);
                                }
                            }
                        }
                    }
                }
            }
            Preview::CurveEase => {
                // 两族同图：ease-out（c1）与 ease-in（c2）= 静态底图
                for step in 0..iw {
                    let t = step as f32 / iw as f32;
                    let yo = crate::ui::fx_ease::ease_out_cubic(t);
                    let yi = crate::ui::fx_ease::ease_in_cubic(t);
                    let px_x = ix + i64::from(step);
                    for (v, c) in [(yo, accent.c1), (yi, accent.c2)] {
                        let px_y = iy + i64::from(ih) - (v * ih as f32) as i64;
                        if px_x >= 0 && px_x < i64::from(frame.w) && px_y >= clip.0 && px_y < clip.1
                        {
                            frame.blend_px(px_x as u32, px_y as u32, c, 255);
                        }
                    }
                }
                // 动画层（十四修 §六）：白球点触小面板顶心→面板
                // ease_out 350ms 下落（= AI 面板实节奏）→停底→
                // ease_in 250ms 收起；球按住到 650 后淡出
                let pw2 = i64::from(iw) * 2 / 3;
                let ph2 = 54i64;
                let px0 = ix + i64::from(iw) / 6;
                let span = i64::from(ih) - ph2;
                let p = if at < 300 {
                    0.0
                } else if at < 650 {
                    crate::ui::fx_ease::ease_out_cubic((at - 300) as f32 / 350.0)
                } else if at < 1500 {
                    1.0
                } else if at < 1750 {
                    1.0 - crate::ui::fx_ease::ease_in_cubic((at - 1500) as f32 / 250.0)
                } else {
                    0.0
                };
                let py = iy + (p * span as f32) as i64;
                paint_thin_frame(frame, px0, py, pw2 as u32, ph2 as u32, accent, denom, clip);
                let ball_a = if at < 200 {
                    at as u32 * 220 / 200
                } else if at < 650 {
                    220
                } else if at < 850 {
                    220 - (at - 650) as u32 * 220 / 200
                } else {
                    0
                };
                finger_ball(frame, px0 + pw2 / 2, py + 27, ball_a);
            }
            Preview::Swipe => {
                // 轨迹线 + 起点圆 + 终点箭头（横向锁定制示意）
                let my = iy + i64::from(ih) / 2;
                let x0 = ix + 30;
                let x1 = ix + i64::from(iw) - 60;
                for xx in x0..x1 {
                    for dy in -1..=1i64 {
                        let yy = my + dy;
                        if xx >= 0 && xx < i64::from(frame.w) && yy >= 0 && yy < i64::from(frame.h)
                        {
                            let c = grad.sample(xx, yy);
                            frame.blend_px(xx as u32, yy as u32, c, 200);
                        }
                    }
                }
                // 起点圆（c1 实填 r10）
                for dy in -10..=10i64 {
                    for dx in -10..=10i64 {
                        if dx * dx + dy * dy <= 100 {
                            let (xx, yy) = (x0 + dx, my + dy);
                            if xx >= 0
                                && xx < i64::from(frame.w)
                                && yy >= 0
                                && yy < i64::from(frame.h)
                            {
                                frame.blend_px(xx as u32, yy as u32, accent.c1, 255);
                            }
                        }
                    }
                }
                // 终点箭头（c2 实心三角，尖朝右）
                for k in 0..21i64 {
                    let half = 10 - k / 2;
                    for dy in -half..=half {
                        let (xx, yy) = (x1 + k, my + dy);
                        if xx >= 0 && xx < i64::from(frame.w) && yy >= 0 && yy < i64::from(frame.h)
                        {
                            frame.blend_px(xx as u32, yy as u32, accent.c2, 255);
                        }
                    }
                }
                // 动画层（十四修 §六）：白球起点淡入→1:1 拖到轨道 70%
                // （小卡片跟手）→松手卡片 ease_out 500ms 滑到终点、球淡出
                let cw2 = i64::from(iw) / 4;
                let ch2 = i64::from(ih) / 3;
                let release_x = x0 + (x1 - x0) * 7 / 10;
                let slide = |t0: u64| {
                    release_x
                        + (crate::ui::fx_ease::ease_out_cubic((t0 - 1200) as f32 / 500.0)
                            * (x1 - release_x) as f32) as i64
                };
                let (ball_x, card_cx, ball_a) = if at < 200 {
                    (x0, x0, at as u32 * 220 / 200)
                } else if at < 1200 {
                    let bx = x0 + (at - 200) as i64 * (release_x - x0) / 1000;
                    (bx, bx, 220)
                } else if at < 1400 {
                    (release_x, slide(at), 220 - (at - 1200) as u32 * 220 / 200)
                } else if at < 1700 {
                    (release_x, slide(at), 0)
                } else {
                    (release_x, x1, 0)
                };
                let cx0 = (card_cx - cw2 / 2).clamp(ix, ix + i64::from(iw) - cw2);
                paint_thin_frame(
                    frame,
                    cx0,
                    my - ch2 / 2,
                    cw2 as u32,
                    ch2 as u32,
                    accent,
                    denom,
                    clip,
                );
                finger_ball(frame, ball_x, my, ball_a);
            }
            Preview::ViewportPush => {
                // 旧页挤出（左，灰框）+ 新页推入（右，accent 框）——
                // 十四修动画层：白球右缘淡入→左拖，新页 1:1 跟手推入、
                // 旧页同比挤出；1100 松手 rise_release 180ms 补到靠泊
                //（BAR-095：与实机面板召唤同款，预览不得自编曲线）
                let ph = ih * 3 / 4;
                let py0 = iy + (i64::from(ih) - i64::from(ph)) / 2;
                let pw = i64::from(iw) / 2;
                let p = if at < 300 {
                    0.0f32
                } else if at < 1100 {
                    (at - 300) as f32 / 800.0 * 0.8
                } else if at < 1280 {
                    0.8 + crate::ui::fx_ease::rise_release((at - 1100) as f32 / 180.0) * 0.2
                } else {
                    1.0
                };
                let new_left = icx + ((1.0 - p) * pw as f32) as i64;
                let old_left = ix - pw / 3 - (p * (pw / 2) as f32) as i64;
                paint_rect_ring(
                    frame,
                    old_left,
                    py0,
                    old_left + pw * 5 / 6,
                    py0 + i64::from(ph),
                    ix,
                    i64::MAX,
                    crate::ui::accent::CARD_PAGE_BG,
                    0x0040_4040,
                    0x0060_6060,
                    18,
                    true,
                );
                paint_rect_ring(
                    frame,
                    new_left,
                    py0,
                    new_left + pw,
                    py0 + i64::from(ph),
                    0,
                    ix + i64::from(iw),
                    crate::ui::accent::CARD_PAGE_BG,
                    accent.c2,
                    accent.c1,
                    18,
                    true,
                );
                let ball_a = if at < 200 {
                    at as u32 * 220 / 200
                } else if at < 1100 {
                    220
                } else if at < 1300 {
                    220 - (at - 1100) as u32 * 220 / 200
                } else {
                    0
                };
                let ball_x = if at < 300 {
                    ix + i64::from(iw) - 20
                } else {
                    new_left.max(ix + 10)
                };
                finger_ball(frame, ball_x, py0 + i64::from(ph) / 2, ball_a);
            }
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
        self.draw_items_left_inset(frame, items, cx, cw, cy, rh, px, fg, clip_y, 18.0);
    }

    /// 考题专用通道（BAR-088 钉：恰好满宽末字必须落墨）——集成测试
    /// 摸不到 pub(crate) Frame，经此薄壳直打 draw_items_left_inset
    /// 本体（单源不抄实现，同 pub text_width 先例）
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn spec_draw_items_left(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        items: &[(&fontdue::Font, char, f32)],
        cx: u32,
        cw: u32,
        cy: u32,
        rh: u32,
        px: f32,
        fg: u32,
    ) {
        let mut frame = Frame { buf, w, h };
        self.draw_items_left_inset(&mut frame, items, cx, cw, cy, rh, px, fg, None, 0.0);
    }

    /// draw_items_left 全参版：显式起笔内缩（18 是输入栏标定，四版
    /// 配置页 ×1.5 = 27；老调用方走 draw_items_left 行为不变）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_items_left_inset(
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
        inset: f32,
    ) {
        let Some(hm) = self.font.horizontal_line_metrics(px) else {
            return;
        };
        let mut pen_x = cx as f32 + inset;
        let clip_right = cx + cw;
        let baseline = cy as f32 + (rh as f32 - (hm.ascent - hm.descent)) / 2.0 + hm.ascent;
        for (f, c, adv) in items {
            // BAR-088：恰好满宽 = 装得下（> 才 break）——像素字体步进
            // 整数和 == 内宽时 >= 必误杀末字（CJK 实机字段行集体少一字；
            // host DejaVu 非整数步进永远打不中此边界，考题靠手搓整数
            // 步进 items 钉死，见 termview_spec）
            if pen_x + adv > clip_right as f32 {
                break; // 右缘装不下就停（v1 无横滚，截断即判卷）
            }
            let g = self.rasterize_cached(f, *c, px); // BAR-102：缓存光栅
            let (m, bmp) = (&g.0, &g.1);
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

    /// 字段框文涂装（十四修 §五 动态宽度条款）：measure 序列按
    /// cfg_page::wrap_field_lines 贪心折行（≤2 行，余量进末行靠右缘
    /// 裁剪），逐行整体垂直居中于 (cy, rh)；align_left = 标签（逐行
    /// 左对齐，起笔 cx+1.5 格），否则 = 值（逐行右对齐，末笔贴
    /// cx+cw−1.5 格）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_field_lines(
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
        align_left: bool,
    ) {
        let inset = crate::ui::cfg_page::FIELD_TEXT_INSET;
        let inner = cw.saturating_sub(inset * 2);
        if inner == 0 || items.is_empty() {
            return;
        }
        let widths: Vec<f32> = items.iter().map(|it| it.2).collect();
        let lines = crate::ui::cfg_page::wrap_field_lines(&widths, inner as f32);
        let line_h = (px * 4.0 / 3.0).ceil() as u32;
        let total = line_h * lines.len() as u32;
        let top = cy + rh.saturating_sub(total) / 2;
        for (k, (s, e, line_w)) in lines.iter().enumerate() {
            let lcy = top + k as u32 * line_h;
            if align_left {
                self.draw_items_left_inset(
                    frame,
                    &items[*s..*e],
                    cx + inset,
                    inner,
                    lcy,
                    line_h,
                    px,
                    fg,
                    clip_y,
                    0.0,
                );
            } else {
                // 右对齐：起笔 = 右内缘 − 行宽；行超内宽时左贴内缘
                // （右缘裁剪归 draw_items_left_inset 的 clip_right）
                let right = cx + cw - inset;
                let x0 = ((right as f32) - line_w).max(cx as f32 + inset as f32) as u32;
                self.draw_items_left_inset(
                    frame,
                    &items[*s..*e],
                    x0,
                    right.saturating_sub(x0),
                    lcy,
                    line_h,
                    px,
                    fg,
                    clip_y,
                    0.0,
                );
            }
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
        let g = self.rasterize_cached(font, c, font_px); // BAR-102：缓存光栅
        let (metrics, bitmap) = (&g.0, &g.1);
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

// 三公民页面（文件树/解析/配置）底色/边框已迁随机 accent 体系
// （theme.md 宪法 §2.2，2026-09-12）：底色统一 `ui::accent::CARD_PAGE_BG`
// 深底，边框环色由 `ui::accent::AccentRng` 召唤即随机生成（kfmv4
// `_generateRandomAccents` 约束区间 HSL 移植），涂装函数 accent 入参。
// 旧固定色常量（CFG/FT/PT_PAGE_BG + FRAME_C1/C2）同日删除。

/// 终端卡片壳底色/边框（2026-09-11 用户拍板「终端也包一个全屏卡片壳，
/// 样式统一」）：同配方**无色相碳灰**——终端是基座不是卡，一眼看出
/// 「这是底」。低饱和钉：r≈g≈b（与三面板的彩色相机器可区分）；底色
/// 近黑微蓝灰，比屏外纯黑略亮（卡片感 = 壳内略亮于壳外）
pub const TERM_CARD_BG: u32 = 0x000D_0F13;
pub const TERM_FRAME_C1: u32 = 0x00AE_B6C2; // 亮银灰 rgba(174,182,194,~.8)
pub const TERM_FRAME_C2: u32 = 0x0046_4C55; // 暗碳灰 rgba(70,76,85,~.7)

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
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x1 <= x || y1 <= y {
            return;
        }
        // 行切片 fill（BAR-103）：逐像素索引的边界检查在全屏底色填充
        // （3M px）上是实税，slice::fill 编译期单检查 ≈ memset
        for row in y..y1 {
            let start = (row * self.w + x) as usize;
            self.buf[start..start + (x1 - x) as usize].fill(color);
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
        // 带切（2026-09-06）：中带行（矩形竖直中段）的墨只存在于左右
        // spread 缘条——内部像素 d<=0 全是零墨废访（输入栏 field 光晕
        // 480k 次 SDF 的 ~85%）。角带行（含上下外扩）保持全宽
        let rc = i64::from(r)
            .min(w.max(1) as i64 / 2)
            .min(h.max(1) as i64 / 2);
        for ay in y0..y1 {
            let ly = ay - y;
            let corner_row = ly < rc + spread || ly >= i64::from(h) - rc - spread;
            for ax in x0..x1 {
                if !corner_row && ax >= x && ax < x + i64::from(w) {
                    continue; // 中带行的矩形内部：零墨
                }
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
        // BAR-067：装饰混合不改目标的透明度——chrome 层半透像素（栏带
        // (α,E) 底）上叠发丝线/发光/veil 时，α 必须原样保留（否则掉回
        // 0 被条件 alpha 强转成不透明补丁）。softbuffer 路径高字节恒 0，
        // 保留位运算无影响
        let keep = *dst & 0xFF00_0000;
        *dst = keep | blend(fg, *dst, a);
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

/// 135° 对角渐变采样（paint_rect_ring 配方抽核，宪法 §六 禁手抄）：
/// t = (lx + ly)·255/denom——lx/ly = 采样点相对渐变框原点（框内恒
/// 非负），denom = (框宽−1)+(框高−1)。页环/池框/功能光标（五修起
/// 吃 accent 渐变）同一把尺：光标像素 = 从页环渐变布上剪下来的
pub fn ring_gradient_rgb(c1: u32, c2: u32, lx: i64, ly: i64, denom: i64) -> u32 {
    let d = denom.max(1) as u64;
    let t = (((lx + ly).max(0) as u64 * 255) / d).min(255) as u32;
    lerp_rgb(c1, c2, t)
}

/// 渐变框（ring_gradient_rgb 的采样坐标系打包）：双色 + 框原点 +
/// 分母。涂装函数间传递用，采样 = 同一坐标系里的绝对像素点
#[derive(Debug, Clone, Copy)]
pub struct RingGradient {
    pub c1: u32,
    pub c2: u32,
    pub x0: i64,
    pub y0: i64,
    pub denom: i64,
}

impl RingGradient {
    /// 采样绝对像素点 (ax, ay) 的渐变色
    pub fn sample(&self, ax: i64, ay: i64) -> u32 {
        ring_gradient_rgb(self.c1, self.c2, ax - self.x0, ay - self.y0, self.denom)
    }
}

/// 渐变暗底压暗档（宪法 §三 十二修标定，2026-09-14 用户拍板）：dark(c)
/// = lerp(c, 黑, FRAME_BG_DIM) ≈ 22% 亮度——输入栏内芯 field_bg 相对
/// 描边色的实测比例。考题侧用字面量 200 钉（本常量漂移即红）
pub const FRAME_BG_DIM: u32 = 200;

/// 渐变暗底采样（宪法 §三 十二修：凡 135° 渐变装修框的内芯 = 同尺暗部
/// 渐变——dark(c1)→dark(c2)，同原点同分母，「从同一块渐变布上剪下来的
/// 暗部」；不透明写入，取代 4% 白平填）。无边框组件（未选中行/值框）
/// 同吃——框没了暗底在
pub fn frame_bg_rgb(c1: u32, c2: u32, lx: i64, ly: i64, denom: i64) -> u32 {
    ring_gradient_rgb(
        lerp_rgb(c1, 0, FRAME_BG_DIM),
        lerp_rgb(c2, 0, FRAME_BG_DIM),
        lx,
        ly,
        denom,
    )
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
    fn render_into(&mut self, buf: &mut [u32], w: u32, h: u32, card_bottom_inset: u32);
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
    /// 文本实量宽 px（十四修 §五 字段框动态宽度：触摸命中量宽与涂装
    /// 同一条 measure_items 尺——android_app 池区手势命中调用方）
    fn text_width(&self, text: &str, px: f32) -> u32;
    fn render_keybar(&self, buf: &mut [u32], w: u32, h: u32, ime_bottom: u32, mods: u8);
    /// 配置卡标签栏涂装（主题宪法 §四，2026-09-13 五修）：标签文字
    /// （格内居中、内容带左缘裁剪）+ 选中功能光标开口框（ui/cursor.rs
    /// 规格——线色吃本页 accent 双色渐变，渐变框与页环同原点同分母；
    /// bottom_inset 即页环 inset，渐变分母同源的关键）。cfg_off_x =
    /// 面板刚体平移（GLES 烘焙调用恒 0，位移在合成期；softbuffer/值守
    /// 倒帧传真值）。画在配置页底装修之上
    #[allow(clippy::too_many_arguments)]
    fn paint_cfg_tab_bar(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        snap: &crate::ui::tab_bar::TabBarSnap,
        cfg_off_x: i32,
        bottom_inset: u32,
        accent: crate::ui::accent::AccentPair,
    );
    /// 配置卡双池涂装（主题宪法 §五，2026-09-12）：上池/下池两个二级
    /// 卡片框（paint_rect_ring 同配方；内卡渐变反转 c2→c1，§三 多级
    /// 嵌套逐层反转）。cfg_off_x 语义同 paint_cfg_tab_bar；画在配置页
    /// 底装修之上、标签栏同层
    fn paint_cfg_dual_pool(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        snap: &crate::ui::dual_pool::DualPoolSnap,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
    );
    /// 配置卡双池内容涂装（宪法 §五 目录语义，2026-09-13 三层目录）：
    /// 标签栏**层**涂装（BAR-096 拆槽，考题/壳层门面）：层缓冲 = 屏宽 ×
    /// TAB_LAYER_H，y_shift = 内容原点（层坐标）；整页语义传 0
    #[allow(clippy::too_many_arguments)]
    fn paint_tab_bar_layer(
        &self,
        buf: &mut [u32],
        cw: u32,
        ch: u32,
        y_shift: i64,
        snap: &crate::ui::tab_bar::TabBarSnap,
        accent: crate::ui::accent::AccentPair,
    );
    /// 下池光标**层**涂装（BAR-096 拆槽，考题/壳层门面）：层缓冲 =
    /// 池内容宽 × 下池行高；page_x/page_y/page_denom = 框在页上原位坐标
    /// 与页渐变分母（保真参照）
    #[allow(clippy::too_many_arguments)]
    fn paint_lower_cursor_layer(
        &self,
        buf: &mut [u32],
        cw: u32,
        ch: u32,
        page_x: i64,
        page_y: i64,
        page_denom: i64,
        accent: crate::ui::accent::AccentPair,
        row: Option<&crate::ui::cfg_page::RowView>,
    );
    /// 下池子目录行表 + 上池联动下拉触发器/字段行/下拉 panel。
    /// cfg_off_x 语义同 paint_cfg_dual_pool；画在双池框之上。
    /// now_ms = 动画时钟（十四修 §六：跳框动效预览的相位源；无动画
    /// 预览时本参不读）。pan_upper_hold（十九修 D8）：Upper 平移 hold
    /// 期上池行不进主画布——带内静物=池内芯（GLES 烘焙路径用；
    /// softbuffer 双代同画路径恒 false 不受影响）
    #[allow(clippy::too_many_arguments)]
    fn paint_cfg_pool_content(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ps: &crate::ui::dual_pool::DualPoolSnap,
        page: &crate::ui::cfg_page::CfgPageSnap,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
        now_ms: u64,
        pan_upper_hold: bool,
        // BAR-096 拆层：true = 选中全包框由下池光标层提供（GLES）；
        // softbuffer 兜底传 false（整页自带光标）
        skip_cursor: bool,
    );
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
    fn render_into(&mut self, buf: &mut [u32], w: u32, h: u32, card_bottom_inset: u32) {
        TermView::render_into(self, buf, w, h, card_bottom_inset)
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
    fn text_width(&self, text: &str, px: f32) -> u32 {
        TermView::text_width(self, text, px)
    }
    fn render_keybar(&self, buf: &mut [u32], w: u32, h: u32, ime_bottom: u32, mods: u8) {
        TermView::render_keybar(self, buf, w, h, ime_bottom, mods)
    }
    fn paint_cfg_tab_bar(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        snap: &crate::ui::tab_bar::TabBarSnap,
        cfg_off_x: i32,
        bottom_inset: u32,
        accent: crate::ui::accent::AccentPair,
    ) {
        // BAR-096 拆层：整页语义 = y_shift 0（标签行顶就是页坐标）——
        // cfg_off 由合成期 placement 承担、bottom_inset 经 snap.line_span
        // 由壳层预填（层缓冲不知屏高）
        let _ = (cfg_off_x, bottom_inset);
        TermView::paint_tab_bar_layer(self, buf, w, h, 0, snap, accent)
    }
    fn paint_cfg_dual_pool(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        snap: &crate::ui::dual_pool::DualPoolSnap,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
    ) {
        TermView::paint_cfg_dual_pool_impl(self, buf, w, h, snap, cfg_off_x, accent)
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_tab_bar_layer(
        &self,
        buf: &mut [u32],
        cw: u32,
        ch: u32,
        y_shift: i64,
        snap: &crate::ui::tab_bar::TabBarSnap,
        accent: crate::ui::accent::AccentPair,
    ) {
        TermView::paint_tab_bar_layer(self, buf, cw, ch, y_shift, snap, accent)
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_lower_cursor_layer(
        &self,
        buf: &mut [u32],
        cw: u32,
        ch: u32,
        page_x: i64,
        page_y: i64,
        page_denom: i64,
        accent: crate::ui::accent::AccentPair,
        row: Option<&crate::ui::cfg_page::RowView>,
    ) {
        TermView::paint_lower_cursor_layer(
            self, buf, cw, ch, page_x, page_y, page_denom, accent, row,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_cfg_pool_content(
        &self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        ps: &crate::ui::dual_pool::DualPoolSnap,
        page: &crate::ui::cfg_page::CfgPageSnap,
        cfg_off_x: i32,
        accent: crate::ui::accent::AccentPair,
        now_ms: u64,
        pan_upper_hold: bool,
        skip_cursor: bool,
    ) {
        TermView::paint_cfg_pool_content_impl(
            self,
            buf,
            w,
            h,
            ps,
            page,
            cfg_off_x,
            accent,
            now_ms,
            pan_upper_hold,
            skip_cursor,
        )
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
