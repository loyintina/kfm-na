//! grid_text_spec.rs — 网格文字引擎考卷（BAR-178，A 档）。
//! 判卷对象：src/ui/grid_text.rs（格宽分类 / 总格数 / 格折行，全纯函数）。
//!
//! 变异抽检预期（先改坏、看对应考题变红，cp 备份恢复——禁用 git checkout）：
//! - `char_cells` 全返 1 → 「真值表」与「混排总格数」红；
//! - `grid_wrap` 硬断改 +1 格（`acc + w > max_cells` 改 `>=` 等价病）→
//!   「恰好满格不断」红；
//! - `grid_wrap` 不吃空格断点（断点分支删空格优先）→ 「多空格优选断点」红。

use kfm_na::ui::grid_text::{
    char_cells, field_value_col_px, field_value_lines, grid_text_cells, grid_wrap,
};

// ── 格宽分类 ────────────────────────────────────────────────────────

#[test]
fn spec_char_cells_真值表() {
    assert_eq!(char_cells('中'), 2, "CJK 全角 = 2 格");
    assert_eq!(char_cells('M'), 1, "半角大写 = 1 格");
    assert_eq!(char_cells('a'), 1);
    assert_eq!(char_cells('\0'), 0, "NUL 控制符 = 0 格");
    assert_eq!(char_cells('\t'), 0, "tab 控制符 = 0 格");
    assert_eq!(char_cells('\u{7f}'), 0, "DEL 控制符 = 0 格");
    // 全角标点：0x2E80 土判据的盲区（U+3000–U+303F 在 0x2E80 之前），
    // 唯一源必须判 2 格——本钉就是「收编土判据」的物质证据
    assert_eq!(char_cells('，'), 2, "全角逗号 U+FF0C = 2 格");
    assert_eq!(char_cells('。'), 2, "句号 U+3002 = 2 格");
    assert_eq!(char_cells('🙂'), 2, "emoji = 2 格");
    // 零宽连词符不占格（终端同口径）
    assert_eq!(char_cells('\u{200d}'), 0, "ZWJ 零宽 = 0 格");
    // 箭头块 U+2190–U+21FF = 2 格（BAR-199 与 unicode-width 的唯一有意
    // 分叉：Ambiguous 墨宽 ≈30px > 1 格会被格盒裁残——承影 0075 实测）
    assert_eq!(char_cells('→'), 2, "U+2192 箭头 = 2 格（BAR-199 分叉）");
    assert_eq!(char_cells('←'), 2, "U+2190 箭头 = 2 格（BAR-199 分叉）");
    assert_eq!(char_cells('↑'), 2, "U+2191 箭头 = 2 格（BAR-199 分叉）");
    assert_eq!(char_cells('↓'), 2, "U+2193 箭头 = 2 格（BAR-199 分叉）");
    assert_eq!(char_cells('\u{21ff}'), 2, "箭头块末 = 2 格");
    assert_eq!(char_cells('\u{2200}'), 1, "块外 ∀ 不受分叉影响");
}

#[test]
fn spec_grid_text_cells_混排() {
    assert_eq!(grid_text_cells(""), 0);
    assert_eq!(grid_text_cells("abc"), 3);
    assert_eq!(grid_text_cells("中文"), 4);
    assert_eq!(grid_text_cells("a中b。"), 1 + 2 + 1 + 2);
    assert_eq!(grid_text_cells("中\0中"), 4, "控制符不占格");
}

// ── 格折行 ──────────────────────────────────────────────────────────

/// 折行结果转切片便于断言（返回每行的字符串）
fn wrap_strs(text: &str, max: u32) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    grid_wrap(text, max)
        .into_iter()
        .map(|(a, b)| chars[a..b].iter().collect())
        .collect()
}

#[test]
fn spec_grid_wrap_恰好满格不断() {
    // 4 格容量装 4 格（中+两半角）：恰好满 = 装得下，不许断（> 才断）
    assert_eq!(wrap_strs("中ab", 4), vec!["中ab"]);
    assert_eq!(wrap_strs("abcd", 4), vec!["abcd"]);
}

#[test]
fn spec_grid_wrap_超一格断() {
    // 5 格进 4 格行：硬断（无空格）
    assert_eq!(wrap_strs("abcde", 4), vec!["abcd", "e"]);
    // 全角字不许劈半：3 格处放 2 格字超 4 → 整字让到下行
    assert_eq!(wrap_strs("abc中d", 4), vec!["abc", "中d"]);
}

#[test]
fn spec_grid_wrap_长词硬断() {
    // 单字超行宽：行首第一字恒收（不死循环、不吞字）
    assert_eq!(wrap_strs("中", 1), vec!["中"]);
    assert_eq!(wrap_strs("中中", 2), vec!["中", "中"]);
    // 连续硬断：9 格长词进 4 格行 = 4/4/1
    assert_eq!(wrap_strs("abcdefghi", 4), vec!["abcd", "efgh", "i"]);
}

#[test]
fn spec_grid_wrap_多空格优选断点() {
    // 「aa bb cc」8 格进 5 格行：首行装 "aa bb" 恰好 5 格，下一字符
    // （空格）超 → 断在**最后一个**空格（下标 2）之后，空格留上行尾；
    // 余下 "bb cc" 恰好 5 格独占一行（贪心不回吐已在行的字）
    assert_eq!(wrap_strs("aa bb cc", 5), vec!["aa ", "bb cc"]);
    // 断点取**最后**一个空格，不是第一个
    assert_eq!(wrap_strs("a b c d", 5), vec!["a b ", "c d"]);
}

#[test]
fn spec_grid_wrap_零宽不占格() {
    // ZWJ 混在行里：不占格、不触发断行，跟随当前行
    assert_eq!(wrap_strs("ab\u{200d}cd", 4), vec!["ab\u{200d}cd"]);
    // 控制符同理（NUL 0 格）
    assert_eq!(wrap_strs("ab\0cd", 4), vec!["ab\0cd"]);
}

#[test]
fn spec_grid_wrap_空串与边界() {
    assert!(grid_wrap("", 4).is_empty(), "空串无行");
    assert_eq!(wrap_strs("中。", 4), vec!["中。"], "两全角恰好满格");
    // 区间覆盖全文不丢字：拼接回去 == 原文
    let t = "hello 世界 foo bar";
    let chars: Vec<char> = t.chars().collect();
    let joined: String = grid_wrap(t, 6)
        .into_iter()
        .flat_map(|(a, b)| chars[a..b].iter().copied())
        .collect();
    assert_eq!(joined, t, "折行不许吞字/多字");
}

// ── 字段值折行（BAR-206 打回重做，2026-10-01 用户裁定：省略号 = 信息
// 丢失，「那些信息都是必要的」——旧 elide_middle 中段省略件删除，长值
// 一律格折行往下长，行高随内容长，永不删字）──────────────────────────
//
// 契约：值列内宽 = 行内宽 − 标签宽 − 1 格间隔（保底 1 格）；折行全量
// 可见——各行拼接逐字等于原文，不出现「…」；行数随内容长（退化长值
// 也不许回退省略）。
//
// 变异抽检预期（cp 备份改坏看红、备份恢复）：
// - 值列不扣标签宽（label_px 漏减）→ 「值列扣标签与间隔」红；
// - 保底 1 格删掉（saturating 后不 max）→ 「病态窄列保底一格」红；
// - 折行掺省略（任一行塞「…」/丢字）→ 「全量可见逐字复算」红。

/// 同预算复算折行拼接（判卷口径：逐字等于原文、无省略号）
fn rejoin_wrapped(text: &str, cells: u32) -> String {
    let chars: Vec<char> = text.chars().collect();
    grid_wrap(text, cells)
        .into_iter()
        .flat_map(|(a, b)| chars[a..b].iter().copied())
        .collect()
}

#[test]
fn spec_field_value_bar206_值列扣标签与间隔() {
    // 行内宽 180px、格宽 10px、标签「磁盘」= 4 格：值列 = 180 − (4+1)×10
    assert_eq!(field_value_col_px(180, 10, "磁盘"), 130);
    // 标签宽则列窄（同尺减法，不许各写一份）：「QUIC 62633」= 10 半角格
    assert_eq!(field_value_col_px(180, 10, "QUIC 62633"), 70);
    // 行内宽装得下标签+间隔+余量时按余量出
    assert_eq!(field_value_col_px(120, 10, "目标"), 70);
}

#[test]
fn spec_field_value_bar206_病态窄列保底一格() {
    // 行内宽 < 标签宽 + 间隔：值列保底 1 格——病态窄也不吞字（往下长）
    assert_eq!(field_value_col_px(30, 10, "磁盘"), 10);
    assert_eq!(field_value_col_px(0, 10, "磁盘"), 10);
    // cell_w 病态 0 也不除零、不吞字
    let n = field_value_lines("abc", 0, 0, "磁盘");
    assert!(n >= 1, "保底有行：{n}");
}

#[test]
fn spec_field_value_bar206_装得下一行() {
    // 短值 = 1 行（旧恒定几何不受影响）
    assert_eq!(field_value_lines("45%", 180, 10, "磁盘"), 1);
    assert_eq!(field_value_lines("", 180, 10, "磁盘"), 1);
    // 恰好满值列 = 1 行（13 格值列装 13 格串）
    assert_eq!(field_value_lines("105.5G/235.9G", 180, 10, "磁盘"), 1);
}

#[test]
fn spec_field_value_bar206_全量可见逐字复算() {
    // 判卷口径 §五.1：显示文本逐字等于数据源，不出现「…」
    let cases = [
        "root@some-long-host.example.com:22",
        "105.5G/235.9G 45%",
        "https://example.com/some/very/long/path?query=1&other=2",
        "中文测试长字符串超宽很多很多",
    ];
    for (inner, cell_w, label) in [(180u32, 10u32, "磁盘"), (90, 10, "目标"), (60, 10, "在线")]
    {
        let col = field_value_col_px(inner, cell_w, label);
        let cells = (col / cell_w).max(1);
        for s in cases {
            let n = field_value_lines(s, inner, cell_w, label);
            let joined = rejoin_wrapped(s, cells);
            assert_eq!(joined, s, "折行吞字/多字（{s:?} @ {inner}px）");
            assert!(!joined.contains('…'), "不许出现省略号（{s:?}）");
            assert_eq!(
                grid_wrap(s, cells).len() as u32,
                n,
                "行数账与折行引擎同源（{s:?} @ {inner}px）"
            );
        }
    }
}

#[test]
fn spec_field_value_bar206_真机磁盘场景() {
    // 用户报障原话场景：磁盘值「数字过长被遮住」——行内宽 18 格量级、
    // 标签「磁盘」4 格：值列 = 18 − 4 − 1 = 13 格，17 格值折 2 行全显
    let inner_px = 18 * 10; // 真机 1260 宽 kfm-zoom 1.2778 左区字段行量级
    let n = field_value_lines("105.5G/235.9G 45%", inner_px, 10, "磁盘");
    assert_eq!(n, 2, "长值折两行往下长（不省略、不压柱轨——行高随长）");
    // 十位磁盘数退化档：也不许回退省略
    let big = "1024.5G/8192.0G 100%";
    let n2 = field_value_lines(big, inner_px, 10, "磁盘");
    assert!(n2 >= 2, "退化长值继续折行：{n2}");
    assert_eq!(rejoin_wrapped(big, 13), big, "退化档同样全量可见");
}
