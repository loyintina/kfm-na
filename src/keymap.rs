//! keymap.rs — 快捷键行映射（A 档纯逻辑，考题 tests/keymap_spec.rs）
//!
//! 两件契约：
//! ① map_text：修饰键（Ctrl/Alt/Shift，Java 侧一次性粘滞）× commitText
//!    文本的组合变换。kfmv4 卡片键盘的病根是 Ctrl+字母联动不上——根源是
//!    映射散在前端裸写没人判卷；这里每个组合有题盯着。
//! ② key_seq：Android 键码 → 终端字节序列。方向键/End 分普通模式与应用
//!    光标模式（对端开 ?1h 时要发 SS3 的 ESC O A，不是 CSI 的 ESC [ A）——
//!    模式位只有事件循环里的 Term 知道，所以本函数吃 app_cursor 参数，
//!    由排干侧（android_app drain_ime_inject）按当下模式翻。

/// 修饰键 × 文本 → 实际注入字节。
/// 优先级：Ctrl > Alt > Shift；多字符（中文候选落字）不转控制字节，原样过
pub fn map_text(ctrl: bool, alt: bool, shift: bool, text: &str) -> String {
    let mut chars = text.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None, // 空串或多字符：不转
    };
    let mut out = String::new();
    if alt {
        out.push('\x1b'); // Meta 惯例：Alt+X = ESC x
    }
    match single {
        // Ctrl：ASCII 可打印 & 0x1f → 控制字节（Ctrl+C=\x03 的命根）；
        // 大小写同映射（c & 0x1f 与 C & 0x1f 同值），Ctrl 优先于 Shift
        Some(c) if ctrl && c.is_ascii() && !c.is_control() => {
            out.push((c as u8 & 0x1f) as char);
        }
        // Shift：单字母大写（非字母原样）
        Some(c) if shift && c.is_ascii_alphabetic() => {
            out.push(c.to_ascii_uppercase());
        }
        _ => out.push_str(text),
    }
    out
}

/// Android 键码 → 终端字节序列。app_cursor = 对端开了应用光标模式（?1h）。
/// 未知键 → None（吞掉，不注入垃圾）
pub fn key_seq(code: i32, app_cursor: bool) -> Option<&'static str> {
    // android.view.KeyEvent 键码
    const KEYCODE_DPAD_UP: i32 = 19;
    const KEYCODE_DPAD_DOWN: i32 = 20;
    const KEYCODE_DPAD_LEFT: i32 = 21;
    const KEYCODE_DPAD_RIGHT: i32 = 22;
    const KEYCODE_TAB: i32 = 61;
    const KEYCODE_ENTER: i32 = 66;
    const KEYCODE_DEL: i32 = 67;
    const KEYCODE_PAGE_UP: i32 = 92;
    const KEYCODE_PAGE_DOWN: i32 = 93;
    const KEYCODE_ESCAPE: i32 = 111;
    const KEYCODE_MOVE_HOME: i32 = 122;
    const KEYCODE_MOVE_END: i32 = 123;
    Some(match (code, app_cursor) {
        (KEYCODE_ENTER, _) => "\r",
        (KEYCODE_DEL, _) => "\x7f",
        (KEYCODE_ESCAPE, _) => "\x1b",
        (KEYCODE_TAB, _) => "\t",
        (KEYCODE_PAGE_UP, _) => "\x1b[5~",
        (KEYCODE_PAGE_DOWN, _) => "\x1b[6~",
        // 方向键/End：应用光标模式发 SS3，普通模式发 CSI
        (KEYCODE_DPAD_UP, true) => "\x1bOA",
        (KEYCODE_DPAD_UP, false) => "\x1b[A",
        (KEYCODE_DPAD_DOWN, true) => "\x1bOB",
        (KEYCODE_DPAD_DOWN, false) => "\x1b[B",
        (KEYCODE_DPAD_RIGHT, true) => "\x1bOC",
        (KEYCODE_DPAD_RIGHT, false) => "\x1b[C",
        (KEYCODE_DPAD_LEFT, true) => "\x1bOD",
        (KEYCODE_DPAD_LEFT, false) => "\x1b[D",
        (KEYCODE_MOVE_HOME, true) => "\x1bOH",
        (KEYCODE_MOVE_END, true) => "\x1bOF",
        (KEYCODE_MOVE_HOME, false) => "\x1b[H",
        (KEYCODE_MOVE_END, false) => "\x1b[F",
        _ => return None,
    })
}
