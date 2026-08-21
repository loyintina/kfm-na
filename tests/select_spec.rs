//! select_spec.rs — 长按选择/复制考题（2026-08-21，壳层三件套之三）
//!
//! 判卷维度（全 A 档纯逻辑，状态机/坐标约定见 docs/active/壳层交互.md）：
//! - 词字符集 is_word_char（路径字符连续段算一词）
//! - 坐标换算 px_to_cell（含 MARGIN_X/动态顶带/越界钳制）
//! - 选词边界（单词/路径/空白落点）、跨行/反向扩选归一化
//! - 提取 selected_text 逐字比对：tab 还原、行尾空白 trim、行间 \n、
//!   历史区 display_offset 下的行号换算
//! - 高亮渲染钉：选中格盖 SELECT_BG 底色，清选后消失
//!
//! JNI 剪贴板/Toast 薄壳（src/clipboard.rs）是 B 档平台胶水，无考题，
//! 真机判卷（粘贴到别处 + Toast 文案）。

use kfm_na::termview::{
    self, CELL_H, CELL_W, SELECT_BG, Selection, TermView, in_selection, is_word_char, px_to_cell,
};

/// 测试字体夹具双环境解析（与 termview_spec.rs 同规则：服务器 /usr/share，
/// 手机 Termux $PREFIX/share）
fn host_font() -> fontdue::Font {
    for p in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/data/data/com.termux/files/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(p) {
            return fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .expect("fontdue 不认 DejaVuSansMono");
        }
    }
    panic!("host 测试字体缺失");
}

fn host_termview(cols: u32, rows: u32) -> TermView {
    TermView::new(host_font(), None, cols, rows, CELL_W, CELL_H)
}

/// 屏内格 (col, row) 中心的帧缓冲像素坐标（长按/拖动的落点喂法）
fn cell_center(col: u32, row: u32) -> (f64, f64) {
    (
        f64::from(termview::MARGIN_X + col * CELL_W) + f64::from(CELL_W) / 2.0,
        f64::from(termview::margin_top(CELL_H) + row * CELL_H) + f64::from(CELL_H) / 2.0,
    )
}

// ---------- 词字符集 ----------

#[test]
fn spec_选择_词字符集() {
    // 契约：字母数字 + `_-./:~` 连续段算一词（变异抽检：从 matches! 里
    // 摘掉任一字符，或 is_alphanumeric 换成 is_ascii_alphanumeric，本考题必须红）
    for c in ['a', 'Z', '5', '_', '-', '.', '/', ':', '~'] {
        assert!(is_word_char(c), "'{c}' 必须是词字符");
    }
    // 中文也算词字符（alphanumeric 含 Unicode 字母）
    assert!(is_word_char('中'), "CJK 必须是词字符");
    for c in [' ', '\t', ',', '(', ')', '|', '=', '"'] {
        assert!(!is_word_char(c), "'{c}' 不许是词字符");
    }
}

// ---------- 坐标换算（含边距/动态顶带/钳制） ----------

#[test]
fn spec_选择_坐标换算含边距与钳制() {
    // 首格内 → (0,0)；边距带里（左/上）→ 钳到 (0,0)
    assert_eq!(px_to_cell(0.0, 0.0, 8, 4, CELL_W, CELL_H), (0, 0));
    let (x, y) = cell_center(0, 0);
    assert_eq!(px_to_cell(x, y, 8, 4, CELL_W, CELL_H), (0, 0));
    let (x, y) = cell_center(5, 2);
    assert_eq!(px_to_cell(x, y, 8, 4, CELL_W, CELL_H), (5, 2));
    // 格右缘前 1px 仍属本格，过缘进下一格（floor 语义，变异抽检：
    // 改 round 必须红——round 会让后半格误判到下一格）
    let edge_x = f64::from(termview::MARGIN_X + 3 * CELL_W) - 1.0;
    let (_, cy) = cell_center(0, 1);
    assert_eq!(px_to_cell(edge_x, cy, 8, 4, CELL_W, CELL_H), (2, 1));
    assert_eq!(px_to_cell(edge_x + 1.0, cy, 8, 4, CELL_W, CELL_H), (3, 1));
    // 越界钳到网格边缘
    assert_eq!(px_to_cell(99999.0, 99999.0, 8, 4, CELL_W, CELL_H), (7, 3));
    // 顶带跟格高走：cell_h=20 时顶带是 MARGIN_Y+20，带内仍钳 row 0
    let ch = 20;
    let in_band_y = f64::from(termview::margin_top(ch)) - 1.0;
    let col5_x = f64::from(termview::MARGIN_X + 5 * CELL_W) + 1.0;
    assert_eq!(px_to_cell(col5_x, in_band_y, 8, 4, CELL_W, ch), (5, 0));
}

// ---------- 选择范围判定（归一化） ----------

#[test]
fn spec_选择_范围判定归一化() {
    let anchor = (0, 2);
    let cursor = (1, 5);
    // 闭区间含端点
    assert!(in_selection(anchor, cursor, 0, 2));
    assert!(in_selection(anchor, cursor, 1, 5));
    assert!(in_selection(anchor, cursor, 0, 4));
    assert!(in_selection(anchor, cursor, 1, 0));
    // 区间外
    assert!(!in_selection(anchor, cursor, 0, 1));
    assert!(!in_selection(anchor, cursor, 1, 6));
    assert!(!in_selection(anchor, cursor, -1, 9));
    assert!(!in_selection(anchor, cursor, 2, 0));
    // 反向（cursor 在 anchor 前）归一化后同判定
    assert!(in_selection(cursor, anchor, 0, 3));
    assert!(!in_selection(cursor, anchor, 0, 1));
}

// ---------- 选词 ----------

#[test]
fn spec_选择_长按选词边界() {
    let mut tv = host_termview(40, 4);
    tv.feed(b"git status");
    let (x, y) = cell_center(6, 0); // 'a' in "status"（列 4..9）
    tv.select_word_at(x, y);
    assert!(tv.selection_active());
    assert_eq!(tv.selected_text().as_deref(), Some("status"));
    // 词首/词末落点同结果
    let (x, _) = cell_center(4, 0);
    tv.select_word_at(x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("status"));
    let (x, _) = cell_center(9, 0);
    tv.select_word_at(x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("status"));
}

#[test]
fn spec_选择_路径整段当选() {
    // 词字符集的立意：路径/带行号串整段拎出来
    let mut tv = host_termview(40, 4);
    tv.feed(b"open /root/kfm-na/src/a.rs:12 ok");
    let (x, y) = cell_center(8, 0); // 'o' in "/root/..."
    tv.select_word_at(x, y);
    assert_eq!(
        tv.selected_text().as_deref(),
        Some("/root/kfm-na/src/a.rs:12")
    );
}

#[test]
fn spec_选择_空白落点选单格() {
    let mut tv = host_termview(40, 4);
    tv.feed(b"ab cd");
    let (x, y) = cell_center(2, 0); // 空格
    tv.select_word_at(x, y);
    assert!(tv.selection_active(), "空白落点也进选择态（单格选区）");
    // 单格空白提取 trim 后为空——壳层据此不打扰剪贴板
    assert_eq!(tv.selected_text().as_deref(), Some(""));
}

// ---------- 跨行/反向扩选与提取 ----------

#[test]
fn spec_选择_跨行扩选逐字比对() {
    let mut tv = host_termview(40, 6);
    tv.feed(b"first line\r\nsecond line\r\nthird");
    // 长按 "first"（词选 anchor=(0,0)..(0,4)），拖到第 1 行第 5 列
    let (x, y) = cell_center(2, 0);
    tv.select_word_at(x, y);
    let (x, y) = cell_center(5, 1);
    tv.extend_selection(x, y);
    // 契约：anchor 行从词首到行尾，cursor 行从行首到 cursor 列，
    // 行间补 \n，行尾空白 trim，串尾不多 \n
    assert_eq!(tv.selected_text().as_deref(), Some("first line\nsecond"));
}

#[test]
fn spec_选择_反向扩选归一化() {
    let mut tv = host_termview(40, 6);
    tv.feed(b"first line\r\nsecond line\r\nthird");
    // 从下往上拖：anchor 在 "second"（行 1 列 0..5），cursor 拖到行 0 列 2
    let (x, y) = cell_center(3, 1);
    tv.select_word_at(x, y);
    let (x, y) = cell_center(2, 0);
    tv.extend_selection(x, y);
    // 归一化后 = (0,2)..(1,5)：行 0 从列 2 起（"rst line"），行 1 到列 5
    assert_eq!(tv.selected_text().as_deref(), Some("rst line\nsecond"));
}

#[test]
fn spec_选择_tab还原与行尾trim() {
    let mut tv = host_termview(40, 4);
    tv.feed(b"a\tb");
    // 选整行：从 'a' 拖到行末（alacritty put_tab 把 '\t' 本体写进格，
    // 提取必须还原 tab；跳过的列是空白格；行尾空白 trim）
    let (x, y) = cell_center(0, 0);
    tv.select_word_at(x, y);
    let (x, _) = cell_center(39, 0);
    tv.extend_selection(x, y);
    let text = tv.selected_text().expect("必须有选区文字");
    assert!(text.contains('\t'), "tab 必须还原进提取文字: {text:?}");
    assert!(
        text.starts_with('a') && text.ends_with('b'),
        "行尾必须 trim: {text:?}"
    );
    // 逐字钉死格内容：'a' + '\t' + 6 空格 + 'b'（tab 推进到第 8 列）
    assert_eq!(text, "a\t      b");
}

#[test]
fn spec_选择_历史区滚屏后选对行() {
    // display_offset 坐标约定钉：滚 5 行后屏顶是历史行，长按屏顶选中的
    // 必须是历史行的词（不是屏底新内容）——换算含 display_offset
    let mut tv = host_termview(8, 10);
    for i in 0..30 {
        tv.feed(format!("L{i:02}\r\n").as_bytes());
    }
    tv.scroll_lines(5);
    assert_eq!(tv.display_offset(), 5);
    // 31 行网格（30 内容 + 1 空行尾）屏 10 行 → 屏顶(滚 5 后) = 网格行 16
    let (x, y) = cell_center(1, 0);
    tv.select_word_at(x, y);
    assert_eq!(
        tv.selected_text().as_deref(),
        Some("L16"),
        "滚 5 行后屏顶必须是历史行 L16"
    );
    // 扩选到屏内第 2 行 → 跨历史行提取
    let (x, y) = cell_center(2, 2);
    tv.extend_selection(x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("L16\nL17\nL18"));
}

#[test]
fn spec_选择_清选() {
    let mut tv = host_termview(40, 4);
    tv.feed(b"hello");
    let (x, y) = cell_center(2, 0);
    tv.select_word_at(x, y);
    assert!(tv.selection_active());
    tv.clear_selection();
    assert!(!tv.selection_active());
    assert_eq!(tv.selected_text(), None);
}

// ---------- 高亮渲染钉 ----------

#[test]
fn spec_选择_高亮渲染盖底色() {
    let mut tv = host_termview(40, 4);
    tv.feed(b"hello");
    let (x, y) = cell_center(2, 0);
    tv.select_word_at(x, y);
    let buf_w = 2 * termview::MARGIN_X + 40 * CELL_W;
    let buf_h = termview::margin_top(CELL_H) + 4 * CELL_H + termview::MARGIN_Y;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    // 选中格（列 0..4）整格盖 SELECT_BG：格内过半像素必须是选择底色
    // （字形墨 alpha 混合在上，边角大块还是底色）
    let cell_sel_px = |col: u32| -> usize {
        let (x0, y0) = (
            termview::MARGIN_X + col * CELL_W,
            termview::margin_top(CELL_H),
        );
        let mut n = 0;
        for y in y0..y0 + CELL_H {
            for x in x0..x0 + CELL_W {
                if buf[(y * buf_w + x) as usize] == SELECT_BG {
                    n += 1;
                }
            }
        }
        n
    };
    for col in 0..5u32 {
        assert!(
            cell_sel_px(col) as u32 > CELL_W * CELL_H / 2,
            "选中格 {col} 必须盖 SELECT_BG 底色"
        );
    }
    // 未选中格（列 6）不许有选择底色（防「整行涂满」式假实现）
    assert_eq!(cell_sel_px(6), 0, "未选中格不许有选择底色");
    // 清选后高亮消失
    tv.clear_selection();
    let mut buf2 = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf2, buf_w, buf_h);
    assert!(!buf2.contains(&SELECT_BG), "清选后帧缓冲不许再有选择底色");
}

/// Selection 结构公开面钉（壳层 android_app 调试/未来序列化用）：
/// anchor/cursor 语义不悄悄翻面
#[test]
fn spec_选择_选区结构语义() {
    let sel = Selection {
        anchor: (3, 10),
        cursor: (1, 2),
    };
    // 结构只是载体，归一化永远走 in_selection/selected_text
    assert!(in_selection(sel.anchor, sel.cursor, 2, 0));
    assert!(!in_selection(sel.anchor, sel.cursor, 4, 0));
}
