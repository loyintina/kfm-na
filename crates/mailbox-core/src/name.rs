//! 纪元检测 / 文件名文法解析（契约 §二，移植 JS parseV21Name）。

/// 契约 §二 9 词表（2026-09-28 二次更新：落地移出、新增勘误）
pub const V21_TYPES: [&str; 9] = [
    "提案", "审阅", "裁决", "回信", "回执", "通报", "报告", "日报", "勘误",
];

/// 连接字（号/致/复/的/及/等）——名字、事由禁用
pub const CONNECT_CHARS: [char; 6] = ['号', '致', '复', '的', '及', '等'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToItem {
    pub func: Option<String>,
    pub name: String,
    pub inherited: bool,
}

#[derive(Debug, Default, Clone)]
pub struct V21Parsed {
    pub no: Option<String>,
    pub sorting: Option<String>,
    pub from_name: Option<String>,
    pub to: Option<String>,
    pub to_items: Option<Vec<ToItem>>,
    pub reply: Option<String>,
    pub subject: Option<String>,
    pub type_word: Option<String>,
    pub errs: Vec<String>,
}

fn han_re() -> regex::Regex {
    regex::Regex::new(r"^\p{Han}+$").unwrap()
}

pub(crate) fn is_han_str(s: &str) -> bool {
    !s.is_empty() && han_re().is_match(s)
}

pub(crate) fn has_connect_char(s: &str) -> bool {
    s.chars().any(|c| CONNECT_CHARS.contains(&c))
}

/// 解析编号段前缀 `[A-Z]{0,4}\d{4}`，返回 (分拣码, 4位号, 消耗字节数)
fn split_no_prefix(f: &str) -> Option<(String, String, usize)> {
    let b = f.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_uppercase() {
        i += 1;
    }
    let uppers = i;
    let mut d = 0;
    while i < b.len() && b[i].is_ascii_digit() && d < 4 {
        i += 1;
        d += 1;
    }
    if uppers > 4 || d < 4 {
        return None;
    }
    Some((f[..uppers].to_string(), f[uppers..i].to_string(), i))
}

/// 纪元检测：`^[A-Z]{0,4}\d{4}号`
pub fn is_v21_name(f: &str) -> bool {
    match split_no_prefix(f) {
        Some((_, _, used)) => f[used..].starts_with('号'),
        None => false,
    }
}

/// 取全编号（含分拣码），如 NA0024
pub fn v21_no_of(f: &str) -> Option<String> {
    let (sorting, no, used) = split_no_prefix(f)?;
    if f[used..].starts_with('号') {
        Some(format!("{sorting}{no}"))
    } else {
        None
    }
}

/// 旧 ASCII 形态编号：`NNNN-` 前缀
pub fn legacy_no_of(f: &str) -> Option<String> {
    let b = f.as_bytes();
    if b.len() >= 5 && b[..4].iter().all(|c| c.is_ascii_digit()) && b[4] == b'-' {
        Some(f[..4].to_string())
    } else {
        None
    }
}

fn take_chars(s: &str, n: usize) -> (String, usize) {
    let mut out = String::new();
    let mut bytes = 0;
    for c in s.chars().take(n) {
        out.push(c);
        bytes += c.len_utf8();
    }
    (out, bytes)
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
    let total = char_count(s);
    let skip = total.saturating_sub(n);
    match s.char_indices().nth(skip) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

/// 文件名文法解析（契约 §二「解析算法」，逐函数移植 JS 版）
pub fn parse_v21_name(file: &str) -> V21Parsed {
    let mut p = V21Parsed::default();
    let base = file.strip_suffix(".md").unwrap_or(file);
    let Some(i_hao) = base.find('号') else {
        p.errs.push(format!("文件名缺连接字「号」：{file}"));
        return p;
    };
    let no_seg = &base[..i_hao];
    let Some((sorting, no, _)) = split_no_prefix(no_seg).filter(|(s, n, u)| {
        // 编号段须全段合法：^([A-Z]{0,4})(\d{4})$
        *u == no_seg.len() && s.len() <= 4 && n.len() == 4
    }) else {
        p.errs.push(format!(
            "编号段非法：{no_seg}（须 [分拣码]NNNN，如 0025／NA0024）"
        ));
        return p;
    };
    p.sorting = Some(sorting.clone());
    p.no = Some(no.clone());

    let body = &base[i_hao + '号'.len_utf8()..];
    let (from_name, from_bytes) = take_chars(body, 2);
    p.from_name = Some(from_name.clone());
    if char_count(&from_name) != 2 || !is_han_str(&from_name) {
        let shown = if from_name.is_empty() {
            "（空）"
        } else {
            &from_name
        };
        p.errs.push(format!("发信人须恰好两个汉字：{shown}"));
    } else if has_connect_char(&from_name) {
        p.errs.push(format!(
            "发信人名「{from_name}」含连接字（号/致/复/的/及/等）"
        ));
    }
    let s = &body[from_bytes..];
    // 名字／职能／事由都禁含「的」→ 末尾那个「的」就是类型词分界（契约 §二步骤 4）
    let Some(i_de) = s.rfind('的') else {
        p.errs
            .push(format!("缺连接字「的」（类型词前必须写「的」）：{file}"));
        return p;
    };
    let type_word = s[i_de + '的'.len_utf8()..].to_string();
    p.type_word = Some(type_word.clone());
    let mut head = &s[..i_de];

    // 致 段在「复」「关于」处收尾（两者都是它之后的可选段）
    let earliest = |str_: &str, tokens: &[&str]| -> Option<usize> {
        tokens.iter().filter_map(|t| str_.find(t)).min()
    };
    if let Some(rest) = head.strip_prefix('致') {
        match earliest(rest, &["复", "关于"]) {
            Some(end) => {
                p.to = Some(rest[..end].to_string());
                head = &rest[end..];
            }
            None => {
                p.to = Some(rest.to_string());
                head = "";
            }
        }
    }
    if let Some(rest) = head.strip_prefix('复') {
        match rest.find("关于") {
            Some(end) => {
                p.reply = Some(rest[..end].to_string());
                head = &rest[end..];
            }
            None => {
                p.reply = Some(rest.to_string());
                head = "";
            }
        }
    }
    if let Some(rest) = head.strip_prefix("关于") {
        // 可选段：短通报／内部信可省，缺失不算违规（契约 §二护栏④）
        p.subject = Some(rest.to_string());
        head = "";
    }
    if !head.is_empty() {
        let (shown, _) = take_chars(head, 12);
        p.errs.push(format!(
            "主体段须为「[致…][复…][关于…]的<类型词>」：{file}（余「{shown}」）"
        ));
        return p;
    }
    if !V21_TYPES.contains(&type_word.as_str()) {
        p.errs.push(format!(
            "类型词出表：「{type_word}」∉ {}",
            V21_TYPES.join("/")
        ));
    }
    if let Some(subject) = &p.subject {
        let n = char_count(subject);
        if !(2..=12).contains(&n) || !is_han_str(subject) {
            p.errs.push(format!(
                "「关于」事由须为 2–12 个汉字（建议 4–8）：「{subject}」"
            ));
        } else if has_connect_char(subject) || subject.contains("关于") {
            p.errs.push(format!(
                "「关于」事由含连接字（号/致/复/的/及/等/关于）：「{subject}」"
            ));
        }
    }
    if let Some(to) = &p.to {
        let t = to.strip_suffix('等').unwrap_or(to);
        if t.is_empty() {
            p.errs.push("「致」段为空".to_string());
        } else {
            let mut items = vec![];
            let mut prev_func: Option<String> = None;
            for raw in t.split('及') {
                if raw.is_empty() {
                    p.errs
                        .push("「致」段含空项（多余/重复的「及」）".to_string());
                    continue;
                }
                if raw == "全体" {
                    items.push(ToItem {
                        func: None,
                        name: "全体".into(),
                        inherited: false,
                    });
                    continue;
                }
                if char_count(raw) == 2 {
                    match &prev_func {
                        None => {
                            p.errs.push(format!(
                                "收件人「{raw}」是纯两字项但无前项职能可继承（首项必须带职能）"
                            ));
                            continue;
                        }
                        Some(f) => {
                            items.push(ToItem {
                                func: Some(f.clone()),
                                name: raw.into(),
                                inherited: true,
                            });
                            continue;
                        }
                    }
                }
                let func = drop_last_chars(raw, 2).to_string();
                let name = last_chars(raw, 2).to_string();
                if has_connect_char(&name) {
                    p.errs.push(format!("收件人名「{name}」含连接字"));
                }
                prev_func = Some(func.clone());
                items.push(ToItem {
                    func: Some(func),
                    name,
                    inherited: false,
                });
            }
            p.to_items = Some(items);
        }
    }
    p
}
