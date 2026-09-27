//! ui/demo_page.rs — md 渲染打样 demo 页布局核心（2026-09-26 研究线拍板：
//! 静态渲染一份展示样品，硬编码内容，不做 md 解析器——解析器是后续
//! 另一单）。
//!
//! 分层：本册 = 纯逻辑几何单源（A 档考题 tests/demo_page_spec.rs）——
//! 行高/缩进/块序/半格网咬合全在这里钉死；涂装在 termview
//! （paint_demo_page_chrome 底装修 + paint_demo_content 内容墨），
//! 块几何只读本册，不许各算。
//!
//! 视觉条款 = 宪法 2026-09-27 修宪：§2.5 淡彩六色家族（accent c1 色相
//! 固定 60° 步进派生，槽位角色见 pastel_role——粗体/H4/保留/行内码/
//! H2/H3）+ §三 md 标题条款（H1 = [ 形框：左竖+顶底横+上下圆角 R=半格，
//! 横带宽 = 1 格缩进 + 文字行宽 + 0.5 格收尾随字长，文字 = 淡彩 slot0
//! 双绘；H2/H3 无框，字号阶梯 + 淡彩色分档；分隔线 3px 与框厚同尺）。
//! 白三档 0.85/0.75/0.5（§2.3）。
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

/// H1 标题块高：上垫 0.5 格（文字距框缘上 ≥0.5 格）+ 行带 + 下垫 0.5 格
/// （┌ 框 H1 专属——H2/H3 摘框后块高 = 行带，不走本函数）
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

/// 正文段的行内段（样式, 文本）：粗体 = 淡彩 slot0 双绘，行内码 =
/// 淡彩 slot3 + 暗底小块（宪法 §2.5 六色家族，角色表见 pastel_role）
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
    // H2「六档标题」（2026-09-27 修宪：H2/H3 摘框——无框块高 = 行带，
    // 不再留 ┌ 框的上下垫）
    push(
        BlockKind::H2,
        line_h(BODY_PX * H2_SCALE),
        line_h(BODY_PX * H2_SCALE),
        BODY_PX * H2_SCALE,
        &mut y,
    );
    // 六档各一行（H1 已置顶展示）：H2/H3 字号+淡彩色分档，H4-H6 正文
    // 字号文字档
    push(
        BlockKind::H2,
        line_h(BODY_PX * H2_SCALE),
        line_h(BODY_PX * H2_SCALE),
        BODY_PX * H2_SCALE,
        &mut y,
    );
    push(
        BlockKind::H3,
        line_h(BODY_PX * H3_SCALE),
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
/// ┌ 框顶边收尾（px）：顶边宽 = 缩进 + 文字行宽 + 0.5 格收尾（随字长，
/// 不吃满内容宽——宪法 2026-09-27 修宪）
pub const HEAD_TOP_TAIL: u32 = CELL_W / 2;
/// ┌ 框描边厚（px）：与页环细边同尺（AI_PAGE_FRAME_W=3）
pub const HEAD_FRAME_T: u32 = 3;
/// [ 框圆角半径（px）：半格（2026-09-27 用户拍板「直角换圆角，左下
/// 再加一个圆角」——形态从 ┌ 变 [ ：左竖 + 顶底横 + 上下两圆角）
pub const HEAD_CORNER_R: u32 = CELL_H / 2;
/// 分隔线粗（px，2026-09-27 用户拍板「做得粗一些」：1 → 3，与框厚同尺）
pub const HR_THICK: u32 = 3;
/// 淡彩家族槽位角色（宪法 §2.5 角色映射单源；家族本身 accent::
/// pastel_family 派生——本表只管义不管色）
pub mod pastel_role {
    /// 粗体（accent 本相淡彩）
    pub const BOLD: usize = 0;
    /// H4 强调
    pub const H4: usize = 1;
    /// 保留（斜体·链接，md 解析器单启用）
    pub const RESERVED: usize = 2;
    /// 行内码（180° 对色）
    pub const INLINE_CODE: usize = 3;
    /// H2 标题
    pub const H2: usize = 4;
    /// H3 标题
    pub const H3: usize = 5;
}
/// 引用左竖线宽（px）
pub const QUOTE_BAR_W: u32 = 2;
/// 引用/列表文字内缩（px）：缩进 1 格
pub const INDENT_W: u32 = CELL_W;
/// 列表符号块边长（px，▪ 的像素形）与符号后文字位（符号格 1 格）
pub const LIST_MARK_PX: u32 = 8;
pub const LIST_TEXT_INSET: u32 = CELL_W * 2;
