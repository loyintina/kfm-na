//! grid_text_spec.rs — 网格文字引擎考卷（BAR-178，A 档）。
//! 判卷对象：src/ui/grid_text.rs（格宽分类 / 总格数 / 格折行，全纯函数）。
//!
//! 变异抽检预期（先改坏、看对应考题变红，cp 备份恢复——禁用 git checkout）：
//! - `char_cells` 全返 1 → 「真值表」与「混排总格数」红；
//! - `grid_wrap` 硬断改 +1 格（`acc + w > max_cells` 改 `>=` 等价病）→
//!   「恰好满格不断」红；
//! - `grid_wrap` 不吃空格断点（断点分支删空格优先）→ 「多空格优选断点」红。

use kfm_na::ui::grid_text::{char_cells, elide_middle, grid_text_cells, grid_wrap};

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

// ── 中段省略（BAR-206 解析页长值截断/纵溢治理）──────────────────────
//
// 契约：装得下原样；装不下 = 头尾双锚 + 中间一个「…」（1 格），预算
// 对半分（头 ⌈半⌉ 尾 ⌊半⌋），总格数 ≤ max_cells 恒成立；字符原子切，
// 全角字不劈半。字段行盒高恒定 2 格装不下两行 meta（折行 = 第二行纵溢
// 压柱轨），故值恒定单行——单行钉用引擎自己的格折行判（格步进不吃
// meta 缩字，格预算 ≡ px 内宽 ÷ 格宽，与 draw_field_lines_grid 同尺）。
//
// 变异抽检预期（cp 备份改坏看红、备份恢复）：
// - 预算不减 1 格（不给「…」留位）→ 「总长不超预算」红；
// - 尾预算砍 0（只保头）→ 「头尾双锚」红；
// - 装得下判据 `<=` 改 `<` → 「恰好满格原样」红。

#[test]
fn spec_elide_middle_bar206_装得下原样() {
    assert_eq!(elide_middle("abc", 3), "abc", "恰好满格 = 原样（> 才省）");
    assert_eq!(elide_middle("abc", 10), "abc");
    assert_eq!(elide_middle("中文", 4), "中文", "全角恰好满格 = 原样");
    assert_eq!(elide_middle("", 0), "", "空串恒原样");
    assert_eq!(elide_middle("", 5), "");
}

#[test]
fn spec_elide_middle_bar206_头尾双锚() {
    // 10 格进 7 格：预算 6（「…」占 1），头 ⌈3⌉ 尾 ⌊3⌋
    assert_eq!(elide_middle("abcdefghij", 7), "abc…hij");
    // 头 = 原文前缀、尾 = 原文后缀，中间只有一个省略号
    let r = elide_middle("root@some-long-host.example.com:22", 15);
    assert!(r.starts_with("root@"), "头锚保住：{r}");
    assert!(r.ends_with("com:22"), "尾锚保住：{r}");
    assert_eq!(r.matches('…').count(), 1, "恰好一个省略号：{r}");
}

#[test]
fn spec_elide_middle_bar206_总长不超预算() {
    for (s, n) in [
        ("abcdefghij", 7),
        ("root@some-long-host.example.com:22", 15),
        ("105.5G/235.9G 45%", 13),
        ("中文测试长字符串超宽", 9),
        ("abc", 2),
    ] {
        let r = elide_middle(s, n);
        assert!(
            grid_text_cells(&r) <= n,
            "elide_middle({s:?}, {n}) = {r:?} 超预算（{} 格）",
            grid_text_cells(&r)
        );
    }
}

#[test]
fn spec_elide_middle_bar206_全角不劈半() {
    // 「中文测试abc」= 2+2+2+2+1+1+1 = 11 格进 7 格：预算 6，头 3 格
    // 只能装「中」（「文」2 格劈不下整字让），尾 3 格 = 「abc」
    assert_eq!(elide_middle("中文测试abc", 7), "中…abc");
    // 尾侧同样：尾预算 2 格装一个全角
    assert_eq!(elide_middle("abcde中文", 5), "ab…文");
}

#[test]
fn spec_elide_middle_bar206_病态窄预算() {
    assert_eq!(
        elide_middle("abcdef", 0),
        "",
        "0 格 = 空串（放…也是占格撒谎）"
    );
    assert_eq!(
        elide_middle("abcdef", 1),
        "…",
        "1 格 = 只放省略号报「有字被截」"
    );
    assert_eq!(elide_middle("abcdef", 2), "a…", "2 格 = 头 1 + …，尾预算 0");
}

#[test]
fn spec_elide_middle_bar206_省略后恒定单行() {
    // 纵溢病灶的契约钉：字段值经省略后按格折行恒一行（预算 ≥1 时）——
    // draw_field_lines_grid 的 px 折行与格折行同尺（字宽 = 格数×格宽）
    let disk = "105.5G/235.9G 45%";
    for n in [1u32, 2, 7, 13, 15, 40] {
        let r = elide_middle(disk, n);
        assert_eq!(
            grid_wrap(&r, n).len(),
            usize::from(!r.is_empty()),
            "elide_middle({disk:?}, {n}) = {r:?} 折行不是一行"
        );
    }
}

#[test]
fn spec_elide_middle_bar206_真机磁盘场景() {
    // 用户报障原话场景：磁盘值「数字过长被遮住」——预算 = 行内宽折格
    // − 标签「磁盘」4 格 − 1 格间隔；值省略后单行落盒不压柱轨
    let inner_cells = 18u32; // 真机 1260 宽 kfm-zoom 1.2778 左区字段行量级
    let budget = inner_cells - grid_text_cells("磁盘") - 1;
    let v = elide_middle("105.5G/235.9G 45%", budget);
    assert!(grid_text_cells(&v) <= budget, "{v}");
    assert_eq!(grid_wrap(&v, budget).len(), 1, "{v} 必须单行");
}
