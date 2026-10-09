//! floor — 楼层段纯函数核（new-letter.mjs --floor 路径的 Rust 移植）。
//!
//! 纪律：本模块零 IO——所有函数收输入返回结果/诊断，不碰文件系统、不取时间、不提交。
//! 行为基准 = new-letter.mjs 第 676–1526 行＋契约 §十二，格式面逐字节兼容。
//! 生效时刻 2026-10-03 09:00 +08:00：楼头时间 ≥ 该时刻 ⇒ 两段制硬闸；早于 ⇒ 豁免。

use std::collections::HashSet;

// ---- 常量（照 JS 逐字） ----
pub const FLOOR_ARROW: &str = "\u{2192}"; // →
pub const FLOOR_TIME_SEP: &str = " \u{00B7} "; // · (U+00B7 + 两侧空格)
pub const FLOOR_SEP: &str = "---";
pub const OWNER_MARK: &str = "(楼主)";
pub const FLOOR_BODY_MAX: usize = 2000;
/// 两段制生效时刻（epoch ms）：2026-10-03 09:00 +08:00 = 01:00 UTC = 1790989200000
pub const FLOOR_TWO_SECTION_SINCE_MS: i64 = 1_790_989_200_000;

// ---- 楼层头：新形单行四段 ----
/// `> N楼：作者[(楼主)]→被回复者 · 时间`
#[derive(Debug, Clone, PartialEq)]
pub struct FloorHead {
    pub no: u32,
    pub author: String,
    pub is_owner: bool,
    pub to_name: String,
    pub to_is_owner: bool,
    pub to_floor: Option<u32>,
    pub time: String,
}

// ---- 一封已解析的楼 ----
#[derive(Debug, Clone)]
pub struct Floor {
    pub head: FloorHead,
    pub body: String,
    pub start_line: usize, // 0-indexed
    pub withdrawn: bool,
    pub has_ask: bool,
}

// ---- 校验结果 ----
#[derive(Debug, Default)]
pub struct FloorCheck {
    pub errors: Vec<String>,
    pub notes: Vec<String>,
}

// ---- 正则等价：手写匹配（避免 regex crate 依赖） ----

/// 段界（新形）：`> N楼：` 起始的行——返回 (楼号, 行内容去掉 `> N楼：` 后的余段)
pub fn match_new_start(line: &str) -> Option<(u32, &str)> {
    let t = line.trim_start();
    if !t.starts_with('>') {
        return None;
    }
    let rest = t[1..].trim_start();
    // 找 `楼` 后跟 `：` 或 `:`
    let idx_lou = rest.find('楼')?;
    let num_str = rest[..idx_lou].trim();
    let after = rest[idx_lou + "楼".len()..].trim_start();
    let after = after
        .strip_prefix('：')
        .or_else(|| after.strip_prefix(':'))?;
    let no: u32 = num_str.parse().ok()?;
    Some((no, after))
}

/// 段界（旧形）：`> 楼: N`（整行，行尾无余文）
pub fn match_old_start(line: &str) -> Option<u32> {
    let t = line.trim();
    if !t.starts_with('>') {
        return None;
    }
    let rest = t[1..].trim();
    if !rest.starts_with("楼") {
        return None;
    }
    let after = rest["楼".len()..].trim_start();
    let after = after
        .strip_prefix('：')
        .or_else(|| after.strip_prefix(':'))?;
    let no: u32 = after.trim().parse().ok()?;
    if after.trim().parse::<u32>().is_ok() && after.trim().len() == after.trim_end().len() {
        // 确认行尾没有别的
        let n = after.trim();
        if n.chars().all(|c| c.is_ascii_digit()) {
            return Some(no);
        }
    }
    None
}

/// 旧形四行字头：`> 楼:` / `> 从:` / `> 日期:` / `> 复楼:`
pub fn match_old_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let t = line.trim_start();
    if !t.starts_with('>') {
        return None;
    }
    let rest = t[1..].trim_start();
    if !rest.starts_with(field) {
        return None;
    }
    let after = rest[field.len()..].trim_start();
    let after = after
        .strip_prefix('：')
        .or_else(|| after.strip_prefix(':'))?;
    Some(after.trim())
}

/// 楼头时间 → epoch ms；判不出 ⇒ None（豁免侧）
pub fn floor_stamp_ms(s: &str) -> Option<i64> {
    // `YYYY-MM-DD HH:MM ±HH:MM` = 23 bytes
    let b = s.trim().as_bytes();
    if b.len() != 23 {
        return None;
    }
    let parse2 = |off: usize| -> Option<i64> { s.get(off..off + 2)?.parse().ok() };
    let sign = match b.get(17)? {
        b'+' => 1i64,
        b'-' => -1i64,
        _ => return None,
    };
    let (y, mo, d, h, mi) = (
        s.get(0..4)?.parse::<i64>().ok()?,
        parse2(5)?,
        parse2(8)?,
        parse2(11)?,
        parse2(14)?,
    );
    let oh = parse2(18)?;
    let om = parse2(21)?;
    // days from epoch (simplified — 1970-01-01 = 0)
    let days = days_from_civil(y, mo, d)?;
    let utc_ms = ((days * 24 + h) * 60 + mi) * 60_000;
    let off_ms = sign * (oh * 60 + om) * 60_000;
    Some(utc_ms - off_ms)
}

/// 简化版 civil date → days（Howard Hinnant 算法）
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// 该时间的楼是否必须两段（≥ 生效时刻）
pub fn floor_needs_two_sections(time_str: &str) -> bool {
    match floor_stamp_ms(time_str) {
        Some(t) => t >= FLOOR_TWO_SECTION_SINCE_MS,
        None => false,
    }
}

// ---- 楼头解析（新形单行） ----
/// `作者[(楼主)]→被回复者 · 时间` → FloorHead
pub fn parse_head(after_no: &str, no: u32) -> Option<FloorHead> {
    // after_no = 作者[(楼主)]→被回复者 · 时间
    let arrow_idx = after_no.find(FLOOR_ARROW)?;
    let author_part = after_no[..arrow_idx].trim();
    let time_part_full = after_no[arrow_idx + FLOOR_ARROW.len()..].trim();

    // 时间从尾部找 FLOOR_TIME_SEP
    let sep_idx = time_part_full.rfind(FLOOR_TIME_SEP)?;
    let to_part = time_part_full[..sep_idx].trim();
    let time = time_part_full[sep_idx + FLOOR_TIME_SEP.len()..]
        .trim()
        .to_string();

    // 作者侧：(楼主) 是**前缀**不是后缀
    let (author, is_owner) = if let Some(stripped) = author_part.strip_prefix(OWNER_MARK) {
        (stripped.trim().to_string(), true)
    } else {
        (author_part.to_string(), false)
    };

    // 被回复者侧：`(楼主)<名>` 或 `<M>楼[(楼主)]<名>`
    let (to_name, to_is_owner, to_floor) = if let Some(rest) = to_part.strip_prefix(OWNER_MARK) {
        (rest.trim().to_string(), true, None)
    } else if let Some(lou_idx) = to_part.find('楼') {
        let floor_no: u32 = to_part[..lou_idx].trim().parse().ok()?;
        let rest = &to_part[lou_idx + "楼".len()..];
        let (name, owner) = if let Some(s) = rest.strip_prefix(OWNER_MARK) {
            (s.trim().to_string(), true)
        } else {
            (rest.trim().to_string(), false)
        };
        (name, owner, Some(floor_no))
    } else {
        (to_part.to_string(), false, None)
    };

    Some(FloorHead {
        no,
        author,
        is_owner,
        to_name,
        to_is_owner,
        to_floor,
        time,
    })
}

// ---- 解析一封信的全部楼层 ----
pub fn parse_floors(text: &str) -> Vec<Floor> {
    let mut floors = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        // 新形
        if let Some((no, after)) = match_new_start(lines[i])
            && let Some(head) = parse_head(after, no)
        {
            let start = i;
            i += 1;
            let mut body_lines = Vec::new();
            while i < lines.len()
                && match_new_start(lines[i]).is_none()
                && match_old_start(lines[i]).is_none()
            {
                body_lines.push(lines[i]);
                i += 1;
            }
            let body = body_lines.join("\n");
            let withdrawn = body.contains("——撤回：") && body.contains("——原楼作废，理由：");
            let has_ask = body.lines().any(|l| {
                l.trim_start().starts_with("要办：") || l.trim_start().starts_with("要办:")
            });
            floors.push(Floor {
                head,
                body,
                start_line: start,
                withdrawn,
                has_ask,
            });
            continue;
        }
        // 旧形（四行）
        if match_old_start(lines[i]).is_some() && i + 3 < lines.len() {
            let from = match_old_field(lines[i + 1], "从").unwrap_or("");
            let date = match_old_field(lines[i + 2], "日期").unwrap_or("");
            let reply = match_old_field(lines[i + 3], "复楼").unwrap_or("");
            let no = match_old_start(lines[i]).unwrap_or(0);
            // 旧形没有 (楼主) 标记与楼号指向——补默认
            let (to_name, to_floor) = if reply.starts_with("无") {
                ("（旧形）".to_string(), None)
            } else {
                let n: Option<u32> = reply.trim().parse().ok();
                ("（旧形）".to_string(), n)
            };
            // 从旧形 `从:` 取名字（剥职能前缀）
            let author = from
                .chars()
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let head = FloorHead {
                no,
                author,
                is_owner: false,
                to_name,
                to_is_owner: false,
                to_floor,
                time: date.to_string(),
            };
            let start = i;
            i += 4;
            let mut body_lines = Vec::new();
            while i < lines.len()
                && match_new_start(lines[i]).is_none()
                && match_old_start(lines[i]).is_none()
            {
                body_lines.push(lines[i]);
                i += 1;
            }
            let body = body_lines.join("\n");
            let withdrawn = body.contains("——撤回：") && body.contains("——原楼作废，理由：");
            let has_ask = body.lines().any(|l| {
                l.trim_start().starts_with("要办：") || l.trim_start().starts_with("要办:")
            });
            floors.push(Floor {
                head,
                body,
                start_line: start,
                withdrawn,
                has_ask,
            });
            continue;
        }
        i += 1;
    }
    floors
}

// ---- 正文校验 checkFloorBody ----
pub fn check_floor_body(body: &str, max_chars: usize, time_str: Option<&str>) -> FloorCheck {
    let mut fc = FloorCheck::default();
    let t = body.trim();

    if t.is_empty() {
        fc.errors.push("楼层正文为空——至少要写 ### 摘要（摘要必需、正文按需），契约 §十二 4（2026-10-03 四改）".to_string());
        return fc;
    }
    if !t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        fc.errors.push(
            "正文无汉字——不是「白话」形态（契约 §十二 4：引用 ## 摘要 口径，写给隐藏读者）"
                .to_string(),
        );
    }
    if t.lines().any(|l| {
        let lt = l.trim_start();
        lt.starts_with("## 摘要") || lt.starts_with("##摘要") || lt.starts_with("##  摘要")
    }) {
        fc.errors.push("正文自带信件级 ## 摘要 块——楼层的两段用 ### 摘要／### 正文（## 是信纸级标题，混进楼层会被信纸判据误读，契约 §十二 4）".to_string());
    }
    if t.contains("<!-- LETTER-TOKEN") {
        fc.errors.push(
            "正文含 LETTER-TOKEN 行——楼层不占令牌、不进 letter-tokens.jsonl（契约 §十二 1/6）"
                .to_string(),
        );
    }
    // 新形楼层头禁入正文
    if t.lines().any(|l| match_new_start(l).is_some()) {
        fc.errors.push("正文含新形楼层头行（> N楼：…）——楼层头由工具写在本楼最前，正文里再写＝凭空多出一段假楼层（契约 §十二 4）".to_string());
    }
    // 旧形字段禁入
    for field in ["楼", "从", "日期", "复楼"] {
        if t.lines().any(|l| match_old_field(l, field).is_some()) {
            fc.errors.push(format!("正文含旧形楼层头行（> {}：…）——旧形只作存量过渡，新写的楼一律新形；手写楼层头非法（契约 §十二 4）", field));
            break;
        }
    }
    // 签名行禁入
    if t.lines().any(|l| {
        let lt = l.trim();
        lt.starts_with("——") && lt.contains(" · ") && {
            let tail = lt.rsplit(" · ").next().unwrap_or("");
            tail.len() == 10
                && tail.as_bytes().get(4) == Some(&b'-')
                && tail.as_bytes().get(7) == Some(&b'-')
        }
    }) {
        fc.errors.push("正文写了信末签名行那种形态（——… · 日期）——§五 的 f 族判据在读那种行，混进去会被误判（契约 §十二 4 格式纪律）".to_string());
    }
    // 占位检查
    if t.contains("（待填") || t.contains("(待填") {
        fc.errors
            .push("正文仍是占位（（待填…））——先写白话面再落盘".to_string());
    }

    let char_count = t.chars().count();
    if char_count > max_chars {
        fc.errors.push(format!("正文超长：{} 字 > 上限 {}（实现级防误塞上限，契约 §十二 8.3 允许；要放宽用 --max-chars）", char_count, max_chars));
    }

    // 两段制硬闸
    if let Some(ts) = time_str
        && floor_needs_two_sections(ts)
    {
        let has_summary = t.lines().any(|l| {
            let lt = l.trim();
            lt == "### 摘要" || lt == "###摘要" || lt == "###  摘要"
        });
        let has_detail = t.lines().any(|l| {
            let lt = l.trim();
            lt == "### 正文" || lt == "###正文" || lt == "###  正文"
        });
        if !has_summary {
            fc.errors.push(
                "缺 ### 摘要 段——摘要必需（白话、三句内），契约 §十二 4（2026-10-03 三改）"
                    .to_string(),
            );
        } else {
            // 摘要在前，正文在后
            let sum_idx = t.lines().position(|l| {
                let lt = l.trim();
                lt == "### 摘要" || lt == "###摘要"
            });
            let det_idx = t.lines().position(|l| {
                let lt = l.trim();
                lt == "### 正文" || lt == "正文"
            });
            if let (Some(s), Some(d)) = (sum_idx, det_idx)
                && s > d
            {
                fc.errors
                    .push("### 摘要 必须在 ### 正文 之前（契约 §十二 4 三改）".to_string());
            }
            // 摘要字数
            if let Some(si) = sum_idx {
                let summary_text: String = t
                    .lines()
                    .skip(si + 1)
                    .take_while(|l| {
                        let lt = l.trim();
                        lt != "### 正文" && lt != "###正文" && !lt.starts_with("###")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let sum_chars: usize = summary_text.chars().filter(|c| !c.is_whitespace()).count();
                if sum_chars > 125 {
                    fc.errors.push(format!(
                        "摘要 {} 字（口径：去空白字符数）＞ 上限 125（楼摘要硬上限；为宜 50–100）",
                        sum_chars
                    ));
                } else if sum_chars < 50 {
                    fc.notes.push(format!(
                        "摘要 {} 字低于宜下限 50（楼摘要；为宜 50–100）",
                        sum_chars
                    ));
                }
            }
        }
        // 正文（如果给了标记）不得为空
        if has_detail {
            let det_idx = t.lines().position(|l| {
                let lt = l.trim();
                lt == "### 正文" || lt == "###正文"
            });
            if let Some(msg) = di_check(t, det_idx) {
                fc.errors.push(msg);
            }
        }
    }
    fc
}

fn di_check(t: &str, det_idx: Option<usize>) -> Option<String> {
    let di = det_idx?;
    let detail: String = t.lines().skip(di + 1).collect::<Vec<_>>().join("\n");
    let dc = detail.trim();
    if dc.is_empty() {
        Some(
            "给了 ### 正文 标记但正文为空——要么删标记（正文按需），要么写内容（契约 §十二 4 四改）"
                .to_string(),
        )
    } else if dc.contains("（待填") {
        Some("### 正文 仍是占位（（待填…））——先写再落盘".to_string())
    } else {
        None
    }
}

// ---- 格式化：渲染新形楼头行 ----
pub fn format_head(h: &FloorHead) -> String {
    let author = if h.is_owner {
        format!("{}{}", OWNER_MARK, h.author)
    } else {
        h.author.clone()
    };
    let to = if h.to_is_owner {
        format!("{}{}", OWNER_MARK, h.to_name)
    } else if let Some(n) = h.to_floor {
        format!("{}楼{}", n, h.to_name)
    } else {
        h.to_name.clone()
    };
    format!(
        "> {}楼：{}{}{}{}{}",
        h.no, author, FLOOR_ARROW, to, FLOOR_TIME_SEP, h.time
    )
}

// ---- 格式化：渲染完整楼块（含分隔线） ----
pub fn format_floor_block(h: &FloorHead, body: &str) -> String {
    let head = format_head(h);
    format!("\n\n{}\n\n{}\n", FLOOR_SEP, head) + body + "\n"
}

// ---- 楼层段验证（verifyFloors：跨楼判据） ----
pub fn verify_floors(floors: &[Floor], roster_names: &HashSet<String>) -> Vec<String> {
    let mut errs = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut prev_no: Option<u32> = None;

    for f in floors {
        // 楼号递增无重无缺
        if seen.contains(&f.head.no) {
            errs.push(format!("楼号重复：{}楼", f.head.no));
        }
        if let Some(p) = prev_no
            && f.head.no != p + 1
        {
            errs.push(format!("楼号不递增或缺号：{}楼 → {}楼", p, f.head.no));
        }
        seen.insert(f.head.no);
        prev_no = Some(f.head.no);

        // 作者过名册
        if !roster_names.contains(&f.head.author) && f.head.author != "（旧形）" {
            errs.push(format!(
                "{}楼：作者「{}」不在名册",
                f.head.no, f.head.author
            ));
        }

        // 被回复者指向存在楼或主信
        if let Some(n) = f.head.to_floor
            && !floors.iter().any(|other| other.head.no == n)
        {
            errs.push(format!("{}楼：被回复的 {}楼 不存在", f.head.no, n));
        }

        // 时间格式（粗判：长度＋含 ±）
        let t = f.head.time.trim();
        // `YYYY-MM-DD HH:MM ±HH:MM` = 23 字节（评审 2026-10-08 修：原写 25 是错的）
        let tz_ok = t.len() == 23
            && (t.as_bytes().get(17) == Some(&b'+') || t.as_bytes().get(17) == Some(&b'-'));
        if !tz_ok || floor_stamp_ms(t).is_none() {
            errs.push(format!(
                "{}楼：时间格式不合（须 YYYY-MM-DD HH:MM ±HH:MM）：「{}」",
                f.head.no, t
            ));
        }

        // 正文校验（不撤回的楼才查）
        if !f.withdrawn {
            let fc = check_floor_body(&f.body, FLOOR_BODY_MAX, Some(&f.head.time));
            errs.extend(fc.errors);
        }
    }
    errs
}

// ---- 测试 ----
#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> HashSet<String> {
        [
            "白露", "清和", "观澜", "闻灯", "承影", "南舟", "小满", "卡萝",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    const OK_BODY: &str =
        "### 摘要\n\n你好呀，这是一句够长的白话摘要需要五十字以上才算达标所以再多写几个字凑一下。";

    #[test]
    fn test_verify_floors_pass() {
        let text = format!(
            "> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n{}\n\n---\n\n> 2楼：(楼主)清和→1楼白露 · 2026-10-04 11:00 +08:00\n\n{}",
            OK_BODY, OK_BODY
        );
        let floors = parse_floors(&text);
        let errs = verify_floors(&floors, &names());
        assert!(errs.is_empty(), "{:?}", errs);
    }

    #[test]
    fn test_verify_floors_catches_three() {
        let text = format!(
            "> 1楼：无名氏→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n{}\n\n---\n\n> 3楼：白露→2楼清和 · 2026-10-04 11:00 +08:00\n\n{}",
            OK_BODY, OK_BODY
        );
        let floors = parse_floors(&text);
        let errs = verify_floors(&floors, &names());
        assert!(errs.iter().any(|e| e.contains("不在名册")), "{:?}", errs);
        assert!(errs.iter().any(|e| e.contains("缺号")), "{:?}", errs);
        assert!(errs.iter().any(|e| e.contains("不存在")), "{:?}", errs);
    }

    #[test]
    fn test_new_start() {
        assert_eq!(
            match_new_start("> 3楼：白露→清和 · 2026-10-04 10:00 +08:00"),
            Some((3, "白露→清和 · 2026-10-04 10:00 +08:00"))
        );
        assert_eq!(
            match_new_start("> 12楼：清和→(楼主)白露 · 2026-10-04 11:00 +08:00"),
            Some((12, "清和→(楼主)白露 · 2026-10-04 11:00 +08:00"))
        );
        assert_eq!(match_new_start("普通行"), None);
        assert_eq!(match_new_start("> 楼: 5"), None); // 旧形不是新形
    }

    #[test]
    fn test_parse_head() {
        let h = parse_head("白露→(楼主)清和 · 2026-10-04 10:00 +08:00", 1).unwrap();
        assert_eq!(h.author, "白露");
        assert!(!h.is_owner);
        assert_eq!(h.to_name, "清和");
        assert!(h.to_is_owner);
        assert_eq!(h.to_floor, None);
        assert_eq!(h.time, "2026-10-04 10:00 +08:00");

        let h2 = parse_head("(楼主)清和→3楼白露 · 2026-10-04 11:00 +08:00", 4).unwrap();
        assert!(h2.is_owner);
        assert_eq!(h2.to_floor, Some(3));
        assert_eq!(h2.to_name, "白露");
    }

    #[test]
    fn test_format_head() {
        let h = FloorHead {
            no: 5,
            author: "清和".into(),
            is_owner: true,
            to_name: "白露".into(),
            to_is_owner: false,
            to_floor: Some(3),
            time: "2026-10-04 12:00 +08:00".into(),
        };
        assert_eq!(
            format_head(&h),
            "> 5楼：(楼主)清和→3楼白露 · 2026-10-04 12:00 +08:00"
        );
    }

    #[test]
    fn test_two_section_gate() {
        assert!(floor_needs_two_sections("2026-10-04 10:00 +08:00"));
        assert!(!floor_needs_two_sections("2026-10-02 10:00 +08:00"));
        assert!(!floor_needs_two_sections("bad"));
    }

    #[test]
    fn test_check_body_two_sections() {
        // 有摘要＋正文 → 过
        let fc = check_floor_body(
            "### 摘要\n\n好的收到。\n\n### 正文\n\n细节。",
            2000,
            Some("2026-10-04 10:00 +08:00"),
        );
        assert!(fc.errors.is_empty(), "{:?}", fc.errors);

        // 缺摘要 → 红
        let fc2 = check_floor_body("### 正文\n\n细节。", 2000, Some("2026-10-04 10:00 +08:00"));
        assert!(!fc2.errors.is_empty());

        // 早于生效 → 豁免
        let fc3 = check_floor_body("随便写", 2000, Some("2026-10-01 10:00 +08:00"));
        assert!(fc3.errors.is_empty());
    }

    #[test]
    fn test_parse_floors() {
        let text = "正文。\n\n---\n\n> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n### 摘要\n\n你好。\n\n---\n\n> 2楼：(楼主)清和→1楼白露 · 2026-10-04 11:00 +08:00\n\n### 摘要\n\n收到。";
        let floors = parse_floors(text);
        assert_eq!(floors.len(), 2);
        assert_eq!(floors[0].head.no, 1);
        assert_eq!(floors[1].head.no, 2);
        assert_eq!(floors[1].head.to_floor, Some(1));
    }
}
