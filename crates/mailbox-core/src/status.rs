//! 状态词表 / 代际戳 / 日期格式（移植 JS 状态面判据）。

/// 从 README 文本解析 `合法状态词表（…）：…。`（唯一出处 = README 规则区）
pub fn parse_status_words(readme: &str) -> Vec<String> {
    // /合法状态词表（[^）]*）：([^。]+)。/
    let Some(i) = readme.find("合法状态词表（") else {
        return vec![];
    };
    let rest = &readme[i + "合法状态词表（".len()..];
    let Some(j) = rest.find('）') else {
        return vec![];
    };
    let rest = &rest[j + '）'.len_utf8()..];
    let Some(rest) = rest.strip_prefix('：') else {
        return vec![];
    };
    let Some(k) = rest.find('。') else {
        return vec![];
    };
    rest[..k]
        .split('/')
        .map(|seg| {
            // 剥 （…） 注记（/（[^）]*）/g）
            let mut out = String::new();
            let mut s = seg;
            while let Some(a) = s.find('（') {
                out.push_str(&s[..a]);
                match s[a..].find('）') {
                    Some(b) => s = &s[a + b + '）'.len_utf8()..],
                    None => {
                        s = "";
                        break;
                    }
                }
            }
            out.push_str(s);
            out.trim().to_string()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// 状态词前缀 regex：词长按降序拼 alternation（JS 同款构造，稳定排序）
pub fn build_status_re(words: &[String]) -> Option<regex::Regex> {
    if words.is_empty() {
        return None;
    }
    let mut sorted: Vec<&String> = words.iter().collect();
    sorted.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    let alts: Vec<&str> = sorted.iter().map(|s| s.as_str()).collect();
    regex::Regex::new(&format!(
        r"^[^\p{{L}}\p{{Han}}]*(?:\p{{Han}}{{1,4}})?(?:{})",
        alts.join("|")
    ))
    .ok()
}

/// 欠账判定：剥非字母/非汉字前缀后以「待」开头
pub fn is_debt(status: &str) -> bool {
    regex::Regex::new(r"^[^\p{L}\p{Han}]*待")
        .unwrap()
        .is_match(status)
}

/// `YYYY-MM-DD HH:MM ±HH:MM`
pub fn is_valid_v21_date(s: &str) -> bool {
    regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2} [+-]\d{2}:\d{2}$")
        .unwrap()
        .is_match(s)
}

/// 代际戳 `（YYYY-MM-DD HH:MM ±HH:MM \S+ 更新[:：]`
pub fn has_v21_stamp(s: &str) -> bool {
    regex::Regex::new(r"（\d{4}-\d{2}-\d{2} \d{2}:\d{2} [+-]\d{2}:\d{2} \S+ 更新[:：]")
        .unwrap()
        .is_match(s)
}

/// 信封「状态」剥前缀（非汉字非字母）后是否待*（待* 豁免代际戳）
pub fn status_is_pending(s: &str) -> bool {
    let re = regex::Regex::new(r"^[^\p{Han}A-Za-z]+").unwrap();
    re.replace(s, "").starts_with('待')
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 日期串 → epoch 毫秒（JS toMs：缺时刻按 00:00、缺时区按 +08:00；非法 NaN）
pub fn to_ms(s: &str) -> f64 {
    let re = regex::Regex::new(
        r"^(\d{4})-(\d{2})-(\d{2})(?:\s+(\d{2}):(\d{2}))?(?:\s+([+-])(\d{2}):(\d{2}))?$",
    )
    .unwrap();
    let Some(m) = re.captures(s.trim()) else {
        return f64::NAN;
    };
    let num = |i: usize| m.get(i).unwrap().as_str().parse::<i64>().unwrap();
    let (y, mo, d) = (num(1), num(2), num(3));
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) {
        return f64::NAN;
    }
    let (hh, mm) = if m.get(4).is_some() {
        (num(4), num(5))
    } else {
        (0, 0)
    };
    if hh > 23 || mm > 59 {
        return f64::NAN;
    }
    let off_min = if m.get(6).is_some() {
        let v = num(7) * 60 + num(8);
        if m.get(6).unwrap().as_str() == "-" {
            -v
        } else {
            v
        }
    } else {
        8 * 60
    };
    let days = days_from_civil(y, mo, d);
    ((days * 86400 + hh * 3600 + mm * 60 - off_min * 60) * 1000) as f64
}
