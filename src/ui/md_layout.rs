//! ui/md_layout.rs — md 排版层（BAR-169 md 渲染器一期）：块流 → 几何，
//! **尺子单源对齐 demo_page**（行高 = 1.4 倍字号上取整咬半格网、块隙
//! 0.5 格、缩进档全同源——demo_page 的常量表是唯一宪法读本，本册只
//! 参数化字号基准与行距两个可调项）。
//!
//! **折行条款落地**：正文/标题按内容宽（px）折行——文字量宽走
//! `MdMeasure` 通路（壳 = TermView::text_width 真字尺；考题 =  Mock
//! 定宽尺）；H1 每行横带宽随该行字宽（涂装读本册存的行宽，不重算）。
//! 代码围栏不折行（涂装右缘断墨，与 demo 页同取舍）。
//!
//! 消费关系：md_parse 出事件流 → 本册 Collector 收块 → 几何 pass 折行
//! 定高。纯逻辑零平台依赖（CELL_W/CELL_H 常量同源 modal.rs 先例）。

use crate::ui::demo_page::{self as dp, BlockKind, SegStyle};
use crate::ui::md_parse::{self, MdSink, Span};

/// 文字量宽通路（钉死：排版与涂装必须同一把尺——壳两侧都喂
/// TermView::text_width；考题喂 Mock 定宽尺）
pub trait MdMeasure {
    fn md_text_w(&self, text: &str, px: f32) -> u32;
}

/// 渲染样式可调项（设置页渲染配置卡的两枚旋钮；缺省 = demo_page 宪法值）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MdStyle {
    /// 正文字号基准（px）
    pub body_px: f32,
    /// 行距倍数（行高 = ratio 倍字号上取整咬半格网）
    pub line_ratio: f32,
}

impl Default for MdStyle {
    /// 宪法缺省锚：demo_page::BODY_PX / LINE_RATIO（行为零变化承诺——
    /// 不设渲染配置卡时的版面 = 打样规格）
    fn default() -> Self {
        MdStyle {
            body_px: dp::BODY_PX,
            line_ratio: dp::LINE_RATIO,
        }
    }
}

// ---- 全局样式口（壳单源：涂装/滚动上限/命中三处读同一份——
// sess_pool::snap / cfg_page_handle 同规静态口）----

/// 存整数百倍（f32 不进静态可变态的等值比较账；36px→3600、1.4→140）
static MD_STYLE: std::sync::Mutex<(u32, u32)> = std::sync::Mutex::new((3600, 140));

/// 设置页渲染配置卡写入（render.json 落盘由壳自理，本口只管内存态）
pub fn set_md_style(body_px: u32, line_ratio_pct: u32) {
    *MD_STYLE.lock().unwrap() = (body_px * 100, line_ratio_pct);
}

/// 当前渲染样式（排版/涂装/滚动上限同读；未设置 = 宪法缺省 36px/1.4）
pub fn md_style() -> MdStyle {
    let (px100, ratio_pct) = *MD_STYLE.lock().unwrap();
    MdStyle {
        body_px: px100 as f32 / 100.0,
        line_ratio: ratio_pct as f32 / 100.0,
    }
}

/// 静态初值与宪法缺省咬合的编译期钉（改 demo_page 宪法值会在这里撞）
const _: () = {
    assert!(dp::BODY_PX as u32 == 36);
    assert!((dp::LINE_RATIO * 100.0) as u32 == 140);
};

/// 一行折行结果：行内段列 + 实量行宽（px，H1 横带随字宽与涂装右裁同读）
#[derive(Debug, Clone, PartialEq)]
pub struct MdLine {
    pub spans: Vec<Span>,
    pub w: u32,
    /// 列表项首行标记（涂装 ▪ 符只挂项首行，折行续行不挂；其余块型恒假）
    pub item_start: bool,
}

/// 一块的几何+内容（y/h 相对文档画布原点，全咬半格网）
#[derive(Debug, Clone, PartialEq)]
pub struct MdBlock {
    pub kind: BlockKind,
    pub y: u32,
    pub h: u32,
    pub line_h: u32,
    pub px: f32,
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

/// 行高（px）：ratio 倍字号上取整咬半格网（demo_page::line_h 的参数化
/// 同式——缺省 ratio 下两式逐值相等，考题钉死）
pub fn line_h_styled(px: f32, ratio: f32) -> u32 {
    ((px * ratio) / dp::HU as f32).ceil() as u32 * dp::HU
}

/// 折行（像素尺）：spans 展平成带样式的字符流，贪心装满 content_w_px
/// 即断（满即断、刚好放下不断——同 modal::wrap_text 律，尺子从格宽换
/// 像素）；空行 = 一行空占位不塌。行宽 = 该行各段实量宽之和。
fn wrap_spans(spans: &[Span], content_w_px: u32, px: f32, m: &impl MdMeasure) -> Vec<MdLine> {
    if content_w_px == 0 {
        return vec![MdLine {
            spans: vec![(SegStyle::Normal, String::new())],
            w: 0,
            item_start: false,
        }];
    }
    let mut lines: Vec<MdLine> = Vec::new();
    let mut cur: Vec<Span> = Vec::new(); // 当前行的段列（同运行样式并入尾段）
    let mut cur_w = 0u32;
    for (style, text) in spans {
        for c in text.chars() {
            let mut buf = [0u8; 4];
            let cw = m.md_text_w(c.encode_utf8(&mut buf), px);
            if cur_w + cw > content_w_px && !cur.is_empty() {
                let w = line_w(&cur, px, m);
                lines.push(MdLine {
                    spans: std::mem::take(&mut cur),
                    w,
                    item_start: false,
                });
                cur_w = 0;
            }
            match cur.last_mut() {
                Some((s, t)) if *s == *style => t.push(c),
                _ => cur.push((*style, c.to_string())),
            }
            cur_w += cw;
        }
    }
    let w = line_w(&cur, px, m);
    lines.push(MdLine {
        spans: cur,
        w,
        item_start: false,
    });
    lines
}

/// 行实量宽 = 各段整串量宽之和（与涂装 pen 累进同尺——涂装按段画，
/// 段宽即 text_width(段串)）
fn line_w(spans: &[Span], px: f32, m: &impl MdMeasure) -> u32 {
    spans.iter().map(|(_, t)| m.md_text_w(t, px)).sum()
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

/// 整文档排版（纯函数）：content_w_px = 内容视口宽（px）；块列自上而下，
/// 块隙 BLOCK_GAP，y/h 全咬半格网；空文档 = 一行空正文占位（不塌，
/// 与旧 viewer_fields 空行占位同取舍）
pub fn layout_md(text: &str, content_w_px: u32, style: &MdStyle, m: &impl MdMeasure) -> MdLayout {
    let mut col = Collector::default();
    md_parse::parse_md(text, &mut col);

    let body_px = style.body_px;
    let body_lh = line_h_styled(body_px, style.line_ratio);
    let code_lh = line_h_styled(dp::CODE_PX, style.line_ratio);
    let mut blocks: Vec<MdBlock> = Vec::new();
    let mut y = 0u32;
    macro_rules! push {
        ($kind:expr, $h:expr, $lh:expr, $px:expr, $lines:expr) => {{
            blocks.push(MdBlock {
                kind: $kind,
                y,
                h: $h,
                line_h: $lh,
                px: $px,
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
                let px = body_px * scale;
                let lh = line_h_styled(px, style.line_ratio);
                // H1 折行宽 = 内容宽 − 框内缩 − 收尾（横带随字宽不吃满）；
                // 块高 = 上垫 + 各行带 + 下垫（多行 H1 行带累加）
                let wrap_w = if *level == 1 {
                    content_w_px.saturating_sub(dp::HEAD_TEXT_INSET + dp::HEAD_TOP_TAIL)
                } else {
                    content_w_px
                };
                let lines = wrap_spans(spans, wrap_w, px, m);
                let n = lines.len() as u32;
                let h = if *level == 1 {
                    dp::HU + lh * n + dp::HU
                } else {
                    lh * n
                };
                push!(kind, h, lh, px, lines);
            }
            Ev::Para(i) => {
                let mut lines = Vec::new();
                for spans in &col.paras[*i] {
                    lines.extend(wrap_spans(spans, content_w_px, body_px, m));
                }
                let n = lines.len() as u32;
                push!(BlockKind::Body, body_lh * n, body_lh, body_px, lines);
            }
            Ev::Code(i) => {
                // 围栏不折行：字面行原样（涂装右缘断墨同 demo 取舍）
                let lines = col.codes[*i]
                    .iter()
                    .map(|l| MdLine {
                        spans: vec![(SegStyle::Normal, l.clone())],
                        w: m.md_text_w(l, dp::CODE_PX),
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
                    dp::CODE_PX,
                    lines
                );
            }
            Ev::Quote(i) => {
                let mut lines = Vec::new();
                for spans in &col.quotes[*i] {
                    lines.extend(wrap_spans(
                        spans,
                        content_w_px.saturating_sub(dp::INDENT_W),
                        body_px,
                        m,
                    ));
                }
                let n = lines.len() as u32;
                push!(BlockKind::Quote, body_lh * n, body_lh, body_px, lines);
            }
            Ev::List(i) => {
                let mut lines = Vec::new();
                for spans in &col.lists[*i] {
                    let mut item_lines = wrap_spans(
                        spans,
                        content_w_px.saturating_sub(dp::LIST_TEXT_INSET),
                        body_px,
                        m,
                    );
                    if let Some(first) = item_lines.first_mut() {
                        first.item_start = true; // ▪ 符只挂项首行
                    }
                    lines.extend(item_lines);
                }
                let n = lines.len() as u32;
                push!(BlockKind::List, body_lh * n, body_lh, body_px, lines);
            }
            Ev::Hr => {
                push!(BlockKind::Hr, dp::HU * 2, dp::HU * 2, body_px, Vec::new());
            }
        }
    }
    if blocks.is_empty() {
        // 空文档 = 一行空正文占位（不塌）
        push!(
            BlockKind::Body,
            body_lh,
            body_lh,
            body_px,
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

// ---- 排版缓存（BAR-208 阅读页滚动病灶根修，2026-09-30）----
//
// 排版只随 (文本代, 内容宽, 样式两位) 失效——**滚动不动文本代**。旧路
// 帧内消费方（帧泵/拖拽/甩尾/涂装）各自 `text.clone()` + 全文重排：
// 一次滚动步 = 数次 2MB 克隆 + 数次全量 layout_md（用户真机「滑到约
// 三分之一卡一下静止」的结构性本体；demo 页同病）。缓存后同代共读
// 一份 Arc，重排只在新块/改宽/改样式时发生——终端视口模型：内容
// 一次排版，视口滑动零重排。
//
// 锁纪律：LAYOUT_CACHE 是叶子锁（持它绝不取他锁），term→reader 红线
// 不受影响。调用方责任：未中排版需要量宽器（持 term 锁），**不许持
// reader 锁调本族函数**——先 peek，未中再短锁克隆文本后排版。
/// 缓存键（文本代, 内容宽, body_px 位模, line_ratio 位模）
type CacheKey = (u64, u32, u32, u32);
/// 柜件 = 键 + 排版结果（共读 Arc）
type CacheEntry = (u64, u32, u32, u32, std::sync::Arc<MdLayout>);
static LAYOUT_CACHE: std::sync::Mutex<Option<CacheEntry>> = std::sync::Mutex::new(None);

/// 缓存键（文本代, 内容宽, body_px 位模, line_ratio 位模）
fn cache_key(txt_gen: u64, content_w_px: u32, style: &MdStyle) -> CacheKey {
    (
        txt_gen,
        content_w_px,
        style.body_px.to_bits(),
        style.line_ratio.to_bits(),
    )
}

/// 只查不排（帧内高频路径：滚动拖拽/甩尾/帧泵每事件一问）：命中 =
/// Arc 克隆 O(1)；未中 = None（调用方走 cached 全路）
pub fn layout_md_peek(
    txt_gen: u64,
    content_w_px: u32,
    style: &MdStyle,
) -> Option<std::sync::Arc<MdLayout>> {
    let g = LAYOUT_CACHE.lock().unwrap();
    match &*g {
        Some((g0, w0, b0, r0, lay))
            if (*g0, *w0, *b0, *r0) == cache_key(txt_gen, content_w_px, style) =>
        {
            Some(lay.clone())
        }
        _ => None,
    }
}

/// 查 + 未中全量排版入柜（低频路径：新块回执/改宽/改样式后的第一问）
pub fn layout_md_cached(
    txt_gen: u64,
    text: &str,
    content_w_px: u32,
    style: &MdStyle,
    m: &impl MdMeasure,
) -> std::sync::Arc<MdLayout> {
    let key = cache_key(txt_gen, content_w_px, style);
    let mut g = LAYOUT_CACHE.lock().unwrap();
    if let Some((g0, w0, b0, r0, lay)) = &*g
        && (*g0, *w0, *b0, *r0) == key
    {
        return lay.clone();
    }
    let lay = std::sync::Arc::new(layout_md(text, content_w_px, style, m));
    *g = Some((key.0, key.1, key.2, key.3, lay.clone()));
    lay
}
