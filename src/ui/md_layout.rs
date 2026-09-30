//! ui/md_layout.rs — md 排版层（BAR-169 md 渲染器一期；BAR-204 换芯网格
//! 文字引擎）：块流 → 几何。**尺子 = `grid_text::char_cells` × 实例格
//! 步进**（排版与涂装同源唯一纯函数——涂装侧 pen 累进直调
//! `grid_stepped_w`，不许另写尺子）；唯一可调维 = 实例格
//! (cell_w, cell_h)（a 案：pinch 双指缩放一统，渲染字号/行距档体系与
//! 全局样式态同单废除）。行高 = cell_h × 档 scale × 宪法行距 上取整咬
//! 实例半格网；块隙/缩进档/框量全读 demo_page 宪法常量表不动。
//!
//! **折行条款落地**：正文/标题按内容宽（px）折行——文字量宽 =
//! char_cells 逐字格数 × 步进（满即断、刚好放下不断，同
//! modal::wrap_text 律，尺子从像素实量换格步进）；H1 每行横带宽随该行
//! 字宽（涂装读本册存的行宽，不重算）。代码围栏不折行（涂装右缘断墨，
//! 与 demo 页同取舍）。
//!
//! 消费关系：md_parse 出事件流 → 本册 Collector 收块 → 几何 pass 折行
//! 定高。纯逻辑零平台依赖（CELL_W/CELL_H 常量同源 modal.rs 先例）。

use crate::ui::demo_page::{self as dp, BlockKind, SegStyle};
use crate::ui::grid_text::{char_cells, grid_text_cells};
use crate::ui::md_parse::{self, MdSink, Span};

/// 1× 缺省等价锚的编译期钉（BAR-204 新约：基准格 18×36 时正文行带 =
/// 3 个半格 = 旧 36px/1.4 版面——宪法常量表动 = 这里撞；逐值等旧的
/// 运行期钉在 tests/md_layout_spec.rs spec_bar204_01）
const _: () = {
    assert!(crate::termview::CELL_W == dp::HU);
    assert!(crate::termview::CELL_H == dp::HU * 2);
    assert!(dp::BODY_PX as u32 == crate::termview::CELL_H);
    assert!((dp::LINE_RATIO * 100.0) as u32 == 140);
};

/// 一档的派生尺：步进（px/格）= cell_w × scale；行高（px）=
/// cell_h × scale × 宪法行距 上取整咬实例半格网（line_h 现式的 HU 换
/// 实例 cell_h/2——1× 缺省下逐值等旧，考题钉死）
pub fn line_h_grid(cell_h: u32, scale: f32) -> u32 {
    let hu = cell_h.max(2) as f32 / 2.0;
    let n = (cell_h as f32 * scale * dp::LINE_RATIO / hu).ceil();
    (n * hu).round() as u32
}

/// 段串格步进宽（px）：总格数 × 步进取整——**排版行宽与涂装 pen 累进
/// 的同源唯一源**（BAR-204；md_paint 直调本件，同源性钉死）
pub fn grid_stepped_w(text: &str, step_unit: f32) -> u32 {
    (grid_text_cells(text) as f32 * step_unit).round() as u32
}

/// 一行折行结果：行内段列 + 实量行宽（px，H1 横带随字宽与涂装右裁同读）
#[derive(Debug, Clone, PartialEq)]
pub struct MdLine {
    pub spans: Vec<Span>,
    pub w: u32,
    /// 列表项首行标记（涂装 ▪ 符只挂项首行，折行续行不挂；其余块型恒假）
    pub item_start: bool,
}

/// 一块的几何+内容（y/h 相对文档画布原点，行带咬实例半格网）
#[derive(Debug, Clone, PartialEq)]
pub struct MdBlock {
    pub kind: BlockKind,
    pub y: u32,
    pub h: u32,
    pub line_h: u32,
    /// 档位缩放（相对正文：H1=1.7/H2=1.45/H3=1.2/代码=30/36，余 1.0）——
    /// 步进 = 实例 cell_w × scale、字形 px = grid_fit × scale（涂装侧读）
    pub scale: f32,
    /// 折行后的视觉行（Code 块 = 字面行不折行）
    pub lines: Vec<MdLine>,
}

/// 整文档排版结果
#[derive(Debug, Clone, PartialEq)]
pub struct MdLayout {
    pub blocks: Vec<MdBlock>,
    /// 文档全高（块高累加 + 块隙；滚动上限的分子）
    pub total_h: u32,
}

/// 折行（格尺）：spans 展平成带样式的字符流，贪心装满 content_w_px
/// 即断（满即断、刚好放下不断——同 modal::wrap_text 律，尺子 =
/// char_cells × step_unit）；空行 = 一行空占位不塌。行宽 = 该行各段
/// 格步进宽之和（与涂装 pen 累进同尺——涂装按段画，段宽即
/// grid_stepped_w(段串)）
fn wrap_spans(spans: &[Span], content_w_px: u32, step_unit: f32) -> Vec<MdLine> {
    if content_w_px == 0 {
        return vec![MdLine {
            spans: vec![(SegStyle::Normal, String::new())],
            w: 0,
            item_start: false,
        }];
    }
    let mut lines: Vec<MdLine> = Vec::new();
    let mut cur: Vec<Span> = Vec::new(); // 当前行的段列（同运行样式并入尾段）
    let mut cur_w = 0.0f32;
    for (style, text) in spans {
        for c in text.chars() {
            let cw = char_cells(c) as f32 * step_unit;
            if cur_w + cw > content_w_px as f32 && !cur.is_empty() {
                let w = line_w(&cur, step_unit);
                lines.push(MdLine {
                    spans: std::mem::take(&mut cur),
                    w,
                    item_start: false,
                });
                cur_w = 0.0;
            }
            match cur.last_mut() {
                Some((s, t)) if *s == *style => t.push(c),
                _ => cur.push((*style, c.to_string())),
            }
            cur_w += cw;
        }
    }
    let w = line_w(&cur, step_unit);
    lines.push(MdLine {
        spans: cur,
        w,
        item_start: false,
    });
    lines
}

/// 行实量宽 = 各段格步进宽之和（与涂装 pen 累进同尺）
fn line_w(spans: &[Span], step_unit: f32) -> u32 {
    spans
        .iter()
        .map(|(_, t)| grid_stepped_w(t, step_unit))
        .sum()
}

/// 收块夹具（MdSink 消费端①：排版层；demo 页二期是②）
#[derive(Default)]
struct Collector {
    heads: Vec<(u8, Vec<Span>)>,
    paras: Vec<Vec<Vec<Span>>>,
    codes: Vec<Vec<String>>,
    quotes: Vec<Vec<Vec<Span>>>,
    lists: Vec<Vec<Vec<Span>>>,
    hrs: u32,
    order: Vec<Ev>,
}

#[derive(Debug, Clone)]
enum Ev {
    Head(usize),
    Para(usize),
    Code(usize),
    Quote(usize),
    List(usize),
    Hr,
}

impl MdSink for Collector {
    fn heading(&mut self, level: u8, spans: Vec<Span>) {
        self.heads.push((level, spans));
        self.order.push(Ev::Head(self.heads.len() - 1));
    }
    fn paragraph(&mut self, lines: Vec<Vec<Span>>) {
        self.paras.push(lines);
        self.order.push(Ev::Para(self.paras.len() - 1));
    }
    fn code_block(&mut self, lines: Vec<String>) {
        self.codes.push(lines);
        self.order.push(Ev::Code(self.codes.len() - 1));
    }
    fn quote(&mut self, lines: Vec<Vec<Span>>) {
        self.quotes.push(lines);
        self.order.push(Ev::Quote(self.quotes.len() - 1));
    }
    fn list(&mut self, items: Vec<Vec<Span>>) {
        self.lists.push(items);
        self.order.push(Ev::List(self.lists.len() - 1));
    }
    fn hr(&mut self) {
        self.hrs += 1;
        self.order.push(Ev::Hr);
    }
}

/// 整文档排版（纯函数）：content_w_px = 内容视口宽（px）；cell = 实例格
/// (cell_w, cell_h)（pinch 现值——排版结果由内容宽与实例格两维唯一决定，
/// 缓存 sig 吃这两维）。块列自上而下，块隙 BLOCK_GAP；空文档 = 一行空
/// 正文占位（不塌，与旧 viewer_fields 空行占位同取舍）
pub fn layout_md(text: &str, content_w_px: u32, cell: (u32, u32)) -> MdLayout {
    let mut col = Collector::default();
    md_parse::parse_md(text, &mut col);

    let (cell_w, cell_h) = (cell.0.max(1), cell.1.max(2));
    // 档位派生尺：(步进 px/格, 行高 px)
    let gear = |scale: f32| (cell_w as f32 * scale, line_h_grid(cell_h, scale));
    let (body_step, body_lh) = gear(1.0);
    let (code_step, code_lh) = gear(dp::CODE_PX / dp::BODY_PX);
    let mut blocks: Vec<MdBlock> = Vec::new();
    let mut y = 0u32;
    macro_rules! push {
        ($kind:expr, $h:expr, $lh:expr, $scale:expr, $lines:expr) => {{
            blocks.push(MdBlock {
                kind: $kind,
                y,
                h: $h,
                line_h: $lh,
                scale: $scale,
                lines: $lines,
            });
            y += $h + dp::BLOCK_GAP;
        }};
    }

    for ev in &col.order {
        match ev {
            Ev::Head(i) => {
                let (level, spans) = &col.heads[*i];
                let (kind, scale) = match level {
                    1 => (BlockKind::H1, dp::H1_SCALE),
                    2 => (BlockKind::H2, dp::H2_SCALE),
                    3 => (BlockKind::H3, dp::H3_SCALE),
                    // H4-H6 正文字号文字档（宪法 §三）
                    4 => (BlockKind::H4, 1.0),
                    5 => (BlockKind::H5, 1.0),
                    _ => (BlockKind::H6, 1.0),
                };
                let (step, lh) = gear(scale);
                // H1 折行宽 = 内容宽 − 框内缩 − 收尾（横带随字宽不吃满）；
                // 块高 = 上垫 + 各行带 + 下垫（多行 H1 行带累加）
                let wrap_w = if *level == 1 {
                    content_w_px.saturating_sub(dp::HEAD_TEXT_INSET + dp::HEAD_TOP_TAIL)
                } else {
                    content_w_px
                };
                let lines = wrap_spans(spans, wrap_w, step);
                let n = lines.len() as u32;
                let h = if *level == 1 {
                    dp::HU + lh * n + dp::HU
                } else {
                    lh * n
                };
                push!(kind, h, lh, scale, lines);
            }
            Ev::Para(i) => {
                let mut lines = Vec::new();
                for spans in &col.paras[*i] {
                    lines.extend(wrap_spans(spans, content_w_px, body_step));
                }
                let n = lines.len() as u32;
                push!(BlockKind::Body, body_lh * n, body_lh, 1.0, lines);
            }
            Ev::Code(i) => {
                // 围栏不折行：字面行原样（涂装右缘断墨同 demo 取舍）
                let lines = col.codes[*i]
                    .iter()
                    .map(|l| MdLine {
                        spans: vec![(SegStyle::Normal, l.clone())],
                        w: grid_stepped_w(l, code_step),
                        item_start: false,
                    })
                    .collect::<Vec<_>>();
                let n = lines.len().max(1) as u32;
                let lines = if lines.is_empty() {
                    vec![MdLine {
                        spans: vec![(SegStyle::Normal, String::new())],
                        w: 0,
                        item_start: false,
                    }]
                } else {
                    lines
                };
                push!(
                    BlockKind::Code,
                    dp::HU + code_lh * n + dp::HU,
                    code_lh,
                    dp::CODE_PX / dp::BODY_PX,
                    lines
                );
            }
            Ev::Quote(i) => {
                let mut lines = Vec::new();
                for spans in &col.quotes[*i] {
                    lines.extend(wrap_spans(
                        spans,
                        content_w_px.saturating_sub(dp::INDENT_W),
                        body_step,
                    ));
                }
                let n = lines.len() as u32;
                push!(BlockKind::Quote, body_lh * n, body_lh, 1.0, lines);
            }
            Ev::List(i) => {
                let mut lines = Vec::new();
                for spans in &col.lists[*i] {
                    let mut item_lines = wrap_spans(
                        spans,
                        content_w_px.saturating_sub(dp::LIST_TEXT_INSET),
                        body_step,
                    );
                    if let Some(first) = item_lines.first_mut() {
                        first.item_start = true; // ▪ 符只挂项首行
                    }
                    lines.extend(item_lines);
                }
                let n = lines.len() as u32;
                push!(BlockKind::List, body_lh * n, body_lh, 1.0, lines);
            }
            Ev::Hr => {
                push!(BlockKind::Hr, dp::HU * 2, dp::HU * 2, 1.0, Vec::new());
            }
        }
    }
    if blocks.is_empty() {
        // 空文档 = 一行空正文占位（不塌）
        push!(
            BlockKind::Body,
            body_lh,
            body_lh,
            1.0,
            vec![MdLine {
                spans: vec![(SegStyle::Normal, String::new())],
                w: 0,
                item_start: false,
            }]
        );
    }
    let total_h = y.saturating_sub(dp::BLOCK_GAP); // 尾块后不欠隙
    MdLayout { blocks, total_h }
}

// ---- 排版缓存（BAR-208 阅读页滚动病灶根修，2026-09-30；BAR-204 键
// 第三/四维换实例格 (cell_w, cell_h)——pinch 变格必重排版）----
//
// 排版只随 (文本代, 内容宽, 实例格两维) 失效——**滚动不动文本代**。旧路
// 帧内消费方（帧泵/拖拽/甩尾/涂装）各自 `text.clone()` + 全文重排：
// 一次滚动步 = 数次 2MB 克隆 + 数次全量 layout_md（用户真机「滑到约
// 三分之一卡一下静止」的结构性本体；demo 页同病）。缓存后同代共读
// 一份 Arc，重排只在新块/改宽/变格时发生——终端视口模型：内容
// 一次排版，视口滑动零重排。
//
// 锁纪律：LAYOUT_CACHE 是叶子锁（持它绝不取他锁），term→reader 红线
// 不受影响。网格尺是纯函数（量宽不需要 term 锁），持 reader 锁直调
// 本族函数安全。
/// 缓存键（文本代, 内容宽, cell_w, cell_h）
type CacheKey = (u64, u32, u32, u32);
/// 柜件 = 键 + 排版结果（共读 Arc）
type CacheEntry = (u64, u32, u32, u32, std::sync::Arc<MdLayout>);
static LAYOUT_CACHE: std::sync::Mutex<Option<CacheEntry>> = std::sync::Mutex::new(None);

/// 缓存键（文本代, 内容宽, cell_w, cell_h）
fn cache_key(txt_gen: u64, content_w_px: u32, cell: (u32, u32)) -> CacheKey {
    (txt_gen, content_w_px, cell.0, cell.1)
}

/// 只查不排（帧内高频路径：滚动拖拽/甩尾/帧泵每事件一问）：命中 =
/// Arc 克隆 O(1)；未中 = None（调用方走 cached 全路）
pub fn layout_md_peek(
    txt_gen: u64,
    content_w_px: u32,
    cell: (u32, u32),
) -> Option<std::sync::Arc<MdLayout>> {
    let g = LAYOUT_CACHE.lock().unwrap();
    match &*g {
        Some((g0, w0, cw0, ch0, lay))
            if (*g0, *w0, *cw0, *ch0) == cache_key(txt_gen, content_w_px, cell) =>
        {
            Some(lay.clone())
        }
        _ => None,
    }
}

/// 查 + 未中全量排版入柜（低频路径：新块回执/改宽/变格后的第一问）
pub fn layout_md_cached(
    txt_gen: u64,
    text: &str,
    content_w_px: u32,
    cell: (u32, u32),
) -> std::sync::Arc<MdLayout> {
    let key = cache_key(txt_gen, content_w_px, cell);
    let mut g = LAYOUT_CACHE.lock().unwrap();
    if let Some((g0, w0, cw0, ch0, lay)) = &*g
        && (*g0, *w0, *cw0, *ch0) == key
    {
        return lay.clone();
    }
    let lay = std::sync::Arc::new(layout_md(text, content_w_px, cell));
    *g = Some((key.0, key.1, key.2, key.3, lay.clone()));
    lay
}
