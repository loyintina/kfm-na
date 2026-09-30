//! ui/demo_page.rs — md 引擎宪法 token 单源（2026-09-30，BAR-207 二期：
//! md 打样 demo 页整体退役，本册只留尺子——打样期的硬编码样品文本与
//! 整页 layout() 随页同焚，留下的全是 md 排版/涂装引擎（md_layout /
//! md_paint）在吃的宪法 token，本册继续当它们的单源）。
//!
//! 视觉条款 = 宪法 2026-09-27 修宪：§2.5 淡彩六色家族（accent c1 色相
//! 固定 60° 步进派生，槽位角色见 pastel_role——粗体/H4/保留/行内码/
//! H2/H3）+ §三 md 标题条款（H1 = 半包框：顶横+左竖+上下圆角 R=半格，
//! 底横去掉只留左下圆角；顶横宽 = 1 格缩进 + 文字行宽 + 0.5 格收尾随
//! 字长；文字 = 淡彩 slot0 双绘；H2/H3 无框，字号阶梯 + 淡彩色分档；
//! 分隔线 3px 与框厚同尺）。白三档 0.85/0.75/0.5（§2.3）。
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

/// 块间留隙 = 0.5 格
pub const BLOCK_GAP: u32 = HU;

/// 块型（md 引擎的块语义清单，一块一型；md_parse 块型语义对齐本表）
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

/// 行高（px）：1.4 倍字号上取整咬半格网（宪法行距 × 半格网两律的合解；
/// md_layout::line_h_styled 是它的参数化版，缺省 ratio 下必须同式）
pub fn line_h(px: f32) -> u32 {
    ((px * LINE_RATIO) / HU as f32).ceil() as u32 * HU
}

/// 正文段的行内段样式：粗体 = 淡彩 slot0 双绘，行内码 =
/// 淡彩 slot3 + 暗底小块（宪法 §2.5 六色家族，角色表见 pastel_role）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegStyle {
    Normal,
    Bold,
    Code,
}

/// [ 框文字内缩（px）：文字距框左缘 ≥1 格（宪法最小容量律同级条款）
pub const HEAD_TEXT_INSET: u32 = CELL_W;
/// [ 框顶边收尾（px）：顶边宽 = 缩进 + 文字行宽 + 0.5 格收尾（随字长，
/// 不吃满内容宽——宪法 2026-09-27 修宪）
pub const HEAD_TOP_TAIL: u32 = CELL_W / 2;
/// [ 框描边厚（px）：与页环细边同尺（AI_PAGE_FRAME_W=3）
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
