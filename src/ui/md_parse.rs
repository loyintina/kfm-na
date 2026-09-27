//! ui/md_parse.rs — md 解析核心层（BAR-169 md 渲染器一期，2026-09-27
//! 研究线工单）：**纯逻辑零平台依赖**，六样子集手写解析（零新依赖红线
//! ——不许引 pulldown 类 crate）。
//!
//! 子集（块型语义对齐 demo_page::BlockKind）：ATX 标题 H1-H6 / 粗体 ** /
//! 行内码 ` / 代码围栏 ``` / 引用 > / 列表 - / 分隔线 ---。
//!
//! **渲染目标藏 trait**：解析产出走 MdSink 事件口——查看器排版
//! （md_layout）与 demo 页（二期）是两个消费者，本册不为任何一边写死。
//!
//! 块律（取舍定案，考题钉死）：
//! - 段落 = 连续非空行合成一块，**每个源行独立折行**（不跨行拼接——
//!   CJK 信件换行即语义换行）；
//! - 围栏 ``` 开闭成块，**围栏内一切字面**（星号/井号/反引号全不解析）；
//!   未闭合围栏 = 余下全文归代码块（信件容错，不炸）；
//! - 引用 `> ` / `>` 前缀连续成块，行内段照解析；列表 `- ` 前缀连续
//!   成块，一项一行；分隔线 = 整行 ≥3 个 `-`（先判分隔线再判列表——
//!   `---` 不是列表项）；
//! - ATX：`#`×1-6 + 空格/行尾 = 标题；`#`×7+ 或 `#`后无空格 = 正文
//!   （CommonMark 同律）；标题文字走行内解析；
//! - 行内：`**粗体**` 与 `` `码` `` 左到右扫描，反引号优先；未闭合
//!   标记 = 字面（不吞文）；**标记内不再嵌套解析**（粗体里的 ` 是字面，
//!   码里的 ** 是字面）；空标记对（`****`/```` `` ````）= 字面。

use crate::ui::demo_page::SegStyle;

/// 行内段（样式, 文本）：样式枚举复用 demo_page（语义单源）
pub type Span = (SegStyle, String);

/// 渲染目标抽象（解析事件口）：每块一次回调，行内段已解析
pub trait MdSink {
    /// ATX 标题（level 1-6）
    fn heading(&mut self, level: u8, spans: Vec<Span>);
    /// 正文段（连续非空行一块，一源行一 Vec<Span>）
    fn paragraph(&mut self, lines: Vec<Vec<Span>>);
    /// 代码围栏（字面行，零解析）
    fn code_block(&mut self, lines: Vec<String>);
    /// 引用块（一源行一 Vec<Span>，行内已解析）
    fn quote(&mut self, lines: Vec<Vec<Span>>);
    /// 列表块（一项一 Vec<Span>，行内已解析）
    fn list(&mut self, items: Vec<Vec<Span>>);
    /// 分隔线
    fn hr(&mut self);
}

/// 行内解析（`**粗体**` / `` `码` ``，余者 Normal）：左到右扫描，
/// 反引号优先于星号；未闭合 = 字面；标记内容不再嵌套解析。
/// 相邻同样式段不合并（语义不变，涂装段排零影响）。
pub fn parse_inline(s: &str) -> Vec<Span> {
    let b = s.as_bytes();
    let mut out: Vec<Span> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    let flush = |cur: &mut String, out: &mut Vec<Span>| {
        if !cur.is_empty() {
            out.push((SegStyle::Normal, std::mem::take(cur)));
        }
    };
    while i < b.len() {
        if b[i] == b'`' {
            // 行内码：找下一个 `（同字节串搜，UTF-8 安全——` 是 ASCII，
            // 多字节序列不含其字节值）
            if let Some(rel) = s[i + 1..].find('`') {
                flush(&mut cur, &mut out);
                out.push((SegStyle::Code, s[i + 1..i + 1 + rel].to_string()));
                i += rel + 2;
            } else {
                cur.push('`'); // 未闭合 = 字面
                i += 1;
            }
        } else if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'*' {
            // 粗体：找下一个 **；空对 **** = 字面（rel==0 不入）
            if let Some(rel) = s[i + 2..].find("**")
                && rel > 0
            {
                flush(&mut cur, &mut out);
                out.push((SegStyle::Bold, s[i + 2..i + 2 + rel].to_string()));
                i += rel + 4;
            } else {
                cur.push('*');
                cur.push('*');
                i += 2;
            }
        } else {
            // 按 char 推进（UTF-8 边界安全）
            let c = s[i..].chars().next().expect("i 在边界上");
            cur.push(c);
            i += c.len_utf8();
        }
    }
    flush(&mut cur, &mut out);
    if out.is_empty() {
        out.push((SegStyle::Normal, String::new())); // 空行 = 空段占位不塌
    }
    out
}

/// 整行 ≥3 个 `-`（允许空格夹杂）= 分隔线
fn is_hr(line: &str) -> bool {
    let dashes = line.chars().filter(|&c| c == '-').count();
    dashes >= 3 && line.chars().all(|c| c == '-' || c == ' ')
}

/// 围栏行判据： trimmed 以 ``` 开头（信息串如 ```rust 照吞）
fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// ATX 级别：`#`×1-6 + 空格/行尾 = Some(level)；否则 None
fn atx_level(line: &str) -> Option<u8> {
    let t = line.trim_start();
    let n = t.bytes().take_while(|&b| b == b'#').count();
    if n == 0 || n > 6 {
        return None;
    }
    match t[n..].chars().next() {
        None | Some(' ') => Some(n as u8),
        _ => None,
    }
}

/// md 全文 → 事件流（sink 回调序 = 文档序）
pub fn parse_md(text: &str, sink: &mut impl MdSink) {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        // 代码围栏（最高优先：围栏内一切字面）
        if is_fence(line) {
            let mut body: Vec<String> = Vec::new();
            i += 1;
            while i < lines.len() && !is_fence(lines[i]) {
                body.push(lines[i].to_string());
                i += 1;
            }
            if i < lines.len() {
                i += 1; // 吃闭合围栏；未闭合 = 余下全文已归代码块
            }
            sink.code_block(body);
            continue;
        }
        if is_hr(trimmed) {
            sink.hr();
            i += 1;
            continue;
        }
        if let Some(level) = atx_level(line) {
            let t = line.trim_start();
            let text = t[level as usize..].trim();
            sink.heading(level, parse_inline(text));
            i += 1;
            continue;
        }
        // 引用块：连续 `>` 前缀行
        if trimmed.starts_with('>') {
            let mut qlines: Vec<Vec<Span>> = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim_start();
                if !t.starts_with('>') || t.trim().is_empty() {
                    break;
                }
                let body = t[1..].strip_prefix(' ').unwrap_or(&t[1..]);
                qlines.push(parse_inline(body));
                i += 1;
            }
            sink.quote(qlines);
            continue;
        }
        // 列表块：连续 `- ` 前缀行（`-` 孤行不是列表项）
        if trimmed.starts_with("- ") {
            let mut items: Vec<Vec<Span>> = Vec::new();
            while i < lines.len() {
                let t = lines[i].trim_start();
                match t.strip_prefix("- ") {
                    Some(body) => {
                        items.push(parse_inline(body));
                        i += 1;
                    }
                    None => break,
                }
            }
            sink.list(items);
            continue;
        }
        // 正文段：连续「非空且不是任何块起手」行
        let mut plines: Vec<Vec<Span>> = Vec::new();
        while i < lines.len() {
            let t = lines[i];
            let tt = t.trim();
            if tt.is_empty()
                || is_fence(t)
                || is_hr(tt)
                || atx_level(t).is_some()
                || tt.starts_with('>')
                || tt.starts_with("- ")
            {
                break;
            }
            plines.push(parse_inline(t));
            i += 1;
        }
        sink.paragraph(plines);
    }
}
