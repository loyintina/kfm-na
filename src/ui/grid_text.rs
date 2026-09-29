//! grid_text — 网格文字引擎（BAR-178，布局唯一源；契约 docs/active/网格文字.md）。
//!
//! 终端页之外的 UI 文字从此与终端同一把尺：全角 2 格、半角 1 格、
//! 控制符 0 格，格 = TermView.cell_w/cell_h（基准 CELL_W=18/CELL_H=36，
//! pinch 双指缩放改实例字段）。三件形制全部收在本模块：
//!
//! 1. **格宽分类** `char_cells` / `grid_text_cells`——终端全角两格口径的
//!    UI 侧唯一源（unicode-width，与 alacritty 网格同宗）。
//!    `tab_bar::text_cells`（BAR-191）与 `modal::wrap_text`（BAR-194）
//!    均已收编。
//!    （土判据真盲区订正（BAR-191 实证）：U+3000 全角标点段在 0x2E80
//!    **之上**，土判据照样判 2 格；真分野 = 谚文首字母 U+1100–115F、
//!    零宽/组合符（U+200B、U+0300 段）——引擎 2/0 格，土判据全判 1）
//! 2. **格折行** `grid_wrap`——贪心按格断行，断点优先最后一个 ASCII
//!    空格之后，没有就硬断（ft_wrap_split 同款贪心语义）。
//! 3. **格落笔**——在 TermView 侧（termview.rs 尾部
//!    `grid_fit` / `measure_items_grid` / `draw_grid_text_left`，
//!    私有字段只有本模块够得着，胶水只能放那边）。
//!
//! 字号两来源条款：运行期字号 = grid_fit 读 TermView 实例格（pinch
//! 联动）；基准常量 CELL_W/CELL_H 不动。**文件树字号自此吃格子变量**
//! （旧 `FT_TEXT_PX=44` 常量随 BAR-178 删除——44 的来历留档：打回改约
//! 症①，原版实测字高 32~33 物理 px 的 ×1.3 放大档，2026-09-27 用户终验）；
//! 行高同理走半格网标定（ROW_H=90=2.5 格 / ROW_H_WRAP=108=3 格，
//! 见 filetree.rs 常量注）。
//!
//! 本模块全部纯函数零 IO（A 档），考卷 tests/grid_text_spec.rs。

use unicode_width::UnicodeWidthChar;

/// 字符格宽（终端全角两格口径的 UI 侧唯一源）：控制符/零宽 = 0 格，
/// 半角 = 1 格，全角（CJK/全角标点/emoji）= 2 格。
/// 判据来源 = unicode-width（alacritty 终端网格同一个 crate）——
/// 终端画几格，UI 就量几格，两份判据不许再各写（0x2E80 土判据的
/// 真盲区（BAR-191 实证订正）：谚文首字母 U+1100–115F 与零宽/组合符
/// ——U+200B、U+0300 段——土判据全误判 1 格；全角标点 U+3000 段在
/// 0x2E80 之上，土判据碰巧不瞎）
pub fn char_cells(c: char) -> u32 {
    UnicodeWidthChar::width(c).unwrap_or(0) as u32
}

/// 字符串总格数（逐字 char_cells 求和）
pub fn grid_text_cells(s: &str) -> u32 {
    s.chars().map(char_cells).sum()
}

/// 按格折行（贪心，ft_wrap_split 同款语义）：返回每行的 **char 下标**
/// 区间 `[start, end)`。
///
/// - 累计格数超 `max_cells` 即断；断点优先当前行内**最后一个 ASCII
///   空格之后**（空格留在行尾），没有就硬断；
/// - 行首第一个字符恒收（哪怕它自己就超宽——病态窄行不死循环，
///   与 ft_wrap_split「超宽单字不吞字」同律）；
/// - 零宽字符不占格、不触发断行，跟随当前行；
/// - 空串返回空表。
pub fn grid_wrap(text: &str, max_cells: u32) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut start = 0usize; // 当前行首 char 下标
    let mut acc = 0u32; // 当前行已占格数
    let mut last_space: Option<usize> = None; // 当前行内最后一个 ASCII 空格下标
    for (i, c) in text.chars().enumerate() {
        let w = char_cells(c);
        if i > start && acc + w > max_cells {
            // 超宽断行：有空格吃空格（空格归上行尾），没有硬断在当前字之前
            let (end, next) = match last_space {
                Some(s) if s >= start => (s + 1, s + 1),
                _ => (i, i),
            };
            lines.push((start, end));
            start = next;
            acc = 0;
            last_space = None;
            // 断点之后到当前字之前的字符重新入账（硬断时区间为空，零循环）
            for (j, c2) in text.chars().enumerate().take(i).skip(start) {
                acc += char_cells(c2);
                if c2 == ' ' {
                    last_space = Some(j);
                }
            }
        }
        acc += w;
        if c == ' ' {
            last_space = Some(i);
        }
    }
    if start < text.chars().count() {
        lines.push((start, text.chars().count()));
    }
    lines
}
