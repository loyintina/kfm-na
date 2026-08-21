//! select_spec.rs — 长按选择/复制考题（2026-08-21，壳层三件套之三）
//!
//! 判卷维度（全 A 档纯逻辑，状态机/坐标约定见 docs/active/壳层交互.md）：
//! - 词字符集 is_word_char（路径字符连续段算一词）
//! - 坐标换算 px_to_cell（含 MARGIN_X/动态顶带/越界钳制）
//! - 选词边界（单词/路径/空白落点）、跨行/反向扩选归一化
//! - 提取 selected_text 逐字比对：tab 还原、行尾空白 trim、行间 \n、
//!   历史区 display_offset 下的行号换算
//! - 高亮渲染钉：选中格盖 SELECT_BG 底色，清选后消失；CJK 双宽字
//!   两格都盖高亮且右半字形墨不被盖（两遍制渲染钉）
//! - 拖柄：命中判定（±1 格触控宽容）、端点移动（含互换）、水滴渲染钉
//!   （青色主体 + 近黑描边，kfmv4 色板字面量钉）
//! - 放大镜：2 倍最近邻映射逐点比对、边框、屏内钳制
//! - 宽字符边界钳制：端点落 CJK spacer 半格按拖动方向钳（右 col+1 /
//!   左 col-1），选词/扩选/拖柄三入口同尺；提取一致性（探针
//!   set_selection_raw）；渲染整字扩边不劈字
//!
//! JNI 剪贴板/Toast 薄壳（src/clipboard.rs）是 B 档平台胶水，无考题，
//! 真机判卷（粘贴到别处 + Toast 文案）。

use kfm_na::termview::{
    self, CELL_H, CELL_W, HANDLE_CYAN, HANDLE_DARK, HandleEnd, MAG_BORDER, SELECT_BG, Selection,
    TermView, handle_hit, handle_radius, in_selection, is_word_char, px_to_cell,
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

// ---------- 高亮 × CJK 双宽（2026-08-21 实拍「选中态中文只剩左半」） ----------

/// 内嵌 CJK/符号字体（FusionPixel，双环境同一份文件——compile-time
/// include_bytes，夹具恒定）：真 CJK 双宽字形源
fn fusion_font() -> fontdue::Font {
    fontdue::Font::from_bytes(
        termview::VENDORED_CJK_FONT,
        fontdue::FontSettings::default(),
    )
    .expect("内嵌 CJK 字体必须可解析")
}

#[test]
fn spec_bar025_选择_高亮_cjk双宽字两格完整() {
    // 病灶：单遍绘制时宽字符格 0 画双宽字形、墨探进格 1，随后 spacer 格的
    // SELECT_BG 背景填充把右半字形盖掉。两遍制（先全背景后全字形）修复。
    // 契约：'中' 的两格都必须 SELECT_BG 过半（两格都盖高亮）且都有字形墨
    // （右半不许被盖）。变异抽检：render_into 回退单遍制，本考题必须红
    let mut tv = TermView::new(host_font(), Some(fusion_font()), 20, 4, CELL_W, CELL_H);
    assert!(
        prefer_cjk_fixture_check(),
        "夹具前提：主字体必须缺 '中'（走 CJK 双宽路径）"
    );
    tv.feed("中文ab".as_bytes());
    let (x, y) = cell_center(0, 0);
    tv.select_word_at(x, y); // 选中 '中'（词扩展被 spacer 挡在格 0）
    let (x, y) = cell_center(5, 0);
    tv.extend_selection(x, y); // 扩到 'b'，覆盖 '中' 的两格
    let buf_w = 2 * termview::MARGIN_X + 20 * CELL_W;
    let buf_h = termview::margin_top(CELL_H) + 4 * CELL_H + termview::MARGIN_Y;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    // 格内 (SELECT_BG 像素数, 墨像素数)：墨 = 非高亮非背景的混合像素
    let stats = |col: u32| -> (usize, usize) {
        let x0 = termview::MARGIN_X + col * CELL_W;
        let y0 = termview::margin_top(CELL_H);
        let (mut bg_n, mut ink) = (0, 0);
        for y in y0..y0 + CELL_H {
            for x in x0..x0 + CELL_W {
                let p = buf[(y * buf_w + x) as usize];
                if p == SELECT_BG {
                    bg_n += 1;
                } else if p != termview::DEFAULT_BG {
                    ink += 1;
                }
            }
        }
        (bg_n, ink)
    };
    for col in [0u32, 1] {
        let (bg_n, ink) = stats(col);
        assert!(
            bg_n as u32 > CELL_W * CELL_H / 2,
            "'中' 的格 {col} 必须盖高亮底色"
        );
        assert!(ink > 0, "'中' 的格 {col} 必须有字形墨——右半不许被底色盖掉");
    }
}

/// 夹具前提检查：host 主字体（DejaVuSansMono）缺 '中'
fn prefer_cjk_fixture_check() -> bool {
    host_font().lookup_glyph_index('中') == 0
}

// ---------- 选择拖柄（2026-08-21，Termux 语义借鉴） ----------

#[test]
fn spec_选择_拖柄命中判定() {
    // 纯函数边界（触控宽容 ±1 格，闭区间；变异抽检：改 < 或放宽到 ±2 格必须红）
    assert!(handle_hit(
        100.0,
        100.0,
        100.0 + f64::from(CELL_W),
        100.0,
        CELL_W,
        CELL_H
    ));
    assert!(!handle_hit(
        100.0,
        100.0,
        100.0 + f64::from(CELL_W) + 1.0,
        100.0,
        CELL_W,
        CELL_H
    ));
    assert!(handle_hit(
        100.0,
        100.0,
        100.0,
        100.0 + f64::from(CELL_H),
        CELL_W,
        CELL_H
    ));
    assert!(!handle_hit(
        100.0,
        100.0,
        100.0,
        100.0 + f64::from(CELL_H) + 1.0,
        CELL_W,
        CELL_H
    ));
    // 端到端：选词后柄心命中起/止端，两柄中间不命中
    let mut tv = host_termview(20, 4);
    tv.feed(b"hello world");
    let (x, y) = cell_center(1, 0);
    tv.select_word_at(x, y); // "hello" = (0,0)..(0,4)
    let (start, end) = tv.selection_handles().expect("选区在屏内");
    let (sx, sy) = start.expect("起端柄在屏内");
    let (ex, ey) = end.expect("止端柄在屏内");
    assert_eq!(tv.hit_handle(sx, sy), Some(HandleEnd::Start));
    assert_eq!(tv.hit_handle(ex, ey), Some(HandleEnd::End));
    // 两柄之间（col 2 格心同高度）距两端都 >1 格 → None
    let mid_x = f64::from(termview::MARGIN_X + 2 * CELL_W) + f64::from(CELL_W) / 2.0;
    assert_eq!(tv.hit_handle(mid_x, sy), None);
    // 无选区 → None
    tv.clear_selection();
    assert_eq!(tv.hit_handle(sx, sy), None);
}

#[test]
fn spec_选择_拖柄移动端点() {
    // "hello world"：h0 e1 l2 l3 o4 空5 w6 o7 r8 l9 d10
    let mut tv = host_termview(20, 4);
    tv.feed(b"hello world");
    let (x, y) = cell_center(1, 0);
    tv.select_word_at(x, y); // "hello"
    // 拖止端右移 3 格 → 纳入 " wo"
    let (x, y) = cell_center(7, 0);
    tv.move_selection_end(HandleEnd::End, x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("hello wo"));
    // 拖起端右移 2 格 → 掐头
    let (x, y) = cell_center(2, 0);
    tv.move_selection_end(HandleEnd::Start, x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("llo wo"));
    // 起端拖过止端 → 角色互换（起点变新终点），选区不塌缩
    let (x, y) = cell_center(9, 0);
    tv.move_selection_end(HandleEnd::Start, x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("orl"));
    // 跨行：止端拖到第 1 行
    let mut tv = host_termview(20, 6);
    tv.feed(b"first\r\nsecond");
    let (x, y) = cell_center(1, 0);
    tv.select_word_at(x, y); // "first"
    let (x, y) = cell_center(3, 1);
    tv.move_selection_end(HandleEnd::End, x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("first\nseco"));
}

#[test]
fn spec_选择_拖柄渲染钉() {
    // 水滴/图钉（kfmv4「黑边 + 亮色辉光」像素版）：圆头中心 = 青色主体，
    // 圆头外沿一圈 = 近黑描边；柄体从格底缘连到圆头，侧沿描边。
    // （变异抽检：摘掉描边层或柄体段，本考题必须红）
    let mut tv = host_termview(20, 4);
    tv.feed(b"hello world");
    let (x, y) = cell_center(1, 0);
    tv.select_word_at(x, y);
    let buf_w = 2 * termview::MARGIN_X + 20 * CELL_W;
    let buf_h = termview::margin_top(CELL_H) + 4 * CELL_H + termview::MARGIN_Y;
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    let r = handle_radius(CELL_W);
    let (start, end) = tv.selection_handles().expect("选区在屏内");
    for hp in [start, end] {
        let (hx, cy) = hp.expect("两端柄都在屏内");
        let (hx, cy) = (hx as u32, cy as u32);
        let px = |x: u32, y: u32| buf[(y * buf_w + x) as usize];
        assert_eq!(px(hx, cy), HANDLE_CYAN, "圆头中心必须是青色主体");
        assert_eq!(
            px(hx + r + 1, cy),
            HANDLE_DARK,
            "圆头右侧沿必须是近黑描边（描边比主体大一圈）"
        );
        // 柄体：格底缘 = 柄心上行 (CELL_H - r) 处；柄心上方 4px 处必在柄体段
        let cell_bottom = cy + r - CELL_H;
        assert_eq!(px(hx, cell_bottom + 4), HANDLE_CYAN, "柄体中线必须是青色");
        assert_eq!(
            px(hx + r / 2 + 1, cell_bottom + 4),
            HANDLE_DARK,
            "柄体侧沿必须是近黑描边"
        );
    }
    tv.clear_selection();
    let mut buf2 = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf2, buf_w, buf_h);
    assert!(!buf2.contains(&HANDLE_CYAN), "清选后拖柄必须消失");
}

// ---------- 放大镜（拖柄拖动中浮窗） ----------

#[test]
fn spec_放大镜_两倍最近邻与边框钳制() {
    // 源区 = 触点格 ±5 格 × ±3 行 → 窗 360x432（18x36 格）。帧缓冲给足
    let mut tv = host_termview(20, 10);
    tv.feed(b"ABCDEFGHIJ\r\nKLMNOPQRST\r\nUVWXYZabcd");
    let (buf_w, buf_h) = (500u32, 700u32);
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    // 触点 = 格 (5,1)（'P'）格心；源区中心对齐该格心
    let (tx, ty) = cell_center(5, 1);
    let (cx, cy) = (tx as u32, ty as u32);
    // 源区中心行在放大前的原始像素带（放大镜会覆盖 buf，先拷对照行）
    let src_row: Vec<u32> = buf[(cy * buf_w) as usize..((cy + 1) * buf_w) as usize].to_vec();
    tv.render_magnifier(&mut buf, buf_w, buf_h, tx, ty);
    let win_w = 5 * CELL_W * 2 * 2; // 360
    let win_h = 3 * CELL_H * 2 * 2; // 432
    // 触点格 (5,1) 格心 = (111, 102)：横向 111-180 < 0 → 钳 0；
    // 纵向 102-60-432 < 0 → 钳 0。钳制本身也是被钉的行为
    let win_x = 0u32;
    let win_y = 0u32;
    // 边框：窗顶中点上沿必须是边框色（圆角切角，取中点避开）
    assert_eq!(
        buf[(win_y * buf_w + win_x + win_w / 2) as usize],
        MAG_BORDER,
        "浮窗外圈必须是边框色"
    );
    // 最近邻 2 倍映射逐点比对：dest(中心 + 2k) == src(中心 + k)
    // （变异抽检：MAG_ZOOM 改 1 或中心不对齐格心，本考题必须红）
    for k in (-60i32..=60).step_by(13) {
        let dest_x = (win_x + win_w / 2) as i32 + 2 * k;
        let dest = buf[((win_y + win_h / 2) * buf_w) as usize + dest_x as usize];
        let src = src_row[(cx as i32 + k) as usize];
        assert_eq!(dest, src, "映射点 k={k} 必须满足 2 倍最近邻");
    }
    // 内容非空：窗内必须有墨（放大的是真字形不是一窗黑）
    let mut ink = 0;
    for y in (win_y + 4)..(win_y + win_h - 4) {
        for x in (win_x + 4)..(win_x + win_w - 4) {
            let p = buf[(y * buf_w + x) as usize];
            if p != termview::DEFAULT_BG && p != MAG_BORDER {
                ink += 1;
            }
        }
    }
    assert!(ink > 1000, "放大窗内必须有大量字形墨（实得 {ink}）");
}

// ---------- kfmv4 色板（2026-08-21 品牌统一） ----------

#[test]
fn spec_视觉_kfmv4色板() {
    // 字面量钉死（变异抽检：改任一色值/半径比例，本考题必须红）
    assert_eq!(SELECT_BG, 0x003B_82F6, "选中底色 = kfmv4 正蓝 #3B82F6");
    assert_eq!(HANDLE_CYAN, 0x0006_B6D4, "拖柄主体 = kfmv4 青 #06B6D4");
    assert_eq!(HANDLE_DARK, 0x000A_0C10, "拖柄描边 = 近黑 #0A0C10");
    assert_eq!(MAG_BORDER, HANDLE_CYAN, "放大镜边框与拖柄同青色系");
    // 圆头直径 ≈0.7 格宽：半径 = cell_w*7/20，下限 2
    assert_eq!(handle_radius(CELL_W), 6, "18 格宽 → 半径 6");
    assert_eq!(handle_radius(10), 3);
    assert_eq!(handle_radius(4), 2, "半径下限 2");
}

// ---------- 宽字符边界钳制（2026-08-21，端点永不劈 CJK 字） ----------
// 场景 "a中bc"：a0 中1(格0) sp2(spacer) b3 c4
// 场景 "a 中bc"（带空格）：a0 空1 中2 sp3 b4 c5

fn cjk_termview(text: &str) -> TermView {
    assert!(
        prefer_cjk_fixture_check(),
        "夹具前提：主字体必须缺 '中'（走 CJK 双宽路径）"
    );
    let mut tv = TermView::new(host_font(), Some(fusion_font()), 20, 6, CELL_W, CELL_H);
    tv.feed(text.as_bytes());
    tv
}

#[test]
fn spec_选择_宽字符_spacer落点选词() {
    let mut tv = cjk_termview("a中bc");
    // 按在 spacer 半格（col 2）→ 当作按在该字格 0 → 词 "a中" 整选，
    // 词尾宽字符带上 spacer（end=2）；提取跳 spacer → "a中"（不含 b）
    let (x, y) = cell_center(2, 0);
    tv.select_word_at(x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("a中"));
}

#[test]
fn spec_选择_宽字符_右拖钳到下一格() {
    // 右移落在 spacer 半格 → 钳到 col+1（越过该字到下一格）。固有结果
    // （实拍判卷点）：后一格非空白时多带一个字进选区
    let mut tv = cjk_termview("a 中bc");
    let (x, y) = cell_center(0, 0);
    tv.select_word_at(x, y); // 'a' 单格（空格挡词扩展）(0,0)..(0,0)
    let (x, y) = cell_center(3, 0); // spacer 格
    tv.extend_selection(x, y);
    assert_eq!(
        tv.selected_text().as_deref(),
        Some("a 中b"),
        "右拖落 spacer 钳到 col+1：'b' 被包进选区"
    );
}

#[test]
fn spec_选择_宽字符_左拖钳回字首() {
    // 左移落在 spacer 半格 → 钳到 col-1（该字格 0），整个中字进选区
    let mut tv = cjk_termview("a 中bc");
    let (x, y) = cell_center(4, 0);
    tv.select_word_at(x, y); // "bc" (0,4)..(0,5)
    let (x, y) = cell_center(3, 0); // spacer 格，向左拖
    tv.extend_selection(x, y);
    assert_eq!(
        tv.selected_text().as_deref(),
        Some("中bc"),
        "左拖落 spacer 钳到格 0：整字进选区"
    );
}

#[test]
fn spec_选择_宽字符_跨行中西文钳制() {
    // "中ab\r\ncd中e"：行0 中0 sp1 a2 b3；行1 c0 d1 中2 sp3 e4
    let mut tv = cjk_termview("中ab\r\ncd中e");
    let (x, y) = cell_center(2, 0);
    tv.select_word_at(x, y); // "ab" (0,2)..(0,3)
    // 拖到行 1 的 '中' 格 0（非 spacer，不钳）
    let (x, y) = cell_center(2, 1);
    tv.extend_selection(x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("ab\ncd中"));
    // 继续右拖到行 1 的 spacer 格 → 钳到 col+1，带上 'e'
    let (x, y) = cell_center(3, 1);
    tv.extend_selection(x, y);
    assert_eq!(tv.selected_text().as_deref(), Some("ab\ncd中e"));
}

#[test]
fn spec_选择_宽字符_拖柄路径同钳() {
    // move_selection_end 与 extend_selection 同一把钳（方向 = 新落点 vs
    // 被拖端旧位置）：起端柄右拖到 spacer → 钳 col+1
    let mut tv = cjk_termview("a中bc");
    let (x, y) = cell_center(0, 0);
    tv.select_word_at(x, y); // "a中" (0,0)..(0,2)
    let (x, y) = cell_center(2, 0); // 起端柄右移到 spacer 格
    tv.move_selection_end(HandleEnd::Start, x, y);
    assert_eq!(
        tv.selected_text().as_deref(),
        Some("b"), // 钳到 col 3：选区 (0,3)..(0,2) 互换后 = 格 3 单格
        "起端柄右拖落 spacer 必须同钳 col+1"
    );
}

#[test]
fn spec_选择_宽字符钳制提取一致性() {
    // 探针 set_selection_raw 把端点人为放到 spacer 上，钉死两条等价：
    //   start 在 spacer ≡ start 钳右 col+1（提取都不含该字）
    //   end 在 spacer ≡ end 钳左 col-1（提取都含该字）
    let mut tv = cjk_termview("a中bc");
    tv.set_selection_raw((0, 2), (0, 4)); // start 落 spacer
    let raw_start = tv.selected_text();
    tv.set_selection_raw((0, 3), (0, 4)); // ≡ 钳右 col+1
    assert_eq!(raw_start, tv.selected_text());
    assert_eq!(raw_start.as_deref(), Some("bc"));
    tv.set_selection_raw((0, 0), (0, 2)); // end 落 spacer
    let raw_end = tv.selected_text();
    tv.set_selection_raw((0, 0), (0, 1)); // ≡ 钳左 col-1
    assert_eq!(raw_end, tv.selected_text());
    assert_eq!(raw_end.as_deref(), Some("a中"));
}

#[test]
fn spec_选择_宽字符_渲染整字扩边() {
    // 渲染层高亮扩到整字边界：spacer 的格 0 选中 → spacer 也亮；
    // 格 0 的 spacer 选中 → 格 0 也亮——任何钳法下高亮都不劈字
    // （变异抽检：摘掉 render_into 收集段的扩边判定，本考题必须红）
    let mut tv = cjk_termview("a中bc");
    let buf_w = 2 * termview::MARGIN_X + 20 * CELL_W;
    let buf_h = termview::margin_top(CELL_H) + 6 * CELL_H + termview::MARGIN_Y;
    let lit = |buf: &[u32], col: u32| -> bool {
        let (x0, y0) = (
            termview::MARGIN_X + col * CELL_W,
            termview::margin_top(CELL_H),
        );
        let mut n = 0u32;
        for y in y0..y0 + CELL_H {
            for x in x0..x0 + CELL_W {
                if buf[(y * buf_w + x) as usize] == SELECT_BG {
                    n += 1;
                }
            }
        }
        n > CELL_W * CELL_H / 2
    };
    // 只选 '中' 的格 0（探针绕钳）→ spacer 格 2 必须随格 0 同亮
    tv.set_selection_raw((0, 1), (0, 1));
    let mut buf = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf, buf_w, buf_h);
    assert!(lit(&buf, 1), "格 0 必须亮");
    assert!(lit(&buf, 2), "spacer 格必须随格 0 同亮（扩边）");
    assert!(!lit(&buf, 3), "选区外的 'b' 不许亮");
    // 反向：选区从 spacer 起 → 格 0 必须随 spacer 同亮
    tv.set_selection_raw((0, 2), (0, 4));
    let mut buf2 = vec![0u32; (buf_w * buf_h) as usize];
    tv.render_into(&mut buf2, buf_w, buf_h);
    assert!(lit(&buf2, 1), "格 0 必须随 spacer 同亮（扩边）");
    assert!(lit(&buf2, 2) && lit(&buf2, 4));
    assert!(!lit(&buf2, 0), "选区外的 'a' 不许亮");
}
