//! 信封（文首引用块）解析 + 「致」字段切项（移植 JS parseHeader/parseToField）。

use crate::name::ToItem;

/// 文首第一个连续 `>` 引用块的行区间（起, 止，含）；无引用块返回 None。
pub fn first_quote_block(text: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|l| l.starts_with('>'))?;
    let mut end = start;
    while end + 1 < lines.len() && lines[end + 1].starts_with('>') {
        end += 1;
    }
    Some((start, end))
}

/// 文首引用块最后一行的行号（reticket 插令牌用）
pub fn first_quote_block_end(text: &str) -> Option<usize> {
    first_quote_block(text).map(|(_, e)| e)
}

/// 文首第一个连续 `>` 引用块内的指定字段；同名字段取最后一次；兼容全角冒号。
/// 返回出现序的 (字段, 值) 列表（值已剥首尾空白；空值不匹配）。
pub fn parse_header(text: &str, names: &[&str]) -> Vec<(String, String)> {
    let mut out = vec![];
    let Some((start, end)) = first_quote_block(text) else {
        return out;
    };
    let lines: Vec<&str> = text.split('\n').collect();
    for line in &lines[start..=end] {
        // ^>\s*(name)\s*[:：]\s*(.+?)\s*$
        let Some(stripped) = line.strip_prefix('>') else {
            continue;
        };
        let rest = stripped.trim_start();
        for name in names {
            let Some(after) = rest.strip_prefix(name) else {
                continue;
            };
            let after = after.trim_start();
            let Some(after) = after.strip_prefix([':', '：']) else {
                continue;
            };
            let value = after.trim();
            if !value.is_empty() {
                out.push((name.to_string(), value.to_string()));
            }
            break;
        }
    }
    out
}

/// 取字段最后一次出现的值
pub fn header_get<'a>(h: &'a [(String, String)], key: &str) -> Option<&'a str> {
    h.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

fn drop_last_chars(s: &str, n: usize) -> &str {
    let keep = char_count(s).saturating_sub(n);
    match s.char_indices().nth(keep) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn last_chars(s: &str, n: usize) -> &str {
    let skip = char_count(s).saturating_sub(n);
    match s.char_indices().nth(skip) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

/// 「致」字段按顿号/逗号/「及」切项；纯两字继承前项职能；禁「等」结尾（记 errs）。
/// 契约 §三：「、」分职能组、组内多名用「及」连接（`开发部闻灯及观澜`）；同职能必须合并。
/// 与文件名「致」段的切项口径同制（见 parse_v21_name）。
pub fn parse_to_field(value: &str, errs: &mut Vec<String>) -> Vec<ToItem> {
    let mut items = vec![];
    let mut prev_func: Option<String> = None;
    for raw in value
        .split(['、', '，', ',', '及'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if raw.ends_with('等') {
            errs.push(format!("信封「致」不得以「等」结尾（必须列全）：{raw}"));
        }
        if raw == "全体" {
            items.push(ToItem {
                func: None,
                name: "全体".into(),
                inherited: false,
            });
            continue;
        }
        let clean = raw.strip_suffix('等').unwrap_or(raw);
        if char_count(clean) == 2 {
            match &prev_func {
                None => {
                    errs.push(format!(
                        "收件人「{clean}」是纯两字项但无前项职能可继承（首项必须带职能）"
                    ));
                    continue;
                }
                Some(f) => {
                    items.push(ToItem {
                        func: Some(f.clone()),
                        name: clean.into(),
                        inherited: true,
                    });
                    continue;
                }
            }
        }
        if char_count(clean) < 3 {
            errs.push(format!(
                "收件人「{raw}」非法（须 <职能><名字>，如 研究部空谷）"
            ));
            continue;
        }
        let func = drop_last_chars(clean, 2).to_string();
        let name = last_chars(clean, 2).to_string();
        prev_func = Some(func.clone());
        items.push(ToItem {
            func: Some(func),
            name,
            inherited: false,
        });
    }
    items
}
