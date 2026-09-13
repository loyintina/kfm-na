//! modal.rs — 跳框（modal）几何核（主题宪法 §六 跳框条款，2026-09-13
//! 九修，用户拍板；核心层纯逻辑零 IO，A 档钉）。
//!
//! 条款兑现：模态详情卡——压暗层（涂装侧 α150 黑）+ 居中卡片
//! （paint_rect_ring 池框同尺 R=36px、内卡反转 c2→c1）+ 标题
//! （2 格 36px 居中）+ 1px 渐变分隔线 + 字段区（题注 1 格 30px 灰
//! 在上、内容 1 格/行 36px 亮在下——与上池字段行字档相反是**有意的**，
//! 跳框里题注是内容的脚注）+ 底部全宽关闭钮（3 格）。
//!
//! 眼手同尺：涂装与触摸命中读本册同一份几何（card_rect/
//! close_btn_rect/hit），壳层不许另算。字段折行（wrap_text）也是
//! 本册的事——卡宽定行宽，涂装/命中/卡高计算吃同一份折行结果。
//!
//! v1 取舍：卡高封顶屏高−8 格，超出截断不做滚动；无入场动画。

use crate::termview::{CELL_H, CELL_W};
use crate::ui::comp_registry::CompEntry;
use crate::ui::dual_pool::PoolRect;

/// 卡距屏左右各 3 格
pub const MODAL_SIDE_MARGIN: u32 = CELL_W * 3;
/// 卡内边距 2 格（最小容量律 §三：左右 ≥1 格，取 2 格与池内边距同尺）
pub const MODAL_PAD_X: i64 = CELL_W as i64 * 2;
/// 卡顶/底留白各 1 格
pub const MODAL_PAD_Y: u32 = CELL_H;
/// 标题带高 2 格
pub const MODAL_TITLE_H: u32 = CELL_H * 2;
/// 题注/内容行高各 1 格
pub const MODAL_LABEL_H: u32 = CELL_H;
pub const MODAL_LINE_H: u32 = CELL_H;
/// 字段间留隙 0.5 格（半格网 §一）
pub const MODAL_FIELD_GAP: u32 = CELL_H / 2;
/// 关闭钮高 3 格（§三 最小容量律下限）
pub const MODAL_CLOSE_H: u32 = CELL_H * 3;
/// 预览画板高 6 格（十修 §六 跳框预览画板条款）
pub const MODAL_PREVIEW_H: u32 = CELL_H * 6;
/// 卡高封顶余量：屏高 − 8 格（上下各 4 格，压暗层仍可见可点）
pub const MODAL_MAX_MARGIN_Y: u32 = CELL_H * 4;

/// 跳框字段（题注 + 已折行的内容行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalField {
    pub label: String,
    pub lines: Vec<String>,
}

/// 命中分类（模态交互：点框外/关闭钮 = 收起；框内其他 = 无操作吃手势）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalHit {
    Close,
    Card,
    Outside,
}

/// 贪心折行（格宽尺：CJK 2 格/其余 1 格，与 tab_bar::text_cells 同尺）。
/// 满即断、刚好放下不断；空串 = 一行空（占位不塌）
pub fn wrap_text(s: &str, width_cells: u32) -> Vec<String> {
    if width_cells == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0u32;
    for c in s.chars() {
        let cw = if (c as u32) >= 0x2E80 { 2 } else { 1 };
        if cur_w + cw > width_cells && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(c);
        cur_w += cw;
    }
    lines.push(cur);
    lines
}

/// 卡内容宽（格）=（卡宽 − 两侧内边距）/ CELL_W——折行的尺子
pub fn content_cells(screen_w: u32) -> u32 {
    let card_w = screen_w.saturating_sub(MODAL_SIDE_MARGIN * 2);
    (card_w as i64 - MODAL_PAD_X * 2).max(0) as u32 / CELL_W
}

/// 组件条目 → 跳框字段区（名 = 标题不在字段里；折行吃 content_cells
/// 同一把尺——卡高计算与涂装断行一致）
pub fn fields_of(entry: &CompEntry, width_cells: u32) -> Vec<ModalField> {
    let mk = |label: &str, value: String| ModalField {
        label: label.into(),
        lines: wrap_text(&value, width_cells),
    };
    vec![
        mk("状态", entry.status.label().into()),
        mk("位置", format!("{} · {}", entry.file, entry.symbol)),
        mk("规范", entry.spec.into()),
        mk("考题", entry.tests.into()),
        mk("说明", entry.desc.into()),
    ]
}

/// 字段区高（题注 + 内容行 + 字段隙，逐字段累加）
fn fields_h(fields: &[ModalField]) -> u32 {
    fields
        .iter()
        .map(|f| MODAL_LABEL_H + f.lines.len() as u32 * MODAL_LINE_H + MODAL_FIELD_GAP)
        .sum()
}

/// 预览画板矩形（十修 §六：分隔线下 0.5 格 → 画板 6 格 → 0.5 格 →
/// 字段区；卡内宽，涂装/卡高计算同读——眼手同尺）
pub fn preview_rect(card: &PoolRect) -> PoolRect {
    PoolRect {
        x: card.x + MODAL_PAD_X,
        y: card.y
            + i64::from(MODAL_PAD_Y)
            + i64::from(MODAL_TITLE_H)
            + i64::from(MODAL_FIELD_GAP)
            + 1
            + i64::from(MODAL_FIELD_GAP),
        w: (card.w as i64 - MODAL_PAD_X * 2).max(0) as u32,
        h: MODAL_PREVIEW_H,
    }
}

/// 字段区起始 y（画板下缘再留 0.5 格呼吸）
pub fn fields_top(card: &PoolRect) -> i64 {
    preview_rect(card).y + i64::from(MODAL_PREVIEW_H) + i64::from(MODAL_FIELD_GAP)
}

/// 居中卡片矩形（高随内容，封顶屏高−8 格；v1 超出截断不滚动）
pub fn card_rect(screen_w: u32, screen_h: u32, fields: &[ModalField]) -> PoolRect {
    let w = screen_w.saturating_sub(MODAL_SIDE_MARGIN * 2);
    // 顶留白 + 标题 + 分隔线带（上 0.5 格 + 1px + 下 0.5 格）+ 预览画板
    // + 0.5 格呼吸 + 字段区 + 关闭钮前隙 0.5 格 + 关闭钮 + 底留白
    let want = MODAL_PAD_Y
        + MODAL_TITLE_H
        + (MODAL_FIELD_GAP + 1 + MODAL_FIELD_GAP)
        + MODAL_PREVIEW_H
        + MODAL_FIELD_GAP
        + fields_h(fields)
        + MODAL_FIELD_GAP
        + MODAL_CLOSE_H
        + MODAL_PAD_Y;
    let max_h = screen_h.saturating_sub(MODAL_MAX_MARGIN_Y * 2);
    let h = want.min(max_h.max(MODAL_CLOSE_H + MODAL_PAD_Y * 2));
    PoolRect {
        x: i64::from(MODAL_SIDE_MARGIN),
        y: i64::from(screen_h.saturating_sub(h) / 2),
        w,
        h,
    }
}

/// 关闭钮矩形（卡底全内宽，3 格高，底留白 1 格之上）
pub fn close_btn_rect(card: &PoolRect) -> PoolRect {
    PoolRect {
        x: card.x + MODAL_PAD_X,
        y: card.y + i64::from(card.h) - i64::from(MODAL_PAD_Y + MODAL_CLOSE_H),
        w: (card.w as i64 - MODAL_PAD_X * 2).max(0) as u32,
        h: MODAL_CLOSE_H,
    }
}

/// 命中分类（x/y 屏坐标；card 由 card_rect 出——眼手同尺）
pub fn hit(x: i64, y: i64, card: &PoolRect) -> ModalHit {
    let in_card = x >= card.x
        && x < card.x + i64::from(card.w)
        && y >= card.y
        && y < card.y + i64::from(card.h);
    if !in_card {
        return ModalHit::Outside;
    }
    let btn = close_btn_rect(card);
    if x >= btn.x && x < btn.x + i64::from(btn.w) && y >= btn.y && y < btn.y + i64::from(btn.h) {
        return ModalHit::Close;
    }
    ModalHit::Card
}
