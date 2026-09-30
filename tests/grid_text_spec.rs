//! grid_text_spec.rs — 网格文字引擎考卷（BAR-178，A 档）。
//! 判卷对象：src/ui/grid_text.rs（格宽分类 / 总格数 / 格折行，全纯函数）。
//!
//! 变异抽检预期（先改坏、看对应考题变红，cp 备份恢复——禁用 git checkout）：
//! - `char_cells` 全返 1 → 「真值表」与「混排总格数」红；
//! - `grid_wrap` 硬断改 +1 格（`acc + w > max_cells` 改 `>=` 等价病）→
//!   「恰好满格不断」红；
//! - `grid_wrap` 不吃空格断点（断点分支删空格优先）→ 「多空格优选断点」红。

use kfm_na::ui::grid_text::{char_cells, grid_text_cells, grid_wrap};

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
