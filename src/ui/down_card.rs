//! ui/down_card.rs — 断线状态卡（A 断线治理提案一，2026-09-20）。
//!
//! 触发相：活跃会话死了（session_over=true）→ 终端页顶部浮一张三级框
//! 状态卡：状态行（左）+ [重试][切本地] 双钮（右）。两条出路：
//! 重试 = kick_reconnect（与敲键触发同路）；切本地 = switch_session
//! （与 Ctrl-] 同路）。会话复活（session_over=false）卡即灭——
//! 终卡槽 sig 带 session_over 维，翻转自动重烘。
//! 状态行文本 = status_text 纯函数（2026-09-23 BAR-135 动态化：
//! 断线期告示全落本卡，终端正文零污染——旧蓝色内联横幅退役）。
//!
//! 分层：几何+命中 = 本册纯逻辑（A 档考题 tests/down_card_spec.rs）；
//! 涂装挂终卡槽烘焙末尾（android_app 调 TermEmu::render_down_card →
//! termview::TermView::paint_down_card 本体）——与齿轮同槽同规：
//! 面板靠泊时终卡槽整层隐，「断线卡只在裸终端页出现」白拿零新逻辑。
//!
//! 宪法合规：卡带 = 三级框主形态（左粗+三细全包框，135° 双色渐变+
//! 渐变暗芯），色源 = 终端页族 TERM_FRAME_C1/C2（终端页不随机）；
//! 高 4 格（内行 3 格 + 上下各半格净空——「最小框 ≥3 格、内容距边框
//! 上下 ≥半格、左右 ≥1 格」）；钮 = 同件三级框行 3 格高 6 格宽。

use crate::termview::{CELL_H, CELL_W, MARGIN_X, MARGIN_Y};

/// 卡带高 4 格（内行 3 格 + 上下各半格净空）
pub const CARD_H: u32 = CELL_H * 4;
/// 内行（文本+钮）高 3 格（三级框最小高）
pub const ROW_H: u32 = CELL_H * 3;
/// 钮宽 6 格（「切本地」3 字 + 左右各约 1 格内垫）
pub const BTN_W: u32 = CELL_W * 6;
/// 钮间距 1 格
pub const BTN_GAP: u32 = CELL_W;
/// 卡顶偏移 = 齿轮带下缘（gy0=MARGIN_Y+6、字形 72）再留 12px 缝——
/// 断线卡与齿轮同屏不叠（齿轮常驻右上角，卡在齿轮带之下通栏）
pub const CARD_TOP_OFF: u32 = 6 + 72 + 12;

/// 矩形（x, y, w, h）——几何单源的通用载体
pub type Rect = (i64, i64, u32, u32);

/// 命中语义：两个钮各一个动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownHit {
    /// [重试] = kick_reconnect（活跃会话重孵）
    Retry,
    /// [切本地] = switch_session（与 Ctrl-] 同路，待机侧通常=本地）
    Local,
}

/// 卡带矩形（x, y, w, h）：通栏——左右各让内容缘 1 格。
/// 太窄（两钮+钮距+6 格文本区摆不下）= None（不画不命中，不挤烂）
pub fn card_rect(buf_w: u32) -> Option<Rect> {
    let inset = MARGIN_X + CELL_W;
    let w = buf_w.checked_sub(2 * inset)?;
    if w < BTN_W * 2 + BTN_GAP + CELL_W * 6 {
        return None;
    }
    Some((
        i64::from(inset),
        i64::from(MARGIN_Y + CARD_TOP_OFF),
        w,
        CARD_H,
    ))
}

/// 内行矩形（文本与钮的共用行带）：卡内垂直居中（上下各半格净空同源）
pub fn row_rect(buf_w: u32) -> Option<Rect> {
    let (cx, cy, cw, _) = card_rect(buf_w)?;
    let ry = cy + i64::from((CARD_H - ROW_H) / 2);
    Some((cx, ry, cw, ROW_H))
}

/// 双钮矩形（(重试, 切本地)）：右簇——切本地贴右（距右细边 1 格内垫），
/// 重试左邻。眼手同尺：涂装与命中同读本函数
pub fn btn_rects(buf_w: u32) -> Option<(Rect, Rect)> {
    let (cx, _, cw, _) = card_rect(buf_w)?;
    let (_, ry, _, _) = row_rect(buf_w)?;
    let inner_r = cx + i64::from(cw) - 3; // 右细边 3px（三级框三细同尺）
    let local_x = inner_r - i64::from(CELL_W) - i64::from(BTN_W);
    let retry_x = local_x - i64::from(BTN_GAP) - i64::from(BTN_W);
    Some(((retry_x, ry, BTN_W, ROW_H), (local_x, ry, BTN_W, ROW_H)))
}

/// 命中判定（纯函数）：点在哪个钮归哪个，卡带其余区不吞触摸
/// （状态行只是信息——点它没语义就别拦路，触摸穿透给终端手势）
pub fn hit(x: f64, y: f64, buf_w: u32) -> Option<DownHit> {
    let (retry, local) = btn_rects(buf_w)?;
    let in_rect = |(bx, by, bw, bh): (i64, i64, u32, u32)| {
        x >= bx as f64
            && x < (bx + i64::from(bw)) as f64
            && y >= by as f64
            && y < (by + i64::from(bh)) as f64
    };
    if in_rect(retry) {
        return Some(DownHit::Retry);
    }
    if in_rect(local) {
        return Some(DownHit::Local);
    }
    None
}

/// 状态行文本（纯函数，2026-09-23 BAR-135「提示全部在跳出的框上」）：
/// 断线期的一切告示都落在这张卡上，终端正文零污染（旧蓝色横幅退役）。
/// 三态优先级：重连在途 > 有暂存 > 裸断开
pub fn status_text(connecting: bool, pending_bytes: usize) -> String {
    if connecting {
        "重连中…接回 = 新 shell（旧现场 tmux attach 接回）".to_string()
    } else if pending_bytes > 0 {
        format!("连接已断开 · 输入已暂存 {pending_bytes} 字节（接回自动补发）")
    } else {
        "连接已断开".to_string()
    }
}
