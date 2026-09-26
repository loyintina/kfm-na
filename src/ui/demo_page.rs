//! ui/demo_page.rs — md 渲染打样 demo 页布局核心（2026-09-26 研究线拍板：
//! 静态渲染一份展示样品，硬编码内容，不做 md 解析器——解析器是后续
//! 另一单）。
//!
//! 分层：本册 = 纯逻辑几何单源（A 档考题 tests/demo_page_spec.rs）——
//! 行高/缩进/块序/半格网咬合全在这里钉死；涂装在 termview
//! （paint_demo_page_chrome 底装修 + paint_demo_content 内容墨），
//! 块几何只读本册，不许各算。
//!
//! 视觉条款 = 宪法 2026-09-26 两修：§2.5 淡彩强调档（粗体淡粉/行内码
//! 浅青）+ §三 md 标题 ┌ 左上直角框（H1-H3，accent 135° 渐变采样，
//! 文字距框缘上 ≥0.5 格、左 ≥1 格）。白三档 0.85/0.75/0.5（§2.3）。
//! 所有行高/间距 = 0.5 格整数倍（§一 半格网，CELL_W=18/CELL_H=36）。

use crate::termview::{CELL_H, CELL_W};

/// 正文字号（px = 1 格高）：阶梯与行距的基准
pub const BODY_PX: f32 = 36.0;
/// 标题字号阶梯（宪法 §三 md 条款）
pub const H1_SCALE: f32 = 1.7;
pub const H2_SCALE: f32 = 1.45;
pub const H3_SCALE: f32 = 1.2;
/// 代码围栏字号（等宽文字白 0.75，比正文小半档）
pub const CODE_PX: f32 = 30.0;
/// 正文行距倍数（宪法：行高 = 1.4 倍字号，上取整咬半格网）
pub const LINE_RATIO: f32 = 1.4;
/// 半格竖向（px）：一切行高/间距的量子
pub const HU: u32 = CELL_H / 2;

/// 内容区右内缘距屏右（内容视口宽 = 屏宽 − 左原点 43 − 右 37，
/// §七 标定值表同源）
pub const CONTENT_RIGHT_INSET: u32 = 37;

/// 块型（展示样品的元素清单，一块一型）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    /// 正文段（含粗体/行内码行内段，行内布局归涂装侧量宽）
    Body,
    /// 代码围栏（展示型值框：四边均匀细框 + 渐变暗底内芯）
    Code,
    /// 引用（左竖线 + 白 0.5 + 缩进 1 格）
    Quote,
    /// 列表（缩进 1 格，▪ accent 符 + 白 0.75）
    List,
    /// 分隔线（1px 横向 accent 渐变，半透明，上下各 0.5 格）
    Hr,
    /// 签名行
    Sign,
}

/// 一块的几何（y/h 都是屏 px，相对页画布；全部咬半格网）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub y: u32,
    pub h: u32,
    /// 文字行带高（行内文字垂直居中于它）
    pub line_h: u32,
    /// 字号
    pub px: f32,
}

/// 整页布局（内容原点 + 内容宽 + 块列）
#[derive(Debug, Clone, PartialEq)]
pub struct DemoLayout {
    pub ox: u32,
    pub oy: u32,
    pub cw: u32,
    pub blocks: Vec<Block>,
}

/// 行高（px）：1.4 倍字号上取整咬半格网（宪法行距 × 半格网两律的合解）
pub fn line_h(px: f32) -> u32 {
    ((px * LINE_RATIO) / HU as f32).ceil() as u32 * HU
}

/// 标题块高：上垫 0.5 格（文字距框缘上 ≥0.5 格）+ 行带 + 下垫 0.5 格
fn heading_h(px: f32) -> u32 {
    HU + line_h(px) + HU
}

/// 块间留隙 = 0.5 格
pub const BLOCK_GAP: u32 = HU;

// ---- 硬编码样品文本（一封假信件；md 解析器落地后由它喂，本册是打样）----

pub const H1_TEXT: &str = "md 渲染打样";
pub const H2_SECTION: &str = "六档标题";
pub const H2_TEXT: &str = "二级标题 Heading";
pub const H3_TEXT: &str = "三级标题 Sub";
pub const H4_TEXT: &str = "四级标题 Emphasis";
pub const H5_TEXT: &str = "五级标题 Dim";
pub const H6_TEXT: &str = "六级标题 Faint";
pub const CODE_LINES: [&str; 3] = ["fn main() {", "    println!(\"hi-bit\");", "}"];
pub const QUOTE_LINES: [&str; 2] = ["引用第一行：打样不定稿。", "引用第二行：宪法先修。"];
pub const LIST_ITEMS: [&str; 3] = ["缩进一格圆点项", "accent 色方块符", "白 0.75 列表文"];
pub const SIGN_TEXT: &str = "—— 研究线 敬上";

/// 正文段的行内段（样式, 文本）：粗体 = 淡粉双绘，行内码 = 浅青暗底小块
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegStyle {
    Normal,
    Bold,
    Code,
}
pub const BODY_SEGS: [(SegStyle, &str); 5] = [
    (SegStyle::Normal, "见字如面："),
    (SegStyle::Bold, "粗体"),
    (SegStyle::Normal, "与"),
    (SegStyle::Code, "code"),
    (SegStyle::Normal, "同排一行。"),
];

/// 整页布局（纯函数）：内容原点 = tab_bar::content_origin 同源
/// （43, 55 = 环内缘 + 1 格，§七 标定值）；块列自上而下排，
/// 每块 y/h 全是半格网整数倍。屏宽只定内容宽，不进纵向账
pub fn layout(w: u32) -> DemoLayout {
    let (ox, oy) = crate::ui::tab_bar::content_origin();
    let cw = w.saturating_sub(ox + CONTENT_RIGHT_INSET);
    let mut blocks = Vec::new();
    let mut y = oy;
    let mut push = |kind: BlockKind, h: u32, lh: u32, px: f32, y: &mut u32| {
        blocks.push(Block {
            kind,
            y: *y,
            h,
            line_h: lh,
            px,
        });
        *y += h + BLOCK_GAP;
    };
    let body_lh = line_h(BODY_PX);
    // H1「md 渲染打样」
    push(
        BlockKind::H1,
        heading_h(BODY_PX * H1_SCALE),
        line_h(BODY_PX * H1_SCALE),
        BODY_PX * H1_SCALE,
        &mut y,
    );
    // 正文段（含粗体与行内码）
    push(BlockKind::Body, body_lh, body_lh, BODY_PX, &mut y);
    // H2「六档标题」
    push(
        BlockKind::H2,
        heading_h(BODY_PX * H2_SCALE),
        line_h(BODY_PX * H2_SCALE),
        BODY_PX * H2_SCALE,
        &mut y,
    );
    // 六档各一行（H1 已置顶展示）：H2/H3 挂 ┌ 框，H4-H6 正文字号文字档
    push(
        BlockKind::H2,
        heading_h(BODY_PX * H2_SCALE),
        line_h(BODY_PX * H2_SCALE),
        BODY_PX * H2_SCALE,
        &mut y,
    );
    push(
        BlockKind::H3,
        heading_h(BODY_PX * H3_SCALE),
        line_h(BODY_PX * H3_SCALE),
        BODY_PX * H3_SCALE,
        &mut y,
    );
    push(BlockKind::H4, body_lh, body_lh, BODY_PX, &mut y);
    push(BlockKind::H5, body_lh, body_lh, BODY_PX, &mut y);
    push(BlockKind::H6, body_lh, body_lh, BODY_PX, &mut y);
    // 代码围栏（三行 Rust，上下各留 0.5 格）
    let code_lh = line_h(CODE_PX);
    push(
        BlockKind::Code,
        HU + code_lh * CODE_LINES.len() as u32 + HU,
        code_lh,
        CODE_PX,
        &mut y,
    );
    // 引用两行
    push(
        BlockKind::Quote,
        body_lh * QUOTE_LINES.len() as u32,
        body_lh,
        BODY_PX,
        &mut y,
    );
    // 列表三项
    push(
        BlockKind::List,
        body_lh * LIST_ITEMS.len() as u32,
        body_lh,
        BODY_PX,
        &mut y,
    );
    // 分隔线（上下各 0.5 格，线体居中）
    push(BlockKind::Hr, HU * 2, HU * 2, BODY_PX, &mut y);
    // 签名行
    push(BlockKind::Sign, body_lh, body_lh, BODY_PX, &mut y);
    DemoLayout { ox, oy, cw, blocks }
}

/// ┌ 框文字内缩（px）：文字距框左缘 ≥1 格（宪法最小容量律同级条款）
pub const HEAD_TEXT_INSET: u32 = CELL_W;
/// ┌ 框描边厚（px）：与页环细边同尺（AI_PAGE_FRAME_W=3）
pub const HEAD_FRAME_T: u32 = 3;
/// 引用左竖线宽（px）
pub const QUOTE_BAR_W: u32 = 2;
/// 引用/列表文字内缩（px）：缩进 1 格
pub const INDENT_W: u32 = CELL_W;
/// 列表符号块边长（px，▪ 的像素形）与符号后文字位（符号格 1 格）
pub const LIST_MARK_PX: u32 = 8;
pub const LIST_TEXT_INSET: u32 = CELL_W * 2;
