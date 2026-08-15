//! keybar.rs — Rust 自绘快捷键行（A 档纯逻辑，考题 tests/keybar_spec.rs）
//!
//! 为什么不是 Java View（BAR-017 实拍判词）：终端是 busy-loop 每帧往窗口
//! surface 重绘，Java View 画在同一面上被原生帧 60fps 盖掉——行其实建出来
//! 了（inset 上报 315px 实锤），但永远看不见。这条路对文件树/光球面板同样
//! 是死路，所以覆盖层 UI 的统一模式 = Rust 自绘 + 触摸命中测试。
//!
//! 布局（2026-08-14 二稿，用户定稿，两排七列）：
//!   上排: [Esc] [Alt] [Home] [PgUp] [ ↑ ] [PgDn] [Shift]
//!   下排: [Tab] [Ctrl] [End]  [ ← ] [ ↓ ] [  → ] [Enter]
//! ↑↓ 严格同列（第 5 列），方向十字 ←↓→ 在下排 3/4/5 列（右手惯用）。
//! 行带位置 = 屏高 - 键盘 inset - 行高（16777485 实拍：画死在屏底会被
//! 弹起的键盘盖住，行必须跟着键盘上浮）。
//!
//! 修饰键一次性粘滞（Termux 同款）：toggle 点亮，下一次 commitText 落字时
//! take_modifiers 读走并清零（联动一次自动灭）。映射逻辑在 keymap.rs。

use std::sync::atomic::{AtomicU8, Ordering};

pub const COLS: u32 = 7;
pub const ROW_H_PX: u32 = 120;
pub const HEIGHT_PX: u32 = ROW_H_PX * 2;

/// 修饰键位掩码
pub const MOD_CTRL: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_SHIFT: u8 = 4;

/// 键行为：直接键发原始 Android 键码（序列翻译在排干侧 keymap.rs）；
/// 修饰键翻粘滞位；None = 空位（第 4 列分隔）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    None,
    Direct(i32),
    Modifier(u8),
}

#[derive(Debug, Clone, Copy)]
pub struct KeyDef {
    pub label: &'static str,
    pub key: Key,
}

// android.view.KeyEvent 键码
const KC_UP: i32 = 19;
const KC_DOWN: i32 = 20;
const KC_LEFT: i32 = 21;
const KC_RIGHT: i32 = 22;
const KC_TAB: i32 = 61;
const KC_ENTER: i32 = 66;
const KC_PGUP: i32 = 92;
const KC_PGDN: i32 = 93;
const KC_HOME: i32 = 122;
const KC_ESC: i32 = 111;
const KC_END: i32 = 123;

/// 键表：[行][列]，行 0 = 上排
pub const KEYS: [[KeyDef; COLS as usize]; 2] = [
    [
        KeyDef {
            label: "ESC",
            key: Key::Direct(KC_ESC),
        },
        KeyDef {
            label: "ALT",
            key: Key::Modifier(MOD_ALT),
        },
        KeyDef {
            label: "HOME",
            key: Key::Direct(KC_HOME),
        },
        KeyDef {
            label: "PGUP",
            key: Key::Direct(KC_PGUP),
        },
        KeyDef {
            label: "↑",
            key: Key::Direct(KC_UP),
        },
        KeyDef {
            label: "PGDN",
            key: Key::Direct(KC_PGDN),
        },
        KeyDef {
            label: "SHIFT",
            key: Key::Modifier(MOD_SHIFT),
        },
    ],
    [
        KeyDef {
            label: "TAB",
            key: Key::Direct(KC_TAB),
        },
        KeyDef {
            label: "CTRL",
            key: Key::Modifier(MOD_CTRL),
        },
        KeyDef {
            label: "END",
            key: Key::Direct(KC_END),
        },
        KeyDef {
            label: "←",
            key: Key::Direct(KC_LEFT),
        },
        KeyDef {
            label: "↓",
            key: Key::Direct(KC_DOWN),
        },
        KeyDef {
            label: "→",
            key: Key::Direct(KC_RIGHT),
        },
        KeyDef {
            label: "ENTER",
            key: Key::Direct(KC_ENTER),
        },
    ],
];

/// 当前粘滞的修饰键位掩码（0 = 无）
static MODS: AtomicU8 = AtomicU8::new(0);

pub fn modifiers() -> u8 {
    MODS.load(Ordering::Relaxed)
}

/// 翻修饰键粘滞位（快捷键行 Modifier 键点按），返回新状态
pub fn toggle(bit: u8) -> u8 {
    MODS.fetch_xor(bit, Ordering::Relaxed) ^ bit
}

/// 读走并清零（一次性粘滞：commitText 落字时调用，联动一次自动灭）
pub fn take_modifiers() -> u8 {
    MODS.swap(0, Ordering::Relaxed)
}

/// 起点判定（BAR-018）：手势按下时认不认这手势归行。与渲染/hit 同一把尺——
/// 行带 = 屏底 - 键盘 inset - 行高；键盘弹起时被盖住的屏底不再算行内
pub fn in_bar(y: f64, win_h: u32, ime_bottom: u32) -> bool {
    let Some(bottom) = win_h.checked_sub(ime_bottom) else {
        return false;
    };
    let Some(top) = bottom.checked_sub(HEIGHT_PX) else {
        return false;
    };
    y >= f64::from(top) && y < f64::from(bottom)
}

/// 命中测试：窗口坐标 → 键定义。行带位置 = 屏底 - 键盘 inset - 行高
/// （键盘弹起时行跟着上浮）；y 在行带外（上方终端区/被键盘盖住的屏底）→ None
pub fn hit(x: f64, y: f64, win_w: u32, win_h: u32, ime_bottom: u32) -> Option<&'static KeyDef> {
    let bottom = win_h.checked_sub(ime_bottom)?;
    let top = bottom.checked_sub(HEIGHT_PX)?;
    if y < f64::from(top) || y >= f64::from(bottom) || x < 0.0 || x >= f64::from(win_w) {
        return None;
    }
    let row = ((y - f64::from(top)) / f64::from(ROW_H_PX)) as usize;
    let col = (x / f64::from(win_w) * COLS as f64) as usize;
    if row >= 2 || col >= COLS as usize {
        return None;
    }
    Some(&KEYS[row][col])
}
