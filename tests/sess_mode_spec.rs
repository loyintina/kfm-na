//! tests/sess_mode_spec.rs — BAR-125 每会话模式快照考题（A 档）
//!
//! 判的坑：鼠标上报/alt-screen 等壳模式跨会话泄漏——切回本地裸 shell
//! 后滑手势被翻成 SGR 滚轮序列发 pty 回显成乱码（用户 2026-09-21 真机报）。
//! 复位漏清 = 泄漏照旧；恢复漏设 = 切回 tmux 滚轮翻译坏掉。

use alacritty_terminal::term::TermMode;
use kfm_na::sess_mode::{managed_mask, reset_seq, restore_seq};

const MOUSE: u32 = TermMode::MOUSE_REPORT_CLICK.bits();
const DRAG: u32 = TermMode::MOUSE_DRAG.bits();
const MOTION: u32 = TermMode::MOUSE_MOTION.bits();
const SGR: u32 = TermMode::SGR_MOUSE.bits();
const ALT: u32 = TermMode::ALT_SCREEN.bits();
const APP_CUR: u32 = TermMode::APP_CURSOR.bits();
const PASTE: u32 = TermMode::BRACKETED_PASTE.bits();

/// 考题 1：复位只清「当前置位的受管模式」，且 ?1049l 必须打头
/// （先回主屏，后续清屏才作用在主网格）
#[test]
fn spec_bar125_复位_只清置位且alt打头() {
    let s = reset_seq(MOUSE | SGR | ALT);
    assert!(
        s.starts_with("\x1b[?1049l"),
        "alt 在场必须第一个回主屏: {s:?}"
    );
    assert!(s.contains("\x1b[?1000l"), "鼠标点击上报要清: {s:?}");
    assert!(s.contains("\x1b[?1006l"), "SGR 鼠标格式要清: {s:?}");
    assert!(!s.contains("\x1b[?1002l"), "未置位的模式不许乱清: {s:?}");
    assert!(s.ends_with("\x1b[r\x1b[?25h"), "滚动区+光标复显尾巴: {s:?}");
}

/// 考题 2：干净态复位 = 零 DECRST，只有滚动区/光标兜底
#[test]
fn spec_bar125_复位_干净态零decrst() {
    let s = reset_seq(0);
    assert_eq!(s, "\x1b[r\x1b[?25h");
}

/// 考题 3：恢复只设快照置位的受管模式；无快照 = 空序列（全干净，
/// 裸 shell 天然如此——本地首次切入就必须是这态）
#[test]
fn spec_bar125_恢复_只设快照位() {
    let s = restore_seq(MOUSE | ALT | PASTE);
    assert!(s.contains("\x1b[?1000h"));
    assert!(s.contains("\x1b[?1049h"));
    assert!(s.contains("\x1b[?2004h"));
    assert!(!s.contains("\x1b[?1002h"), "快照没置位不许恢复: {s:?}");
    assert_eq!(restore_seq(0), "", "无快照 = 空恢复（全干净）");
}

/// 考题 4：往返一致——复位清掉的每一个模式，恢复都要能设回来
/// （切出 tmux 再切回，滚轮翻译不丢的数学保证）
#[test]
fn spec_bar125_往返_清了必能设回() {
    let cur = MOUSE | DRAG | MOTION | SGR | ALT | APP_CUR | PASTE;
    let rst = reset_seq(cur);
    let rec = restore_seq(cur);
    for num in [1, 1000, 1002, 1003, 1006, 1049, 2004] {
        assert!(rst.contains(&format!("\x1b[?{num}l")), "复位漏清 ?{num}");
        assert!(rec.contains(&format!("\x1b[?{num}h")), "恢复漏设 ?{num}");
    }
}

/// 考题 5：受管面不收 LINE_WRAP——双端默认一致的模式进快照面 =
/// 白担恢复错位的风险（快照面越小越好）
#[test]
fn spec_bar125_受管面_不收linewrap() {
    assert_eq!(managed_mask() & TermMode::LINE_WRAP.bits(), 0);
    let s = reset_seq(TermMode::LINE_WRAP.bits());
    assert!(!s.contains("\x1b[?7l"), "LINE_WRAP 不许进受管面: {s:?}");
}

// ---- 快照源保真钉：mode_bits 必须真实反映喂进去的 DECSET/DECRST ----
// （快照存错 = 恢复无米下锅；getter 面判卷成本倒挂豁免不适用——它背的
// 是「快照源准不准」的行为契约）

fn host_font() -> fontdue::Font {
    let bytes = std::fs::read(
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        ]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .expect("host 测试字体缺失"),
    )
    .expect("host 测试字体读不了");
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .expect("fontdue 不认 DejaVuSansMono")
}

/// 考题 6：快照源保真——喂 ?1000h 后 mode_bits 带鼠标位，喂 ?1000l 后
/// 摘掉（切出存账/切入复位同一把尺的物理基础）
#[test]
fn spec_bar125_快照源_mode_bits随喂入真实变化() {
    use kfm_na::termview::{CELL_H, CELL_W, TermView};
    let mut tv = TermView::new(host_font(), None, 8, 2, CELL_W, CELL_H);
    assert_eq!(tv.mode_bits() & MOUSE, 0, "起步不该有鼠标位");
    tv.feed("\x1b[?1000h".as_bytes());
    assert_ne!(tv.mode_bits() & MOUSE, 0, "?1000h 后快照必须带上鼠标位");
    assert_ne!(tv.mode_bits() & managed_mask(), 0, "受管面必须罩得住它");
    tv.feed("\x1b[?1000l".as_bytes());
    assert_eq!(tv.mode_bits() & MOUSE, 0, "?1000l 后快照必须摘掉鼠标位");
}
