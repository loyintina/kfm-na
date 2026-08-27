//! touch_spec.rs — 通道八 touch-in 解析器考题(A 档,2026-08-27)
//!
//! 契约:
//! ①脚本行 → TouchCmd,五种指令语法钉死(tap/down/move/up/scroll/sleep);
//! ②空行/# 注释跳过,坏行收进错误清单不炸解析;
//! ③down/move/up 默认指 id=90,可显式给第二指(捏合用);
//! ④sleep 封顶 10s(写错脚本不许焊死主循环节拍器);
//! ⑤scroll 0 是坏行(无意义指令不许悄悄漏进队列)。
//! 变异抽检口径:改坏解析(如 scroll 符号颠倒/坏行静默吞)本文件必须红。

use kfm_na::gate::{TouchCmd, parse_touch_line, parse_touch_script};

#[test]
fn spec_touch_五种指令语法钉死() {
    assert_eq!(
        parse_touch_line("tap 100 200"),
        Some(Ok(TouchCmd::Tap { x: 100.0, y: 200.0 }))
    );
    assert_eq!(
        parse_touch_line("down 10 20"),
        Some(Ok(TouchCmd::Down {
            id: 90,
            x: 10.0,
            y: 20.0
        }))
    );
    assert_eq!(
        parse_touch_line("move 11 21 91"),
        Some(Ok(TouchCmd::Move {
            id: 91,
            x: 11.0,
            y: 21.0
        }))
    );
    assert_eq!(
        parse_touch_line("up 12 22"),
        Some(Ok(TouchCmd::Up {
            id: 90,
            x: 12.0,
            y: 22.0
        }))
    );
    assert_eq!(
        parse_touch_line("scroll 3"),
        Some(Ok(TouchCmd::Scroll { lines: 3 }))
    );
    assert_eq!(
        parse_touch_line("scroll -2"),
        Some(Ok(TouchCmd::Scroll { lines: -2 }))
    );
    assert_eq!(
        parse_touch_line("sleep 600"),
        Some(Ok(TouchCmd::Sleep { ms: 600 }))
    );
}

#[test]
fn spec_touch_空行注释跳过_坏行收编() {
    assert_eq!(parse_touch_line(""), None);
    assert_eq!(parse_touch_line("   "), None);
    assert_eq!(parse_touch_line("# 注释"), None);
    // 坏行: Some(Err),不是 None(不许和注释混),不是 panic
    assert!(matches!(parse_touch_line("fly 1 2"), Some(Err(_))));
    assert!(matches!(parse_touch_line("tap 1"), Some(Err(_))));
    assert!(matches!(parse_touch_line("tap a b"), Some(Err(_))));
    assert!(matches!(parse_touch_line("down 1 2 x"), Some(Err(_))));
    assert!(matches!(parse_touch_line("scroll 0"), Some(Err(_)))); // 无意义指令
    assert!(matches!(parse_touch_line("sleep x"), Some(Err(_))));
}

#[test]
fn spec_touch_sleep封顶10s() {
    assert_eq!(
        parse_touch_line("sleep 99999"),
        Some(Ok(TouchCmd::Sleep { ms: 10_000 })),
        "超限必须钳到 10s,不许原样进队列"
    );
}

#[test]
fn spec_touch_整段解析_好坏分流() {
    let script = "# 复现脚本\n\ntap 100 200\n坏掉的一行\nscroll 3\nsleep abc\ndown 1 2\n";
    let (cmds, errs) = parse_touch_script(script);
    assert_eq!(cmds.len(), 3, "三行好的必须全进");
    assert_eq!(errs.len(), 2, "两行坏的一条不许丢");
    assert!(errs.iter().all(|e| e.contains("坏行")), "错误要指认原行");
}
