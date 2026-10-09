//! mail_page.rs — 信箱页状态核 + 可变高三栏卡几何（BAR-214，2026-10-01
//! 用户拍板：信箱入口点开的不再是压暗层弹框，而是与 AI 页/池卡/文件树卡/
//! 解析页同级的整页卡——右缘家，从解析页右边向左平移挤入；出场唯一 =
//! 右滑平移退出，中央终端页平移回来）。
//!
//! 条目 = 三级框信渲染器（用户原话逐条兑现）：
//! - **第一栏（最上）**：发信人 H1 半包框标题（剥部门留二字名）居左，
//!   时间居右（灰字、不带时区），正文字箭头 + 正文字收件人；两个收件人
//!   上下并置（本栏随收件人数长行），栏内字上下居中。
//! - **第二栏**：字头标题，引用灰字样式，超长格折行。
//! - **第三栏**：摘要，正文样式，超长格折行。
//! - **第四栏（BAR-222，NA0152）**：楼层渲染，每楼三行——`> N楼` 引用行
//!   （时间居右灰字无时区）/ 层主 H2 + 白正文箭头 + 被回复者 / 评论正文
//!   折行；与前三栏同律不截断，楼数楼文有多少卡就长多少。
//! - **第五件（BAR-244，NA0173）**：楼层 >2 的信卡默认只显前 2 楼，
//!   卡底多一行「展开余下 N 楼」控件；点开全楼长出（200-300ms ease-out，
//!   panel_ease_pos 同族曲线），展开态第一楼上方另有「收起」控件。
//!   核心不变量 = 被点控件行屏幕 y 动画全程不动（锚定补偿：新内容全在
//!   锚上方长 → offset_bottom 账不变；锚下方消失 → 通用补偿式钳回）。
//!   动画中途再点 = 从当前进度重定基反向，不跳。
//! - **渲染器不设最大高度**——卡高 = 各栏实量之和。
//! - **按发信人取色**：accent::accent_for_sender（同名同色/异名异色/
//!   与页色脱撞），渐变暗底 + 框吃发信人双色；点开三级框的查看器跳框
//!   颜色继承本卡（ViewerSnap.accent 维，壳穿线）。
//!
//! 滚动/追底/摘要懒加载窗语义与 mail_list 同款（旧 veil 列表卡退役，
//! 本册是其整页卡形态的继承者）；数据 = mail_feed 零改动。
//!
//! 本册 = 状态核 + 几何/命中/信头纯件（A 档，tests/mail_page_spec 钉死）；
//! 涂装在 termview（眼手同尺同一份 lays）。

use crate::mail_feed::{MailEntry, MailKey};
use crate::termview::{CELL_H, CELL_W};
use crate::ui::demo_page as dp;
use crate::ui::dual_pool::PoolRect;
use crate::ui::grid_text::grid_wrap;

// ---- 几何常量（网格制；涂装按实例格同尺换算）----
//
// BAR-221：本页几何吃实例格（pinch 联动）——各 const 是默认格
// (CELL_W, CELL_H) 下的值，留作同源对表基准；运行期几何一律走
// `metrics_of(cell)` 现算（页态存当前格，pinch/涂装两路喂，见
// note_cell）。考题钉：默认格 metrics == 本表逐值；实例格变 → 几何变。

/// 条目卡内上下留白 1 格
pub const ITEM_PAD_V: u32 = CELL_H;
/// 条目卡内左右内缩 1 格
pub const ITEM_PAD_H: u32 = CELL_W;
/// 栏间距 0.5 格
pub const ROW_GAP: u32 = CELL_H / 2;
/// 条目卡间距 1 格
pub const ITEM_GAP: u32 = CELL_H;
/// 正文体行高 54px = line_h_grid(CELL_H, 1.0)（f32 ceil/round 非 const，
/// 值钉死 + 考题运行期与 line_h_grid 对表同源）
pub const BODY_LH: u32 = 54;
/// H1 行高 90px = line_h_grid(CELL_H, dp::H1_SCALE=1.7)（对表同上）
pub const H1_LH: u32 = 90;
/// H2 行高 90px = line_h_grid(CELL_H, dp::H2_SCALE=1.45)（对表同上；
/// 与 H1 同值是公式巧合不是笔误——对表钉防的是「改 scale 忘改这里」）
pub const H2_LH: u32 = 90;
/// H1 半包框块高 = 上垫 + 行带 + 下垫（md_layout H1 单行为期同式）
pub const H1_BLOCK_H: u32 = dp::HU + H1_LH + dp::HU;
/// 标题栏引用缩进（格）：左竖线 + 1 格缩进（md 引用同尺）
pub const TITLE_INDENT_CELLS: u32 = 2;

/// 实例格几何账（BAR-221）：一切吃格几何量的运行期唯一源。
/// 公式 = 上方 const 定义的参数化（1 格 = cell、半格 = cell/2、
/// 行高 = line_h_grid(cell_h, scale)）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    pub cell_w: u32,
    pub cell_h: u32,
    pub item_pad_v: u32,
    pub item_pad_h: u32,
    pub row_gap: u32,
    pub item_gap: u32,
    pub body_lh: u32,
    pub h1_lh: u32,
    pub h1_block_h: u32,
    /// H2 行高（BAR-222 第四栏层主行）= line_h_grid(cell_h, H2_SCALE)
    pub h2_lh: u32,
}

pub fn metrics_of(cell: (u32, u32)) -> Metrics {
    let (cw, ch) = (cell.0.max(1), cell.1.max(2));
    let hu = ch / 2;
    let body_lh = crate::ui::md_layout::line_h_grid(ch, 1.0);
    let h1_lh = crate::ui::md_layout::line_h_grid(ch, dp::H1_SCALE);
    Metrics {
        cell_w: cw,
        cell_h: ch,
        item_pad_v: ch,
        item_pad_h: cw,
        row_gap: hu,
        item_gap: ch,
        body_lh,
        h1_lh,
        h1_block_h: hu + h1_lh + hu,
        h2_lh: crate::ui::md_layout::line_h_grid(ch, dp::H2_SCALE),
    }
}

/// 默认格几何账（考题/回退路径用）
pub fn default_metrics() -> Metrics {
    metrics_of((CELL_W, CELL_H))
}

/// 摘要占位（懒加载两态词面，沿用 mail_list 旧约）
pub const SUMMARY_PENDING: &str = "（摘要待取）";
pub const SUMMARY_EMPTY: &str = "（无摘要）";

// ---- 信头纯件（剥部门/拆收件人/时间去时区）----

/// 剥部门留名（用户拍板：不要部门，就二字名）：串内含「部」且剥完
/// 还剩 ≥2 字 → 剥到第一个「部」（含）为止；否则原样（「全体」无部
/// 字天然原样）。
pub fn strip_dept(name: &str) -> String {
    if let Some(pos) = name.find('部') {
        let rest = &name[pos + '部'.len_utf8()..];
        if rest.chars().count() >= 2 {
            return rest.to_string();
        }
    }
    name.to_string()
}

/// 拆收件人：「、」为主分隔（兼容 ，,；;），逐段 trim 去空，逐段剥部门。
/// 两个收件人上下并置 = 本 Vec 逐行画；空串 = 一空 Vec（涂装不画箭头列）。
pub fn split_recipients(to: &str) -> Vec<String> {
    to.split(['、', '，', ',', '；', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(strip_dept)
        .collect()
}

/// 时间去时区（用户拍板：不要时区）：「2026-09-30 08:00 +08:00」→
/// 「2026-09-30 08:00」——取前两段（日期 + 时分）；本就无第三段原样。
pub fn fmt_time(time: &str) -> String {
    let mut it = time.split(' ');
    match (it.next(), it.next()) {
        (Some(d), Some(t)) if !d.is_empty() && !t.is_empty() => format!("{d} {t}"),
        _ => time.to_string(),
    }
}

// ---- 条目排版（可变高：卡高 = 三栏实量之和，不设最大高度）----

/// 一楼的排版账（BAR-222，NA0152 §三 三行形制：引用行/层主行/正文，
/// 同律不截断——正文折行全账，楼数与楼文长多少卡就长多少）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorLay {
    /// 行①（`> N楼` 引用行）卡内相对 y（行高 metrics body_lh，时间居右同行）
    pub quote_y: u32,
    /// 行②（层主 H2 + 箭头 + 被回复者）卡内相对 y（行高 metrics h2_lh）
    pub who_y: u32,
    /// 行③（评论正文）首行卡内相对 y + 折行区间（char 下标，对 body）
    pub body_y: u32,
    pub body_lines: Vec<(usize, usize)>,
}

/// 一信的排版账（涂装直读；折行区间 = char 下标，与 grid_wrap 同约）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLay {
    /// 卡高（px）
    pub h: u32,
    /// 内容流内卡顶（px，前缀和）
    pub top: i64,
    /// 第一栏（卡内相对 y / 高）：高 = max(H1 块, 收件人行数 × BODY_LH)
    pub meta_y: u32,
    pub meta_h: u32,
    /// 收件人（已拆已剥部门，涂装逐行画）
    pub recipients: Vec<String>,
    /// 第二栏标题折行区间（≥1 行；空标题 = 单行空）
    pub title_lines: Vec<(usize, usize)>,
    pub title_y: u32,
    /// 第三栏摘要折行区间（≥1 行）
    pub sum_lines: Vec<(usize, usize)>,
    pub sum_y: u32,
    /// 第四栏楼层（每楼一份三行账；空表 = 无楼层，卡高不增）
    pub floors: Vec<FloorLay>,
    /// 顶控件（BAR-244，仅楼层 >2）：(控件行卡内 y = 摘要栏底+row_gap 最终位,
    /// 块分配高 A1 = round(p×(row_gap+ctrl_h)))；p=0 块消失 = None。
    /// 涂装裁剪带 = [行y−row_gap, 行y−row_gap+A1)
    pub ctrl_top: Option<(u32, u32)>,
    /// 底控件行卡内 y（仅楼层 >2；行高恒 ctrl_h=body_lh，折叠态紧跟
    /// floor2+row_gap，展开态在全楼之后——同一行位随 p 连续移动）
    pub ctrl_bottom: Option<u32>,
    /// floors[2..] 涂装裁剪带（仅楼层 >2）：(region_y0 = floor2底+row_gap,
    /// alloc = A2 = round(p×extra_full))；涂装与带取交，全量 floors 照排
    pub extra_band: Option<(u32, u32)>,
}

/// 条目文宽（格）：卡宽 − 左右内缩，按实例格宽折算（BAR-221）
pub fn text_cells_of(item_w: u32, m: &Metrics) -> u32 {
    item_w.saturating_sub(m.item_pad_h * 2) / m.cell_w
}

/// 显示标题（缺字头标题回落信名——涂装与排版同一把尺，单源）
pub fn display_title(e: &MailEntry) -> &str {
    if e.title.is_empty() {
        &e.name
    } else {
        &e.title
    }
}

/// 排一信（纯函数）：text_cells = 卡内文宽（格）。折行宽保底 4 格防
/// 零宽死循；行数下限各栏 1 行。几何吃 metrics（实例格，BAR-221）。
/// fx_p = 楼层展开进度 ∈[0,1]（BAR-244）：仅楼层 >2 生效——0 = 折叠
/// （前 2 楼 + 底控件），1 = 全展开（全楼 + 顶/底双控件），中间值 =
/// 动画帧（顶控件块 A1 与 floors[2..] 露出带 A2 随 p 线性长，控件行位
/// 连续移动）；n≤2 恒全排无控件（fx_p 忽略，卡高与旧约逐值同）
pub fn lay_item(e: &MailEntry, top: i64, text_cells: u32, m: &Metrics, fx_p: f32) -> ItemLay {
    let recipients = split_recipients(&e.to);
    let n_rcp = recipients.len().max(1) as u32;
    let meta_h = m.h1_block_h.max(n_rcp * m.body_lh);
    let title_cells = text_cells.saturating_sub(TITLE_INDENT_CELLS).max(4);
    let body_cells = text_cells.max(4);
    let mut title_lines = grid_wrap(display_title(e), title_cells);
    if title_lines.is_empty() {
        title_lines.push((0, 0));
    }
    let sum_text: &str = match &e.summary {
        Some(s) if !s.is_empty() => s,
        Some(_) => SUMMARY_EMPTY,
        None => SUMMARY_PENDING,
    };
    let mut sum_lines = grid_wrap(sum_text, body_cells);
    if sum_lines.is_empty() {
        sum_lines.push((0, 0));
    }
    let meta_y = m.item_pad_v;
    let title_y = meta_y + meta_h + m.row_gap;
    let sum_y = title_y + title_lines.len() as u32 * m.body_lh + m.row_gap;
    // 第四栏（BAR-222）：每楼 = 引用行(body_lh) + 层主行(h2_lh) + 正文
    // 折行(≥1 行)，楼间 row_gap；与前三栏同一份文宽（正文不缩进）
    let mut floors = Vec::new();
    let lay_floor = |f: &crate::mail_feed::MailFloor, y: &mut u32, floors: &mut Vec<FloorLay>| {
        *y += m.row_gap;
        let quote_y = *y;
        let who_y = quote_y + m.body_lh;
        let body_y = who_y + m.h2_lh;
        let mut body_lines = grid_wrap(&f.display_text(), body_cells);
        if body_lines.is_empty() {
            body_lines.push((0, 0));
        }
        *y = body_y + body_lines.len() as u32 * m.body_lh;
        floors.push(FloorLay {
            quote_y,
            who_y,
            body_y,
            body_lines,
        });
    };
    let sum_end = sum_y + sum_lines.len() as u32 * m.body_lh;
    if e.floors.len() > 2 {
        // BAR-244 折叠/展开排版：顶控件块 A1 长在摘要栏底与 floor1 之间；
        // floor1/2 全高照排（随 A1 下移）；floors[2..] 按全展开最终位置
        // 排（涂装按 extra_band 裁剪露出）；底控件行恒占 ctrl_h，位置 =
        // floor2底 + row_gap + A2（折叠紧贴 floor2，展开在全楼之后）。
        // 守恒：p=0 布局 = 折叠定态逐像素相同（A1=A2=0 无间断跳）
        let p = fx_p.clamp(0.0, 1.0);
        let ctrl_h = m.body_lh;
        let a1 = (p * (m.row_gap + ctrl_h) as f32).round() as u32;
        let ctrl_top = if a1 > 0 {
            Some((sum_end + m.row_gap, a1))
        } else {
            None
        };
        let mut y = sum_end + a1;
        for f in &e.floors[..2] {
            lay_floor(f, &mut y, &mut floors);
        }
        let floor2_end = y;
        for f in &e.floors[2..] {
            lay_floor(f, &mut y, &mut floors);
        }
        let extra_full = y - floor2_end;
        let a2 = (p * extra_full as f32).round() as u32;
        let ctrl_bottom_y = floor2_end + m.row_gap + a2;
        let h = ctrl_bottom_y + ctrl_h + m.item_pad_v;
        return ItemLay {
            h,
            top,
            meta_y,
            meta_h,
            recipients,
            title_lines,
            title_y,
            sum_lines,
            sum_y,
            floors,
            ctrl_top,
            ctrl_bottom: Some(ctrl_bottom_y),
            extra_band: Some((floor2_end + m.row_gap, a2)),
        };
    }
    let mut y = sum_end;
    for f in &e.floors {
        lay_floor(f, &mut y, &mut floors);
    }
    let h = y + m.item_pad_v;
    ItemLay {
        h,
        top,
        meta_y,
        meta_h,
        recipients,
        title_lines,
        title_y,
        sum_lines,
        sum_y,
        floors,
        ctrl_top: None,
        ctrl_bottom: None,
        extra_band: None,
    }
}

/// 整册排版（折叠定态 p=0 流水，前缀和）：oldest_first 序，最新在底
pub fn lay_items(entries: &[MailEntry], text_cells: u32, m: &Metrics) -> Vec<ItemLay> {
    lay_items_fx(entries, text_cells, m, |_| 0.0)
}

/// 整册排版（逐信 fx 进度版）：fx_p 查不到 = 0.0（折叠定态）
pub fn lay_items_fx(
    entries: &[MailEntry],
    text_cells: u32,
    m: &Metrics,
    fx_p: impl Fn(&MailEntry) -> f32,
) -> Vec<ItemLay> {
    let mut top = 0i64;
    entries
        .iter()
        .map(|e| {
            let lay = lay_item(e, top, text_cells, m, fx_p(e));
            top += i64::from(lay.h) + i64::from(m.item_gap);
            lay
        })
        .collect()
}

/// 内容全高（n 卡 + (n-1) 距；0 = 0）
pub fn total_h(lays: &[ItemLay]) -> i64 {
    match lays.last() {
        None => 0,
        Some(l) => l.top + i64::from(l.h),
    }
}

/// 滚动上限（px）
pub fn scroll_max(lays: &[ItemLay], viewport_h: i64) -> i64 {
    (total_h(lays) - viewport_h).max(0)
}

/// 条目卡矩形（视口坐标 → 屏坐标；offset_bottom = 视口底距内容底 px，
/// 0 = 贴底 = 最新信贴视口底）——眼手同尺唯一源
pub fn item_rect(
    area: &PoolRect,
    vp: (i64, i64),
    lays: &[ItemLay],
    i: usize,
    offset_bottom: i64,
) -> PoolRect {
    let vp_h = vp.1 - vp.0;
    let scroll_top = (total_h(lays) - vp_h - offset_bottom).max(0);
    PoolRect {
        x: area.x,
        y: vp.0 + lays[i].top - scroll_top,
        w: area.w,
        h: lays[i].h,
    }
}

/// 可见条目区间（半卡在沿上的也画；裁剪归涂装断墨带）
pub fn visible_range(
    lays: &[ItemLay],
    viewport_h: i64,
    offset_bottom: i64,
) -> std::ops::Range<usize> {
    if lays.is_empty() || viewport_h <= 0 {
        return 0..0;
    }
    let scroll_top = (total_h(lays) - viewport_h - offset_bottom).max(0);
    let scroll_bot = scroll_top + viewport_h;
    let first = lays.partition_point(|l| l.top + i64::from(l.h) <= scroll_top);
    let last = lays.partition_point(|l| l.top < scroll_bot);
    first..last.max(first)
}

/// 摘要懒加载窗：可见窗上沿再向上扩 SUMMARY_LOOKBACK 屏，下沿 = 视口底
pub const SUMMARY_LOOKBACK: i64 = 2;
pub fn summary_window(
    lays: &[ItemLay],
    viewport_h: i64,
    offset_bottom: i64,
) -> std::ops::Range<usize> {
    if lays.is_empty() || viewport_h <= 0 {
        return 0..0;
    }
    let scroll_top = (total_h(lays) - viewport_h - offset_bottom).max(0);
    let lo_y = (scroll_top - SUMMARY_LOOKBACK * viewport_h).max(0);
    let first = lays.partition_point(|l| l.top + i64::from(l.h) <= lo_y);
    let last = lays.partition_point(|l| l.top < scroll_top + viewport_h);
    first..last.max(first)
}

/// 命中：视口带内点中条目卡 = Item(i)；点中控件行（全卡宽 × ctrl_h）
/// = FloorToggle（**先于 Item 判定**；顶控件仅全展开态可点——动画中途
/// 块高未满不派生命中）；页内其余 = Page（吞掉防穿透）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailPageHit {
    Item(usize),
    /// BAR-244 楼层展开/折叠控件：item = 条目下标，top = 顶控件（否则底）
    FloorToggle {
        item: usize,
        top: bool,
    },
    Page,
}

#[allow(clippy::too_many_arguments)]
pub fn hit(
    area: &PoolRect,
    vp: (i64, i64),
    lays: &[ItemLay],
    offset_bottom: i64,
    x: i64,
    y: i64,
    m: &Metrics,
) -> Option<MailPageHit> {
    let inside =
        |r: &PoolRect| x >= r.x && x < r.x + i64::from(r.w) && y >= r.y && y < r.y + i64::from(r.h);
    if y >= vp.0 && y < vp.1 {
        let ctrl_h = i64::from(m.body_lh);
        for i in visible_range(lays, vp.1 - vp.0, offset_bottom) {
            let r = item_rect(area, vp, lays, i, offset_bottom);
            if !inside(&r) {
                continue;
            }
            // BAR-244：控件行命中先于 Item
            if let Some(cy) = lays[i].ctrl_bottom {
                let y0 = r.y + i64::from(cy);
                if y >= y0 && y < y0 + ctrl_h {
                    return Some(MailPageHit::FloorToggle {
                        item: i,
                        top: false,
                    });
                }
            }
            if let Some((ty, alloc)) = lays[i].ctrl_top
                && alloc >= m.row_gap + m.body_lh
            {
                let y0 = r.y + i64::from(ty);
                if y >= y0 && y < y0 + ctrl_h {
                    return Some(MailPageHit::FloorToggle { item: i, top: true });
                }
            }
            return Some(MailPageHit::Item(i));
        }
    }
    Some(MailPageHit::Page)
}

// ---- 页级几何（整页卡内芯；reader_geom 同尺配方）----

/// 顶栏高（物理 px，与阅读页 TOP_BAR_H 同尺配方）
pub const TOP_BAR_H: i64 = 110;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailGeom {
    pub x0: i64,
    pub x1: i64,
    pub bar_y0: i64,
    pub bar_y1: i64,
    /// 分隔线 y（顶栏底）
    pub div_y: i64,
    /// 条目视口纵段
    pub view_y0: i64,
    pub view_y1: i64,
}

pub fn mail_geom(w: u32, h: u32, bottom_inset: u32) -> MailGeom {
    let (ox, oy) = crate::ui::tab_bar::content_origin();
    let x0 = i64::from(ox);
    // BAR-221：右缘让位吃实例格宽（pinch 联动；页态存格，见 note_cell）
    let cell_w = cur_cell().0;
    let x1 = i64::from(w)
        - i64::from(
            crate::termview::AI_PAGE_FRAME_MARGIN + crate::termview::AI_PAGE_FRAME_W + cell_w,
        );
    let bar_y0 = i64::from(oy);
    let bar_y1 = bar_y0 + TOP_BAR_H;
    let view_y0 = bar_y1 + 1;
    let view_y1 = crate::ui::parser_page::visible_bottom(h, bottom_inset).max(view_y0);
    MailGeom {
        x0,
        x1,
        bar_y0,
        bar_y1,
        div_y: bar_y1,
        view_y0,
        view_y1,
    }
}

/// 条目列矩形（item_rect 的 area 入参唯一源）
pub fn items_area(g: &MailGeom) -> PoolRect {
    PoolRect {
        x: g.x0,
        y: 0,
        w: (g.x1 - g.x0).max(0) as u32,
        h: 0,
    }
}

// ---- 楼层展开/折叠动画（BAR-244，NA0173）----

/// 楼层展开进度采样器（fx_ease::EaseState 同款重定基模式本地件）：
/// from→target 走 panel_ease_pos（进场 250ms power2_out/离场 180ms 镜像，
/// 在 200-300ms 建议窗内）；toggle = 采当前进度重定基反向——动画中途
/// 再点从当前进度反向、不跳（重定基语义天然连续）
#[derive(Debug, Clone)]
pub struct FloorFx {
    from: f32,
    target: f32,
    start_ms: u64,
    settled: bool,
}

impl FloorFx {
    pub fn new() -> Self {
        FloorFx {
            from: 0.0,
            target: 0.0,
            start_ms: 0,
            settled: true,
        }
    }

    /// 采样当前进度（顺手结清 settled：贴死 target = 终态）
    pub fn sample(&mut self, now_ms: u64) -> f32 {
        let pos = crate::ui::fx_ease::panel_ease_pos(
            self.from,
            self.target,
            now_ms.saturating_sub(self.start_ms),
        );
        self.settled = pos == self.target;
        pos
    }

    /// 翻转目标：从当前进度重定基反向（中途反向不跳）
    pub fn toggle(&mut self, now_ms: u64) {
        let pos = self.sample(now_ms);
        self.from = pos;
        self.target = 1.0 - self.target;
        self.start_ms = now_ms;
        self.settled = false;
    }

    pub fn settled(&self) -> bool {
        self.settled
    }
}

impl Default for FloorFx {
    fn default() -> Self {
        Self::new()
    }
}

/// 锚定控件别（BAR-244）：被点的那行屏幕 y 动画全程不动
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// 顶控件（收起用；内容在锚下方消失，补偿式钳回）
    Top,
    /// 底控件（展开用；新内容全在锚上方长，offset_bottom 账天然不动）
    Bottom,
}

// ---- 状态核（追底 = follow_tail 同款语义像素账版）----

#[derive(Debug, Clone)]
pub struct MailPageView {
    pub key: MailKey,
    offset_bottom: i64,
    /// 追底态：true = 恒贴底；上滑取消、回底恢复
    follow: bool,
    /// 排版缓存（壳每个烘焙轮喂；滚动态直接读）
    lays: Vec<ItemLay>,
    viewport_h: i64,
    epoch: u64,
    /// 当前实例格（BAR-221：pinch 联动——pinch 应用臂与涂装两路喂，
    /// 几何/排版全吃它；默认格零漂移）
    cell: (u32, u32),
    /// 楼层展开动画态（BAR-244）：key = MailEntry.name（同册内唯一）；
    /// 无条目 = p 0.0（折叠定态）
    floor_fx: Vec<(String, FloorFx)>,
    /// 锚定补偿（BAR-244）：(信名, 控件别, 控件行屏幕 y)；fx settle 后清
    anchor: Option<(String, AnchorKind, i64)>,
    /// 本代条目名序（sync_items 顺手存——toggle 按名找卡与 lays 同代）
    names: Vec<String>,
}

impl MailPageView {
    pub fn new(key: MailKey) -> Self {
        MailPageView {
            key,
            offset_bottom: 0,
            follow: true,
            lays: Vec::new(),
            viewport_h: 0,
            epoch: 0,
            cell: (CELL_W, CELL_H),
            floor_fx: Vec::new(),
            anchor: None,
            names: Vec::new(),
        }
    }

    pub fn offset_bottom(&self) -> i64 {
        self.offset_bottom
    }
    pub fn follow(&self) -> bool {
        self.follow
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn lays(&self) -> &[ItemLay] {
        &self.lays
    }

    /// 排版+布局写回（壳烘焙轮喂）：追底态恒贴底；内容缩水钳回上限。
    /// BAR-244：逐信采样楼层展开进度进排版（无条目 = p 0.0）；末尾
    /// 锚定补偿——anchor 在且该信找得到：new_scroll_top = 锚行新内容 y
    /// − 锚屏 y，offset_bottom = (total_h − viewport_h − new_scroll_top)
    /// .clamp(0, scroll_max)（clamp 撞边 = 内容不够补偿的极端边，锚漂
    /// 认了）；该信 fx settle 后清锚
    pub fn sync_items(
        &mut self,
        entries: &[MailEntry],
        text_cells: u32,
        viewport_h: i64,
        m: &Metrics,
        now_ms: u64,
    ) {
        self.names = entries.iter().map(|e| e.name.clone()).collect();
        let mut sampled: Vec<(String, f32)> = Vec::with_capacity(self.floor_fx.len());
        for (n, fx) in self.floor_fx.iter_mut() {
            sampled.push((n.clone(), fx.sample(now_ms)));
        }
        let p_of = |e: &MailEntry| {
            sampled
                .iter()
                .find(|(n, _)| n == &e.name)
                .map(|(_, p)| *p)
                .unwrap_or(0.0)
        };
        self.lays = lay_items_fx(entries, text_cells, m, p_of);
        self.viewport_h = viewport_h;
        let max = scroll_max(&self.lays, viewport_h);
        if self.follow {
            self.offset_bottom = 0;
        } else if self.offset_bottom > max {
            self.offset_bottom = max;
        }
        if let Some((name, kind, screen_y)) = self.anchor.clone() {
            if let Some(i) = self.names.iter().position(|n| n == &name) {
                let lay = &self.lays[i];
                let row_y = match kind {
                    // 顶控件行文本恒在最终位（摘要栏底+row_gap）——settle 帧
                    // 块已消失（ctrl_top None）也要照最终位补最后一刀，
                    // 否则末帧差值留在 offset 账里（锚漂一截）
                    AnchorKind::Top => lay.ctrl_top.map(|(y, _)| y).or_else(|| {
                        if lay.ctrl_bottom.is_some() {
                            Some(lay.sum_y + lay.sum_lines.len() as u32 * m.body_lh + m.row_gap)
                        } else {
                            None
                        }
                    }),
                    AnchorKind::Bottom => lay.ctrl_bottom,
                };
                if let Some(cy) = row_y {
                    let new_content_y = lay.top + i64::from(cy);
                    let new_scroll_top = new_content_y - screen_y;
                    let max = scroll_max(&self.lays, viewport_h);
                    self.offset_bottom =
                        (total_h(&self.lays) - viewport_h - new_scroll_top).clamp(0, max);
                    self.follow = self.offset_bottom == 0;
                }
            }
            let settled = self
                .floor_fx
                .iter()
                .find(|(n, _)| n == &name)
                .map(|(_, f)| f.settled)
                .unwrap_or(true);
            if settled {
                self.anchor = None;
            }
        }
    }

    /// 楼层展开/折叠翻转（BAR-244）：从当前 lays 算被点控件行屏幕 y
    /// （content_y − scroll_top；scroll_top = total_h − viewport_h −
    /// offset_bottom）存 anchor，再翻 FloorFx（重定基 = 中途反向不跳）。
    /// 找不到信/控件行不在（顶控件 p=0 无块）= 不动
    pub fn toggle_floors(&mut self, name: &str, kind: AnchorKind, now_ms: u64) -> bool {
        let Some(i) = self.names.iter().position(|n| n == name) else {
            return false;
        };
        let Some(lay) = self.lays.get(i) else {
            return false;
        };
        let row_y = match kind {
            AnchorKind::Top => lay.ctrl_top.map(|(y, _)| y),
            AnchorKind::Bottom => lay.ctrl_bottom,
        };
        let Some(cy) = row_y else { return false };
        let scroll_top = (total_h(&self.lays) - self.viewport_h - self.offset_bottom).max(0);
        let screen_y = lay.top + i64::from(cy) - scroll_top;
        match self.floor_fx.iter_mut().find(|(n, _)| n == name) {
            Some((_, fx)) => fx.toggle(now_ms),
            None => {
                let mut fx = FloorFx::new();
                fx.toggle(now_ms);
                self.floor_fx.push((name.to_string(), fx));
            }
        }
        self.anchor = Some((name.to_string(), kind, screen_y));
        self.epoch += 1;
        true
    }

    /// 任一楼层动画在飞（fx_spring 活性表第十路直读）
    pub fn fx_active(&self) -> bool {
        self.floor_fx.iter().any(|(_, f)| !f.settled)
    }

    /// 滚动（壳手势喂增量）：dy > 0 = 看更旧；回底 = 恢复追底。变了才
    /// bump（sig 鬼影纪律）
    pub fn scroll_by(&mut self, dy: i64) -> bool {
        let max = scroll_max(&self.lays, self.viewport_h);
        let ns = (self.offset_bottom + dy).clamp(0, max);
        if ns == self.offset_bottom {
            return false;
        }
        self.offset_bottom = ns;
        self.follow = ns == 0;
        self.epoch += 1;
        true
    }

    pub fn geo(&self) -> (i64, i64) {
        (self.viewport_h, self.offset_bottom)
    }
}

// ---- 全局句柄（reader_handle 同款：壳开收，涂装/命中取）----

static VIEW: std::sync::Mutex<Option<MailPageView>> = std::sync::Mutex::new(None);
static DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bump_dirty() {
    DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 开信箱页（召唤 Panel::Mail 时）：同册重开 = 归零追底
pub fn open(key: MailKey) {
    *VIEW.lock().unwrap() = Some(MailPageView::new(key));
    bump_dirty();
}

/// 收信箱页（离栈清理）
pub fn close() {
    if VIEW.lock().unwrap().take().is_some() {
        bump_dirty();
    }
}

pub fn open_key() -> Option<MailKey> {
    VIEW.lock().unwrap().as_ref().map(|v| v.key)
}

/// 页存活闸（BAR-220）：close 后 VIEW=None。退场滑出期涂装仍可见
/// （mail_draw=true）但内容不可能再变——烘焙槽以此闸空页重烘
pub fn is_open() -> bool {
    VIEW.lock().unwrap().is_some()
}

pub fn snap() -> Option<MailPageView> {
    VIEW.lock().unwrap().clone()
}

pub fn take_dirty() -> bool {
    DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// 当前实例格（页关着 = 默认格回退；BAR-221）
pub fn cur_cell() -> (u32, u32) {
    VIEW.lock()
        .unwrap()
        .as_ref()
        .map(|v| v.cell)
        .unwrap_or((CELL_W, CELL_H))
}

/// 当前几何账（排版/命中/涂装共用同一份——眼手同尺）
pub fn cur_metrics() -> Metrics {
    metrics_of(cur_cell())
}

/// 实例格写回（pinch 应用臂 + 涂装两路喂）：变了才记脏（sig 鬼影纪律）。
/// 页关着 = 无账可记，开页默认格起步、涂装首轮即喂正
pub fn note_cell(cell: (u32, u32)) {
    let mut g = VIEW.lock().unwrap();
    if let Some(v) = g.as_mut()
        && v.cell != cell
    {
        v.cell = cell;
        drop(g);
        bump_dirty();
    }
}

pub fn sync_items(
    entries: &[MailEntry],
    text_cells: u32,
    viewport_h: i64,
    m: &Metrics,
    now_ms: u64,
) {
    if let Some(v) = VIEW.lock().unwrap().as_mut() {
        v.sync_items(entries, text_cells, viewport_h, m, now_ms);
    }
}

/// 楼层展开/折叠翻转（BAR-244 点按臂）：页内自取 anchor 屏幕 y
pub fn toggle_floors(name: &str, kind: AnchorKind, now_ms: u64) -> bool {
    let mut g = VIEW.lock().unwrap();
    match g.as_mut() {
        Some(v) => {
            let r = v.toggle_floors(name, kind, now_ms);
            if r {
                bump_dirty();
            }
            r
        }
        None => false,
    }
}

/// 楼层动画活性（fx_frame_due 活性表第十路；本件状态在 ui 层，直读
/// VIEW，无需 ft_fling 那种壳旗）
pub fn fx_active() -> bool {
    VIEW.lock()
        .unwrap()
        .as_ref()
        .is_some_and(MailPageView::fx_active)
}

/// 帧泵拍（BAR-244）：有在飞动画才采样推进——采样结清 settled、
/// epoch+1（烘焙 sig 换代必重烘）+ 记脏；全 settled = false 零动作
pub fn tick_fx(now_ms: u64) -> bool {
    let mut g = VIEW.lock().unwrap();
    let Some(v) = g.as_mut() else { return false };
    if !v.fx_active() {
        return false;
    }
    for (_, fx) in v.floor_fx.iter_mut() {
        fx.sample(now_ms);
    }
    v.epoch += 1;
    drop(g);
    bump_dirty();
    true
}

pub fn scroll_by(dy: i64) -> bool {
    let mut g = VIEW.lock().unwrap();
    match g.as_mut() {
        Some(v) => {
            let r = v.scroll_by(dy);
            if r {
                bump_dirty();
            }
            r
        }
        None => false,
    }
}
