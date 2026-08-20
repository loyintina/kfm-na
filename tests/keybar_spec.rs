//! keybar_spec.rs — Rust 自绘快捷键行考题（A 档纯逻辑，答案 src/keybar.rs）
//!
//! 判卷维度：
//! - 布局契约：两排七列、↑↓ 严格同列（第 6 列）、方向十字锚最右、
//!   第 4 列空位（2026-08-14 与用户定稿，右手惯用）
//! - 命中测试：窗口坐标 → 键；行带上方/界外 → None；空位 → Key::None
//! - 修饰键一次性粘滞：toggle 点亮/再点灭，take 读走即清

use kfm_na::keybar::{self, HEIGHT_PX, Key, KeyDef, MOD_CTRL, MOD_SHIFT, ROW_H_PX};

// 假想窗口 700x740：格宽 100，行带顶 = 740 - 240 = 500
const W: u32 = 700;
const H: u32 = 740;

fn hit_col_row(col: usize, row: usize) -> &'static KeyDef {
    hit_col_row_ime(col, row, 0)
}

fn hit_col_row_ime(col: usize, row: usize, ime: u32) -> &'static KeyDef {
    let x = (col * 100 + 50) as f64;
    let y = (H - ime - HEIGHT_PX + row as u32 * ROW_H_PX + ROW_H_PX / 2) as f64;
    keybar::hit(x, y, W, H, ime).expect("格心必须命中")
}

#[test]
fn spec_布局_键表与定稿一致() {
    // 上排：Esc Alt Home PgUp ↑ PgDn Shift（2026-08-14 二稿定稿）
    let top: Vec<&str> = keybar::KEYS[0].iter().map(|k| k.label).collect();
    assert_eq!(
        top,
        vec!["ESC", "ALT", "HOME", "PGUP", "↑", "PGDN", "SHIFT"]
    );
    // 下排：Tab Ctrl End ← ↓ → Enter
    let bot: Vec<&str> = keybar::KEYS[1].iter().map(|k| k.label).collect();
    assert_eq!(bot, vec!["TAB", "CTRL", "END", "←", "↓", "→", "ENTER"]);
    // ↑↓ 严格同列（第 5 列，索引 4）；方向十字 ←↓→ 横排在 3,4,5
    assert_eq!(keybar::KEYS[0][4].label, "↑");
    assert_eq!(keybar::KEYS[1][4].label, "↓");
    assert_eq!(keybar::KEYS[1][3].label, "←");
    assert_eq!(keybar::KEYS[1][5].label, "→");
}

#[test]
fn spec_命中_格心归本键() {
    assert_eq!(hit_col_row(0, 0).key, Key::Direct(111)); // ESC
    assert_eq!(hit_col_row(1, 1).key, Key::Modifier(MOD_CTRL)); // CTRL
    assert_eq!(hit_col_row(2, 0).key, Key::Direct(122)); // HOME
    assert_eq!(hit_col_row(4, 0).key, Key::Direct(19)); // ↑
    assert_eq!(hit_col_row(6, 1).key, Key::Direct(66)); // ENTER
    assert_eq!(hit_col_row(3, 1).key, Key::Direct(21)); // ←
}

#[test]
fn spec_命中_带外() {
    // 行带上方（终端区）不命中
    assert!(keybar::hit(350.0, (H - HEIGHT_PX - 1) as f64, W, H, 0).is_none());
    // 界外不命中
    assert!(keybar::hit(350.0, H as f64 + 1.0, W, H, 0).is_none());
    assert!(keybar::hit(W as f64 + 1.0, (H - 10) as f64, W, H, 0).is_none());
}

#[test]
fn spec_命中_跟随键盘上浮() {
    // 实拍病灶（16777485）：键盘弹起时行画死在屏底被键盘盖住。
    // 契约：行带位置 = 屏高 - 键盘 inset - 行高——键盘弹起 300px 时，
    // 命中区整体抬 300px；原屏底位置（被键盘盖住）不再命中
    assert_eq!(hit_col_row_ime(4, 0, 300).label, "↑");
    assert_eq!(hit_col_row_ime(6, 1, 300).label, "ENTER");
    assert!(
        keybar::hit(350.0, (H - 10) as f64, W, H, 300).is_none(),
        "键盘盖住的原行位不许再命中"
    );
}

#[test]
fn spec_bar018_起点判定_跟随键盘上浮() {
    // 实拍病灶（16777488）：键盘弹起（inset=300）时点行带无响应——
    // Started 的 in_bar 判定没减 inset，认的是被键盘盖住的屏底 240px。
    // 契约：起点判定与渲染/hit 同一把尺——行带 = 屏高 - inset - 行高
    assert!(keybar::in_bar((H - 300 - HEIGHT_PX + 10) as f64, H, 300));
    assert!(
        !keybar::in_bar((H - 10) as f64, H, 300),
        "键盘盖住的原行位不许认作行内"
    );
    assert!(keybar::in_bar((H - HEIGHT_PX + 10) as f64, H, 0));
    assert!(!keybar::in_bar((H - HEIGHT_PX - 1) as f64, H, 0));
}

#[test]
fn spec_修饰键_一次性粘滞() {
    // 点亮 → 读走即清；再点灭；take 后回零
    // （2026-08-16 迁移：评审明示批准——断言一字不改，具身从进程静态搬到
    // ModifierState 实例，input-ime 插件化方案 A）
    let mods = keybar::ModifierState::new();
    assert_eq!(mods.peek(), 0, "开考必须无粘滞");
    mods.toggle(MOD_CTRL);
    assert_eq!(mods.peek(), MOD_CTRL, "点亮 Ctrl");
    mods.toggle(MOD_SHIFT);
    assert_eq!(mods.peek(), MOD_CTRL | MOD_SHIFT, "双粘滞并存");
    let taken = mods.take();
    assert_eq!(taken, MOD_CTRL | MOD_SHIFT, "take 读走全部");
    assert_eq!(mods.peek(), 0, "take 后必须清零（联动一次自动灭）");
    mods.toggle(MOD_CTRL);
    mods.toggle(MOD_CTRL);
    assert_eq!(mods.peek(), 0, "再点一次必须灭");
}
