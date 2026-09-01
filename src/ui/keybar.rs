//! ui/keybar.rs — 快捷键行控件（第 3 层；2026-09-01 自 termview 物理搬移
//! 立形：视图 = 本文件 impl TermView 块，键表在 keybar.rs（L2 状态层），
//! 通道 = keys-in，考题 = keybar_spec + termview_spec 渲染冒烟。
//! 配色已 token 化（theme.keybar），值与原 KEYBAR_* 常量逐项一致。
use crate::termview::Frame;

impl crate::termview::TermView {
    /// 快捷键行渲染（BAR-017：Java View 被原生 busy 重绘盖掉，改 Rust 自绘——
    /// 覆盖层 UI 的统一模式）。画在帧缓冲底部、键盘 inset 之上的 HEIGHT_PX 带
    /// （键盘弹起时跟着上浮，16777485 实拍：画死在屏底会被键盘盖住）：
    /// 行底 → 圆角药丸键格（修饰键粘滞中换高亮色）→ 标签字形居中
    /// mods = 调用方传入的修饰键粘滞位（input-ime 方案 A：不自读静态，
    /// 状态归 input.modifiers 服务，渲染层只收参数）
    pub fn render_keybar(
        &self,
        buf: &mut [u32],
        buf_w: u32,
        buf_h: u32,
        ime_bottom: u32,
        mods: u8,
    ) {
        use crate::keybar;
        let Some(top) = buf_h
            .checked_sub(ime_bottom)
            .and_then(|b| b.checked_sub(keybar::HEIGHT_PX))
        else {
            return;
        };
        if buf_w == 0 {
            return;
        }
        let mut frame = Frame {
            buf,
            w: buf_w,
            h: buf_h,
        };
        frame.fill_rect(0, top, buf_w, keybar::HEIGHT_PX, self.theme.keybar.bg);
        let cell_w = buf_w / keybar::COLS;
        if cell_w < 8 {
            return; // 窗太窄画不下，保命要紧
        }
        for (row, keys) in keybar::KEYS.iter().enumerate() {
            for (col, kd) in keys.iter().enumerate() {
                if matches!(kd.key, keybar::Key::None) {
                    continue;
                }
                let x = col as u32 * cell_w;
                let y = top + row as u32 * keybar::ROW_H_PX;
                let active = matches!(kd.key, keybar::Key::Modifier(bit) if mods & bit != 0);
                let bg = if active {
                    self.theme.keybar.mod_on
                } else {
                    self.theme.keybar.key_bg
                };
                // 圆角药丸键格（内缩出缝，圆角半径 14px）
                frame.fill_round_rect(x + 3, y + 3, cell_w - 6, keybar::ROW_H_PX - 6, 14, bg);
                self.draw_label(
                    &mut frame,
                    kd.label,
                    x,
                    cell_w,
                    y,
                    keybar::ROW_H_PX,
                    self.theme.keybar.label,
                );
            }
        }
    }
}
