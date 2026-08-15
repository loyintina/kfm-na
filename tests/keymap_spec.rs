//! keymap_spec.rs — 快捷键行映射考题（A 档纯逻辑，答案 src/keymap.rs）
//!
//! 判卷维度：
//! - map_text：修饰键（Ctrl/Alt/Shift 一次性粘滞）× 文本的组合变换——
//!   kfmv4 卡片键盘的病根就是 Ctrl+字母联动不上，这次每个组合有题盯着
//! - key_seq：Android 键码 → 终端字节序列，方向键/End 分普通与应用光标模式
//!   （vim/kimicode 开 ?1h 后要发 ESC O A 而不是 ESC [ A）

use kfm_na::keymap::{key_seq, map_text};

// Android KeyEvent 键码（与 android.view.KeyEvent 对齐）
const ENTER: i32 = 66;
const DEL: i32 = 67;
const TAB: i32 = 61;
const ESC: i32 = 111;
const UP: i32 = 19;
const DOWN: i32 = 20;
const LEFT: i32 = 21;
const RIGHT: i32 = 22;
const PGUP: i32 = 92;
const PGDN: i32 = 93;
const END: i32 = 123;
const HOME: i32 = 122;

// ---------- map_text：修饰键 × 文本 ----------

#[test]
fn spec_修饰键_无修饰原样过() {
    assert_eq!(map_text(false, false, false, "a"), "a");
    assert_eq!(map_text(false, false, false, "你好"), "你好");
    assert_eq!(map_text(false, false, false, ""), "");
}

#[test]
fn spec_修饰键_ctrl字母转控制字节() {
    // 终端 Ctrl 的本义：字母 & 0x1f。Ctrl+C = \x03 是中断的命根
    assert_eq!(map_text(true, false, false, "c"), "\x03");
    assert_eq!(map_text(true, false, false, "a"), "\x01");
    assert_eq!(map_text(true, false, false, "d"), "\x04");
    // 大写输入（键盘自带 shift 状态时）同映射：Ctrl+Shift+A 也是 \x01
    assert_eq!(map_text(true, false, false, "C"), "\x03");
    // Ctrl+[ = ESC（vim 党刚需）
    assert_eq!(map_text(true, false, false, "["), "\x1b");
}

#[test]
fn spec_修饰键_ctrl中文不转() {
    // 中文候选落字撞上 Ctrl 粘滞：多字符/非 ASCII 不许进控制字节表，原样过
    assert_eq!(map_text(true, false, false, "你好"), "你好");
    assert_eq!(map_text(true, false, false, "ab"), "ab");
}

#[test]
fn spec_修饰键_alt前缀esc() {
    // Meta 键惯例：Alt+X = ESC x（readline 的 M-f/M-b 走这条）
    assert_eq!(map_text(false, true, false, "x"), "\x1bx");
    assert_eq!(map_text(false, true, false, "f"), "\x1bf");
    // Alt+Ctrl 组合：ESC 前缀 + 控制字节
    assert_eq!(map_text(true, true, false, "c"), "\x1b\x03");
}

#[test]
fn spec_修饰键_shift大写_ctrl优先() {
    assert_eq!(map_text(false, false, true, "a"), "A");
    assert_eq!(map_text(false, false, true, "z"), "Z");
    // Ctrl 优先于 Shift：Ctrl+Shift+a = \x01（不是 A 再转）
    assert_eq!(map_text(true, false, true, "a"), "\x01");
    // Shift 对非字母无效
    assert_eq!(map_text(false, false, true, "1"), "1");
}

// ---------- key_seq：键码 → 序列 ----------

#[test]
fn spec_键码_直接键() {
    assert_eq!(key_seq(ENTER, false), Some("\r"));
    assert_eq!(key_seq(DEL, false), Some("\x7f"));
    assert_eq!(key_seq(ESC, false), Some("\x1b"));
    assert_eq!(key_seq(TAB, false), Some("\t"));
}

#[test]
fn spec_键码_翻页与end() {
    assert_eq!(key_seq(PGUP, false), Some("\x1b[5~"));
    assert_eq!(key_seq(PGDN, false), Some("\x1b[6~"));
    // End/Home：普通模式 CSI H/F，应用光标模式 SS3
    assert_eq!(key_seq(END, false), Some("\x1b[F"));
    assert_eq!(key_seq(END, true), Some("\x1bOF"));
    assert_eq!(key_seq(HOME, false), Some("\x1b[H"));
    assert_eq!(key_seq(HOME, true), Some("\x1bOH"));
}

#[test]
fn spec_键码_方向键分模式() {
    // 普通模式：CSI 序列
    assert_eq!(key_seq(UP, false), Some("\x1b[A"));
    assert_eq!(key_seq(DOWN, false), Some("\x1b[B"));
    assert_eq!(key_seq(RIGHT, false), Some("\x1b[C"));
    assert_eq!(key_seq(LEFT, false), Some("\x1b[D"));
    // 应用光标模式（?1h，vim/kimicode 会开）：SS3 序列
    assert_eq!(key_seq(UP, true), Some("\x1bOA"));
    assert_eq!(key_seq(DOWN, true), Some("\x1bOB"));
    assert_eq!(key_seq(RIGHT, true), Some("\x1bOC"));
    assert_eq!(key_seq(LEFT, true), Some("\x1bOD"));
}

#[test]
fn spec_键码_未知键吞掉() {
    assert_eq!(key_seq(9999, false), None);
    assert_eq!(key_seq(0, true), None);
}
