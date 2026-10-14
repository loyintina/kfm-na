//! floor — 楼层段纯函数核（new-letter.mjs --floor 路径的 Rust 移植）。
//!
//! 纪律：本模块零 IO——所有函数收输入返回结果/诊断，不碰文件系统、不取时间、不提交。
//! 行为基准 = new-letter.mjs 第 676–1526 行＋契约 §十二，格式面逐字节兼容。
//! 生效时刻 2026-10-03 09:00 +08:00：楼头时间 ≥ 该时刻 ⇒ 两段制硬闸；早于 ⇒ 豁免。

use crate::name::is_han_str;
use crate::roster::{PoolEntry, Roster, check_pools};
use std::collections::HashMap;

// ---- 常量（照 JS 逐字） ----
pub const FLOOR_ARROW: &str = "\u{2192}"; // →
pub const FLOOR_TIME_SEP: &str = " \u{00B7} "; // · (U+00B7 + 两侧空格)
pub const FLOOR_SEP: &str = "---";
pub const OWNER_MARK: &str = "(楼主)";
pub const FLOOR_BODY_MAX: usize = 2000;
/// 两段制生效时刻（epoch ms）：2026-10-03 09:00 +08:00 = 01:00 UTC = 1790989200000
pub const FLOOR_TWO_SECTION_SINCE_MS: i64 = 1_790_989_200_000;
/// 摘要字数硬闸生效时刻（epoch ms）：2026-10-03 23:00 +08:00 = 15:00 UTC = 1791039600000。
/// **与两段制闸不是同一个时刻**（JS `SUMMARY_GATE_SINCE`）：09:00–23:00 之间的楼只判两段制、
/// 不判摘要字数（01:00 UTC 那批楼摘要超长是合法的——0110/0111 实证）
pub const FLOOR_SUMMARY_GATE_SINCE_MS: i64 = 1_791_039_600_000;
/// 旧形字头只许这四个字段（照 JS `FLOOR_OLD_HEAD_FIELDS`）
pub const FLOOR_OLD_HEAD_FIELDS: [&str; 4] = ["楼", "从", "日期", "复楼"];

/// 楼的形制：新形（单行四段）／旧形（四行字头，存量过渡）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorForm {
    New,
    Legacy,
}

impl FloorForm {
    pub fn as_str(self) -> &'static str {
        match self {
            FloorForm::New => "new",
            FloorForm::Legacy => "legacy",
        }
    }
}

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
/// 照 JS `parseFloors` 生产出的 `f` 对象形状（新形字段＋旧形字段并存，由 `form` 分辨）。
///
/// `head: FloorHead` 是**写盘渲染视图**（`format_head`／`format_floor_block` 的输入面）：
/// 它只是下面判据面字段的投影——`bailed` 时只剩 `no` 可信、旧形时 `to_name` 是原移植的
/// 「（旧形）」占位（真源里旧形 `toName` 恒 null）。判据与机读面**一律**用下面的字段。
#[derive(Debug, Clone)]
pub struct Floor {
    pub head: FloorHead,
    pub body: String,
    pub start_line: usize, // 0-indexed
    pub withdrawn: bool,
    pub has_ask: bool,
    // ---- 以下照 JS `f`（verifyFloors／--list 的判据面）----
    pub no: u32,
    pub form: FloorForm,
    /// 段界起始行原文
    pub line: String,
    /// 字头块原文行（新形 = 1 行；旧形 = 紧接起始行的连续 `>` 行）
    pub head_lines: Vec<String>,
    /// 旧形字头 `> 字段: 值`（出现序；同名取最后一次——照 JS 对象赋值语义）
    pub head_fields: Vec<(String, String)>,
    pub author_name: Option<String>,
    pub author_func: Option<String>,
    pub author_is_owner: Option<bool>,
    pub to_no: Option<u32>,
    pub to_name: Option<String>,
    pub to_is_owner: Option<bool>,
    pub time: Option<String>,
    pub head_errs: Vec<String>,
    /// 三个分隔物没划开、连段都切不出（此时 verify 不再报派生的「缺作者名／缺时间」等）
    pub bailed: bool,
}

// ---- 新形楼头解析结果（照 JS parseNewFloorHead） ----
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewFloorHead {
    pub author_name: Option<String>,
    pub author_is_owner: Option<bool>,
    pub to_no: Option<u32>,
    pub to_name: Option<String>,
    pub to_is_owner: Option<bool>,
    pub time: Option<String>,
    pub head_errs: Vec<String>,
    pub bailed: bool,
}

// ---- 被回复者按事实解（照 JS resolveFloorReply） ----
#[derive(Debug, Clone, PartialEq)]
pub struct FloorReply {
    pub to_no: Option<u32>,
    pub to_name: Option<String>,
    pub to_is_owner: Option<bool>,
    /// 指向本信不存在的楼
    pub bad_target: bool,
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
    // 冒号后只许空白 ＋ 纯数字（照 JS `^>[ \t]*楼[ \t]*[:：][ \t]*(\d+)[ \t]*$`）。
    // 2026-10-14 修：原先多一条 `after.trim().len() == after.trim_end().len()`——
    // 在「冒号后带空格」这种常规写法（`> 楼: 1`，after = " 1"）上恒假 ⇒ 旧形楼一律认不出
    // （match_old_start 返回 None ⇒ parse_floors 的旧形分支永不进入；--list 的 form 也判不出 legacy）
    let no: u32 = after.trim().parse().ok()?;
    if after.trim().chars().all(|c| c.is_ascii_digit()) {
        return Some(no);
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

/// 旧形字头单行的 `> 字段: 值`（照 JS `/^>[ \t]*([^:：]+?)[ \t]*[:：][ \t]*(.*?)[ \t]*$/`）——
/// 与 `match_old_field`（按已知字段名取）不同：本函数**不预设字段名**，用于「不得出现字段 X」判据。
/// 返回 (字段名, 值)；行不合定式 ⇒ None。
pub fn match_old_field_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('>')?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let idx = rest.find([':', '：'])?;
    let name = rest[..idx].trim_end_matches([' ', '\t']);
    if name.is_empty() {
        return None;
    }
    let value = rest[idx + rest[idx..].chars().next()?.len_utf8()..].trim_matches([' ', '\t']);
    Some((name, value))
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

/// 该时间的楼是否适用**摘要字数硬闸**（≥ 23:00 生效时刻）；判不出时间 ⇒ 豁免侧
/// （照 JS `atOrAfterSummaryGate`，与 `floor_needs_two_sections` 同制、只换阈值）
pub fn floor_needs_summary_gate(time_str: &str) -> bool {
    match floor_stamp_ms(time_str) {
        Some(t) => t >= FLOOR_SUMMARY_GATE_SINCE_MS,
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

/// 段落正文成形（照 JS parseFloors）：有下一楼时剥掉段尾那组「空行 ＋ `---` ＋ 空行」
/// 引子（它属下一楼，不进本楼正文），再剥首尾换行。
fn shape_floor_body(mut body_lines: Vec<&str>, has_next: bool) -> String {
    if has_next {
        let mut t = body_lines.len();
        while t > 0 && body_lines[t - 1].is_empty() {
            t -= 1;
        }
        if t > 0 && body_lines[t - 1] == FLOOR_SEP {
            t -= 1;
            while t > 0 && body_lines[t - 1].is_empty() {
                t -= 1;
            }
        }
        body_lines.truncate(t);
    }
    body_lines.join("\n").trim_matches('\n').to_string()
}

// ---- 新形楼层头（单行四段）：照 JS parseNewFloorHead ----
/// `> <N>楼：<作者名[(楼主)]→<被回复者> · <时间>`。
/// 解析不出的部分进 `head_errs`（由 verify_floors 报）；`bailed` ＝ 三个分隔物没划开、
/// 连段都切不出（此时不再报派生的「缺作者名／缺时间」等，免得噪声）。
/// 「右边缺名字」由 verify_floors 按事实报（那里同时知道该指向哪一楼），此处不重复。
pub fn parse_new_head(line: &str) -> NewFloorHead {
    let mut out = NewFloorHead::default();
    let rest = match_new_start(line)
        .map(|(_, after)| after.trim().to_string())
        .unwrap_or_default();
    let Some(ai) = rest.find(FLOOR_ARROW) else {
        out.head_errs
            .push(format!("楼层头不合四段——缺分隔物「{FLOOR_ARROW}」：{line}"));
        out.bailed = true;
        return out;
    };
    let mut left = rest[..ai].trim().to_string();
    let right_part = &rest[ai + FLOOR_ARROW.len()..];
    let Some(si) = right_part.rfind(FLOOR_TIME_SEP) else {
        out.head_errs.push(format!(
            "楼层头不合四段——缺分隔物「 · 」（中点＋两侧空格）：{line}"
        ));
        out.bailed = true;
        return out;
    };
    let target = right_part[..si].trim();
    out.time =
        Some(right_part[si + FLOOR_TIME_SEP.len()..].trim().to_string()).filter(|s| !s.is_empty());
    if let Some(s) = left.strip_prefix(OWNER_MARK) {
        out.author_is_owner = Some(true);
        left = s.trim().to_string();
    } else if left.contains(OWNER_MARK) {
        out.author_is_owner = Some(false);
        out.head_errs.push(format!(
            "「{OWNER_MARK}」标记只能冠在**本楼作者名**前（第 2 段）：{line}"
        ));
        left = left.replace(OWNER_MARK, "").trim().to_string();
    } else {
        out.author_is_owner = Some(false);
    }
    out.author_name = Some(left).filter(|s| !s.is_empty());
    if out.author_name.is_none() {
        out.head_errs
            .push(format!("楼层头缺本楼作者名（第 2 段）：{line}"));
    }
    if let Some(s) = target.strip_prefix(OWNER_MARK) {
        out.to_is_owner = Some(true);
        out.to_name = Some(s.trim().to_string()).filter(|s| !s.is_empty());
        out.to_no = None;
    } else {
        match parse_target_seg(target) {
            Some((n, owner, name)) => {
                out.to_no = Some(n);
                out.to_is_owner = Some(owner);
                out.to_name = Some(name).filter(|s| !s.is_empty());
            }
            None => out.head_errs.push(format!(
                "被回复者段不合式：{target}（须「(楼主)<名字>」或「<M>楼[(楼主)]<名字>」）：{line}"
            )),
        }
    }
    out
}

/// `<M>楼[(楼主)]<名字>`（照 JS `/^(\d+)楼(\(楼主\))?(.*)$/`）→ (楼号, 有无楼主标记, 名字)
fn parse_target_seg(target: &str) -> Option<(u32, bool, String)> {
    let b = target.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let no: u32 = target[..i].parse().ok()?;
    let rest = target[i..].strip_prefix('楼')?;
    let (owner, rest) = match rest.strip_prefix(OWNER_MARK) {
        Some(s) => (true, s),
        None => (false, rest),
    };
    Some((no, owner, rest.trim().to_string()))
}

/// 旧形字头取值（同名后写覆盖——照 JS 对象赋值语义）
pub fn head_get<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// byNo 表（照 JS `new Map(floors.map(f => [f.no, f]))`：同号后写覆盖）
pub fn floors_by_no(floors: &[Floor]) -> HashMap<u32, &Floor> {
    let mut m = HashMap::new();
    for f in floors {
        m.insert(f.no, f);
    }
    m
}

/// 被回复者**按事实解**（照 JS resolveFloorReply，不猜）：对主信 ⇒ 主信作者恒＝楼主；
/// 某楼 ⇒ 那一楼自己的作者。
pub fn resolve_floor_reply(
    f: &Floor,
    by_no: &HashMap<u32, &Floor>,
    owner: Option<&str>,
) -> FloorReply {
    let Some(n) = f.to_no else {
        return FloorReply {
            to_no: None,
            to_name: owner.map(str::to_string),
            to_is_owner: Some(true),
            bad_target: false,
        };
    };
    match by_no.get(&n) {
        None => FloorReply {
            to_no: Some(n),
            to_name: f.to_name.clone(),
            to_is_owner: f.to_is_owner,
            bad_target: true,
        },
        Some(t) => FloorReply {
            to_no: Some(n),
            to_is_owner: Some(t.author_name.as_deref() == owner),
            to_name: t.author_name.clone(),
            bad_target: false,
        },
    }
}

/// 楼头时间格式（照 JS `V21_DATE_RE = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2} [+-]\d{2}:\d{2}$/`）
pub fn v21_date_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 23 {
        return false;
    }
    [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 18, 19, 21, 22]
        .iter()
        .all(|&i| b[i].is_ascii_digit())
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b' '
        && b[13] == b':'
        && b[16] == b' '
        && (b[17] == b'+' || b[17] == b'-')
        && b[20] == b':'
}

/// JS `parseInt(s, 10)` 的等价（取前导十进制整数；取不到 ⇒ None）
fn parse_int_prefix(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (sign, t) = match t.as_bytes().first() {
        Some(b'-') => (-1i64, &t[1..]),
        Some(b'+') => (1i64, &t[1..]),
        _ => (1i64, t),
    };
    let n = t.bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    t[..n].parse::<i64>().ok().map(|v| sign * v)
}

// ---- 解析一封信的全部楼层 ----
/// 照 JS `parseFloors`：先扫出全部段界行（新形优先），再逐段切。
/// 段界到**下一个段界行**或**文件末尾**为止（不靠分隔线、不靠空行）。
/// 新形段界即便字头切不出（`bailed`）也照建楼——真源如此，headErrs 由 verify_floors 报。
pub fn parse_floors(text: &str) -> Vec<Floor> {
    let lines: Vec<&str> = text.lines().collect();
    let mut starts: Vec<(usize, u32, FloorForm)> = vec![];
    for (i, l) in lines.iter().enumerate() {
        if let Some((no, _)) = match_new_start(l) {
            starts.push((i, no, FloorForm::New));
            continue;
        }
        if let Some(no) = match_old_start(l) {
            starts.push((i, no, FloorForm::Legacy));
        }
    }
    let mut floors = Vec::with_capacity(starts.len());
    for (k, &(start, no, form)) in starts.iter().enumerate() {
        let end = if k + 1 < starts.len() {
            starts[k + 1].0
        } else {
            lines.len()
        };
        // 旧形：字头块 ＝ 紧接起始行的连续 `>` 行；新形：就这一行
        let mut he = start;
        let mut head_fields: Vec<(String, String)> = vec![];
        if form == FloorForm::Legacy {
            while he + 1 < end && lines[he + 1].starts_with('>') {
                he += 1;
            }
            for l in &lines[start..=he] {
                if let Some((fk, fv)) = match_old_field_line(l) {
                    head_fields.push((fk.to_string(), fv.to_string()));
                }
            }
        }
        let body = shape_floor_body(lines[he + 1..end].to_vec(), k + 1 < starts.len());
        let withdrawn = body.contains("——撤回：") && body.contains("——原楼作废，理由：");
        let has_ask = body
            .lines()
            .any(|l| l.trim_start().starts_with("要办：") || l.trim_start().starts_with("要办:"));
        let head_lines: Vec<String> = lines[start..=he].iter().map(|s| s.to_string()).collect();
        let line = lines[start].to_string();
        // 两形的字头字段（新形 = parseNewFloorHead；旧形 = 从 `从:`／`复楼:`／`日期:` 推）
        let (
            author_name,
            author_func,
            author_is_owner,
            to_no,
            to_name,
            to_is_owner,
            time,
            head_errs,
            bailed,
        ) = if form == FloorForm::New {
            let nh = parse_new_head(&line);
            (
                nh.author_name,
                None,
                nh.author_is_owner,
                nh.to_no,
                nh.to_name,
                nh.to_is_owner,
                nh.time,
                nh.head_errs,
                nh.bailed,
            )
        } else {
            let from = head_get(&head_fields, "从").unwrap_or("").to_string();
            let n = from.chars().count();
            let (an, af) = if n >= 3 {
                (
                    Some(from.chars().skip(n - 2).collect::<String>()),
                    Some(from.chars().take(n - 2).collect::<String>()),
                )
            } else if from.is_empty() {
                (None, None)
            } else {
                (Some(from.clone()), None)
            };
            let rv = head_get(&head_fields, "复楼")
                .unwrap_or("")
                .trim()
                .to_string();
            let tno = if !rv.is_empty() && rv.chars().all(|c| c.is_ascii_digit()) {
                rv.parse::<u32>().ok()
            } else {
                None
            };
            (
                an,
                af,
                None, // 旧形没有 (楼主) 标记（由 resolve_floor_reply 按事实推）
                tno,
                None,
                None,
                head_get(&head_fields, "日期")
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                vec![],
                false,
            )
        };
        // 写盘渲染视图（`head`）：判据面字段的投影，非独立解析
        let head = if form == FloorForm::New {
            FloorHead {
                no,
                author: author_name.clone().unwrap_or_default(),
                is_owner: author_is_owner.unwrap_or(false),
                to_name: to_name.clone().unwrap_or_default(),
                to_is_owner: to_is_owner.unwrap_or(false),
                to_floor: to_no,
                time: time.clone().unwrap_or_default(),
            }
        } else {
            FloorHead {
                no,
                author: author_name.clone().unwrap_or_default(),
                is_owner: false,
                to_name: "（旧形）".to_string(), // 原移植的占位；真源旧形 toName 恒 null
                to_is_owner: false,
                to_floor: to_no,
                time: time.clone().unwrap_or_default(),
            }
        };
        floors.push(Floor {
            head,
            body,
            start_line: start,
            withdrawn,
            has_ask,
            no,
            form,
            line,
            head_lines,
            head_fields,
            author_name,
            author_func,
            author_is_owner,
            to_no,
            to_name,
            to_is_owner,
            time,
            head_errs,
            bailed,
        });
    }
    floors
}

// ---- 剖开楼层正文的两段（splitFloorSections） ----
/// 楼层正文两段剖分结果。`order_ok` 只在两段都在场时有值（`### 摘要` 在前为 true）；
/// 缺任一段 = None。`summary`／`detail` = 对应段文（段头之后、下一段头之前）join("\n")
/// 后去首尾空白，空串 ⇒ None（照 JS `.trim() || null`）。
#[derive(Debug, Clone, PartialEq)]
pub struct FloorSections {
    pub has_summary: bool,
    pub has_detail: bool,
    pub order_ok: Option<bool>,
    pub summary: Option<String>,
    pub detail: Option<String>,
}

/// 段头行判据：整行匹配 `^[ \t]*###[ \t]*<word>[ \t]*$`
/// （即 `### 摘要`／`###摘要`／`###  摘要` 等变体同认，照 JS 正则）。
fn section_head_index(lines: &[&str], word: &str) -> Option<usize> {
    lines.iter().position(|l| {
        let t = l.trim_matches(|c: char| c == ' ' || c == '\t');
        match t.strip_prefix("###") {
            Some(rest) => rest.trim_matches(|c: char| c == ' ' || c == '\t') == word,
            None => false,
        }
    })
}

/// `splitFloorSections` 的移植：找第一个 `### 摘要` 行 `si`、第一个 `### 正文` 行 `bi`；
/// `summary = si===-1 ? None : lines[si+1 .. (bi>si ? bi : 末)]`、`detail = bi===-1 ? None : lines[bi+1..]`，
/// 两段都过 `.trim()`，空串 ⇒ None。
pub fn split_floor_sections(body: &str) -> FloorSections {
    let lines: Vec<&str> = body.split('\n').collect();
    let si = section_head_index(&lines, "摘要");
    let bi = section_head_index(&lines, "正文");
    let seg = |from: usize, to: usize| -> Option<String> {
        let s = lines[from + 1..to.min(lines.len())].join("\n");
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    let summary = si.and_then(|s| {
        seg(
            s,
            match bi {
                Some(b) if b > s => b,
                _ => lines.len(),
            },
        )
    });
    let detail = bi.and_then(|b| seg(b, lines.len()));
    FloorSections {
        has_summary: si.is_some(),
        has_detail: bi.is_some(),
        order_ok: match (si, bi) {
            (Some(s), Some(b)) => Some(s < b),
            _ => None,
        },
        summary,
        detail,
    }
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
    // 摘要字数硬闸（生效时刻 2026-10-03 23:00 +08:00——**不是**两段制那一个闸，照 JS 分开判）
    if let Some(ts) = time_str
        && floor_needs_summary_gate(ts)
    {
        let sum_idx = t.lines().position(|l| {
            let lt = l.trim();
            lt == "### 摘要" || lt == "###摘要" || lt == "###  摘要"
        });
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
                    "摘要 {} 字（口径：去空白字符数）＜ 为宜下限 50（楼摘要为宜 50–100；只打注意、不拒）",
                    sum_chars
                ));
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
/// 照 JS `verifyFloors` 逐条移植（结构面照 §七 强校验、文面照摘要弱校验）。
///
/// 本函数（verify 主循环）已与真源等价；余下的已知不等价**不在本函数**，而在它调用的
/// `check_floor_body`（真源 `checkFloorBody`）——见工单第 3 批交回「仍未等价的项」：
/// ① 撤回楼的正文机检被跳过（真源无条件调 checkFloorBody）——本批范围外，未动；
/// ② 真源「正文含信件字头行（父信/编号/致/抄/状态）」与「`### 摘要` 段为空／占位」两条判据未移植；
/// ③ 部分报错文案与真源不同（真源带 Markdown 标记）。
///
/// `owner` ＝ 楼主名字（CLI 侧由信封「从:」末两字取，取不到传 None）；
/// `roster` ＝ 名册（None ⇒ 名册相关判据按真源降级）；`strict_pools` ＝ 名字池严格模式。
pub fn verify_floors(
    floors: &[Floor],
    owner: Option<&str>,
    roster: Option<&Roster>,
    strict_pools: bool,
) -> FloorCheck {
    let mut out = FloorCheck::default();
    if owner.is_none() {
        out.errors.push(
            "父信信封取不到「从:」的名字——判不了楼主（楼主＝信封「从:」的名字，契约 §十二 4）"
                .to_string(),
        );
    }
    let by_no = floors_by_no(floors);
    let mut prev: u32 = 0;
    for f in floors {
        let tag = format!("第 {} 楼", f.no);
        // 头切不出段（bailed）时不再报派生的「缺作者名／缺时间」等，免噪声
        let mut derived = true;
        if f.form == FloorForm::Legacy {
            // ---- 旧形（过渡，照认）：四行字头 ----
            for l in &f.head_lines {
                match match_old_field_line(l) {
                    None => out
                        .errors
                        .push(format!("{tag} 旧形字头行不合定式（须「> 字段: 值」）：{l}")),
                    Some((k, _)) => {
                        if !FLOOR_OLD_HEAD_FIELDS.contains(&k) {
                            out.errors.push(format!(
                                "{tag} 旧形字头不得出现字段「{k}」——只许 楼/从/日期/复楼（父信/编号/致/抄/状态 都不写，契约 §十二 4）"
                            ));
                        }
                    }
                }
            }
            for k in ["楼", "从", "日期"] {
                if head_get(&f.head_fields, k).is_none() {
                    out.errors
                        .push(format!("{tag} 旧形字头缺字段「{k}」（契约 §十二 4）"));
                }
            }
            if let Some(v) = head_get(&f.head_fields, "楼").filter(|s| !s.is_empty())
                && parse_int_prefix(v) != Some(f.no as i64)
            {
                out.errors
                    .push(format!("{tag} 旧形字头「楼」与起始行不一致：{v}"));
            }
            if let Some(from) = head_get(&f.head_fields, "从").filter(|s| !s.is_empty()) {
                if from.chars().count() < 3 {
                    out.errors.push(format!(
                        "{tag} 旧形字头「从」非法：{from}（须 <职能><名字>，契约 §十二 4 过渡）"
                    ));
                } else {
                    let n = from.chars().count();
                    let entry = PoolEntry {
                        who: format!("第 {} 楼楼层作者", f.no),
                        func: Some(from.chars().take(n - 2).collect()),
                        name: from.chars().skip(n - 2).collect(),
                    };
                    let mut warns = vec![]; // 真源传 throwaway 数组 ⇒ 警告丢弃
                    check_pools(roster, &[entry], strict_pools, &mut out.errors, &mut warns);
                }
            }
        } else {
            // ---- 新形（三改）：单行四段 ----
            for e in &f.head_errs {
                out.errors.push(format!("{tag} {e}"));
            }
            if f.bailed {
                derived = false;
            } else if let (Some(o), Some(a)) = (owner, f.author_name.as_deref()) {
                if f.author_is_owner == Some(true) && a != o {
                    out.errors.push(format!(
                        "{tag} 「{OWNER_MARK}」标在了非楼主的名字旁：本楼作者 {a}，楼主是 {o}（契约 §十二 4）"
                    ));
                }
                if f.author_is_owner == Some(false) && a == o {
                    out.errors.push(format!(
                        "{tag} 本楼作者 {a} 就是楼主，第 2 段作者名前须冠「{OWNER_MARK}」（契约 §十二 4）"
                    ));
                }
            }
        }
        if derived {
            // ---- 作者名：两形共用（名字过名册；职能不进楼层头，要职能从名册派生）----
            match f.author_name.as_deref() {
                None => out.errors.push(format!("{tag} 判不出本楼作者名")),
                Some(a) if !(a.chars().count() == 2 && is_han_str(a)) => out
                    .errors
                    .push(format!("{tag} 作者名「{a}」须恰好两个汉字")),
                Some(a) if roster.is_none_or(|r| r.name_rec(a).is_none()) => {
                    out.errors.push(format!(
                        "{tag} 作者名「{a}」不在名册 names 池（唯一出处 roster.json；契约 §十二 3）"
                    ))
                }
                _ => {}
            }
            // ---- 时间（第 4 段）----
            match f.time.as_deref() {
                None => out
                    .errors
                    .push(format!("{tag} 楼层头缺时间（第 4 段，契约 §十二 4）")),
                Some(t) if !v21_date_ok(t) => out.errors.push(format!(
                    "{tag} 时间格式非法：{t}（须 YYYY-MM-DD HH:MM ±HH:MM，带时区，契约 §十二 4）"
                )),
                _ => {}
            }
            // ---- 被回复者（第 3 段）：先按事实解，再与写下的字面咬合 ----
            let r = resolve_floor_reply(f, &by_no, owner);
            if r.bad_target {
                out.errors.push(format!(
                    "{tag} 被回复者指向本信不存在的楼：{}楼（契约 §十二 4）",
                    f.to_no.unwrap_or(0)
                ));
            } else if f.form == FloorForm::New {
                match f.to_name.as_deref() {
                    None => out.errors.push(format!(
                        "{tag} 被回复者段缺名字——「{FLOOR_ARROW}」右边的名字必写（契约 §十二 4）"
                    )),
                    Some(tn) => {
                        if let Some(rn) = r.to_name.as_deref()
                            && tn != rn
                        {
                            let side = match r.to_no {
                                None => "主信作者（楼主）".to_string(),
                                Some(n) => format!("第 {n} 楼作者"),
                            };
                            out.errors.push(format!(
                                "{tag} 被回复者名字与那一侧的实际作者不符：写了 {tn}，{side}是 {rn}（契约 §十二 4：按被回复侧的实际作者填，不猜）"
                            ));
                        }
                    }
                }
                if let (Some(fo), Some(ro)) = (f.to_is_owner, r.to_is_owner)
                    && fo != ro
                {
                    let side = match r.to_no {
                        None => "主信".to_string(),
                        Some(n) => format!("第 {n} 楼"),
                    };
                    out.errors.push(format!(
                        "{tag} 被回复者的「{OWNER_MARK}」标记与实际不符：写了 {}，实际 {}——{side}的作者{}楼主 {}（契约 §十二 4）",
                        if fo { "有" } else { "无" },
                        if ro { "有" } else { "无" },
                        if ro { "就是" } else { "不是" },
                        owner.unwrap_or("null")
                    ));
                }
            }
        }
        // 正文机检（两段制／摘要字数硬闸按**本楼楼头时间**判）
        if !f.withdrawn {
            let fc = check_floor_body(&f.body, FLOOR_BODY_MAX, f.time.as_deref());
            out.errors.extend(fc.errors);
            for x in fc.notes {
                out.notes.push(format!("{tag} {x}"));
            }
        }
        if f.no <= prev {
            out.errors.push(format!(
                "{tag} 楼号未递增（前一楼 {prev}）——楼号只增不减（契约 §十二 2）"
            ));
        } else if prev != 0 && f.no != prev + 1 {
            out.errors.push(format!(
                "{tag} 楼号有缺号（前一楼 {prev}）——楼号递增且不回收（契约 §十二 2）"
            ));
        }
        prev = f.no;
    }
    out
}

// ---- 测试 ----
#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::NameRec;

    /// 名册夹具（判据只用到 names 池）
    fn roster() -> Roster {
        let pairs = [
            ("白露", "评审部"),
            ("清和", "研究部"),
            ("观澜", "开发部"),
            ("闻灯", "开发部"),
            ("承影", "测试部"),
            ("南舟", "研究部"),
            ("小满", "研究部"),
            ("卡萝", "开发部"),
        ];
        Roster {
            projects: vec![],
            functions: pairs.iter().map(|(_, f)| (*f).to_string()).collect(),
            names: pairs
                .iter()
                .map(|(n, f)| {
                    (
                        (*n).to_string(),
                        NameRec {
                            functions: Some(vec![(*f).to_string()]),
                            project: None,
                            primary: Some((*f).to_string()),
                        },
                    )
                })
                .collect(),
        }
    }

    /// 判据默认面：楼主 清和（信封 `从:` 末两字）
    const OWNER: &str = "清和";

    fn verify(text: &str) -> FloorCheck {
        verify_floors(&parse_floors(text), Some(OWNER), Some(&roster()), true)
    }

    const OK_BODY: &str =
        "### 摘要\n\n你好呀，这是一句够长的白话摘要需要五十字以上才算达标所以再多写几个字凑一下。";

    #[test]
    fn test_verify_floors_pass() {
        let text = format!(
            "> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n{}\n\n---\n\n> 2楼：(楼主)清和→1楼白露 · 2026-10-04 11:00 +08:00\n\n{}",
            OK_BODY, OK_BODY
        );
        let r = verify(&text);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }

    #[test]
    fn test_verify_floors_catches_three() {
        let text = format!(
            "> 1楼：无名→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n{}\n\n---\n\n> 3楼：白露→2楼清和 · 2026-10-04 11:00 +08:00\n\n{}",
            OK_BODY, OK_BODY
        );
        let r = verify(&text);
        assert!(
            r.errors.iter().any(|e| e.contains("不在名册")),
            "{:?}",
            r.errors
        );
        assert!(
            r.errors.iter().any(|e| e.contains("缺号")),
            "{:?}",
            r.errors
        );
        assert!(
            r.errors.iter().any(|e| e.contains("不存在")),
            "{:?}",
            r.errors
        );
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
        assert_eq!(floors[0].no, 1);
        assert_eq!(floors[1].no, 2);
        assert_eq!(floors[1].to_no, Some(1));
        assert_eq!(floors[0].form, FloorForm::New);
        assert_eq!(floors[0].author_name.as_deref(), Some("白露"));
        assert_eq!(floors[0].author_is_owner, Some(false));
        assert_eq!(floors[0].time.as_deref(), Some("2026-10-04 10:00 +08:00"));
    }

    #[test]
    fn test_old_start() {
        // 旧形段界：`> 楼: N`（整行；冒号前后可有空格/制表）——JS FLOOR_OLD_START_RE 同制
        assert_eq!(match_old_start("> 楼: 1"), Some(1));
        assert_eq!(match_old_start("> 楼:1"), Some(1));
        assert_eq!(match_old_start("> 楼：2"), Some(2));
        assert_eq!(match_old_start("> 楼： 12"), Some(12));
        assert_eq!(match_old_start(">楼:3"), Some(3));
        // 非旧形段界
        assert_eq!(
            match_old_start("> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00"),
            None
        );
        assert_eq!(match_old_start("> 楼: 1 尾巴"), None);
        assert_eq!(match_old_start("> 从: 研究部清和"), None);
    }

    #[test]
    fn test_parse_floors_legacy() {
        // 旧形四行字头（照存量过渡形态）：段界 `> 楼: N` ＋ 从／日期／复楼
        let text = "> 楼: 1\n> 从: 研究部清和\n> 日期: 2026-10-01 10:00 +08:00\n> 复楼: 无\n\n旧形一楼正文。\n\n---\n\n> 2楼：白露→1楼清和 · 2026-10-04 10:00 +08:00\n\n### 摘要\n\n新形二楼。";
        let floors = parse_floors(text);
        assert_eq!(floors.len(), 2, "{:?}", floors.len());
        assert_eq!(floors[0].no, 1);
        assert_eq!(floors[0].form, FloorForm::Legacy);
        assert_eq!(floors[0].author_name.as_deref(), Some("清和"));
        assert_eq!(floors[0].author_func.as_deref(), Some("研究部"));
        assert_eq!(floors[0].to_no, None); // 复楼: 无 ⇒ 对主信
        assert_eq!(floors[0].body, "旧形一楼正文。");
        assert_eq!(floors[0].time.as_deref(), Some("2026-10-01 10:00 +08:00"));
        assert_eq!(floors[0].head_lines.len(), 4);
        assert_eq!(floors[1].no, 2);
    }

    #[test]
    fn test_split_floor_sections_two() {
        let s = split_floor_sections("### 摘要\n\n结论。\n\n### 正文\n\n细节。");
        assert!(s.has_summary && s.has_detail);
        assert_eq!(s.order_ok, Some(true));
        assert_eq!(s.summary.as_deref(), Some("结论。"));
        assert_eq!(s.detail.as_deref(), Some("细节。"));
    }

    #[test]
    fn test_split_floor_sections_summary_only() {
        let s = split_floor_sections("### 摘要\n\n只说摘要。");
        assert!(s.has_summary);
        assert!(!s.has_detail);
        assert_eq!(s.order_ok, None);
        assert_eq!(s.summary.as_deref(), Some("只说摘要。"));
        assert_eq!(s.detail, None);
    }

    #[test]
    fn test_split_floor_sections_no_mark() {
        let s = split_floor_sections("没有标记的正文。");
        assert!(!s.has_summary && !s.has_detail);
        assert_eq!(s.order_ok, None);
        assert_eq!(s.summary, None);
        assert_eq!(s.detail, None);
    }

    #[test]
    fn test_split_floor_sections_variants_empty_and_reversed() {
        // 变体（无空格／多空格）同认（照 JS 正则）
        let s = split_floor_sections("###摘要\n\n甲乙。\n\n###  正文\n\n丙丁。");
        assert_eq!(s.summary.as_deref(), Some("甲乙。"));
        assert_eq!(s.detail.as_deref(), Some("丙丁。"));

        // 段头在但段文空 ⇒ None（照 JS `.trim() || null`）
        let e = split_floor_sections("### 摘要\n\n### 正文");
        assert!(e.has_summary && e.has_detail);
        assert_eq!(e.order_ok, Some(true));
        assert_eq!(e.summary, None);
        assert_eq!(e.detail, None);

        // 顺序颠倒：orderOk=false；detail 段跨到下一段头（JS slice(bi+1) 同制）
        let r = split_floor_sections("### 正文\n\n细节。\n\n### 摘要\n\n结论。");
        assert_eq!(r.order_ok, Some(false));
        assert_eq!(r.summary.as_deref(), Some("结论。"));
        assert_eq!(r.detail.as_deref(), Some("细节。\n\n### 摘要\n\n结论。"));
    }

    #[test]
    fn test_parse_floors_body_shape() {
        // 段尾「空行 ＋ --- ＋ 空行」引子属下一楼，不进本楼正文（JS parseFloors 同制）
        let text = "> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n### 摘要\n\n你好。\n\n---\n\n> 2楼：(楼主)清和→1楼白露 · 2026-10-04 11:00 +08:00\n\n### 摘要\n\n收到。";
        let floors = parse_floors(text);
        assert_eq!(floors[0].body, "### 摘要\n\n你好。");
        assert_eq!(floors[1].body, "### 摘要\n\n收到。");
        assert_eq!(split_floor_sections(&floors[0].body).detail, None);
    }

    #[test]
    fn test_summary_gate_separate_from_two_section() {
        // 两段制闸 09:00 生效、摘要字数闸 23:00 生效——09:00–23:00 之间的楼只判两段、不判字数
        let long = format!("### 摘要\n\n{}", "字".repeat(200));
        // 22:00 的楼：两段制要摘要（写了）⇒ 过；摘要超长但未到 23:00 ⇒ 不判字数
        let fc = check_floor_body(&long, 2000, Some("2026-10-03 22:00 +08:00"));
        assert!(fc.errors.is_empty(), "{:?}", fc.errors);
        // 23:00 之后：摘要超长 ⇒ 红
        let fc2 = check_floor_body(&long, 2000, Some("2026-10-03 23:30 +08:00"));
        assert!(
            fc2.errors.iter().any(|e| e.contains("＞ 上限 125")),
            "{:?}",
            fc2.errors
        );
        // 09:00 之前：两闸同豁免
        let fc3 = check_floor_body(&long, 2000, Some("2026-10-03 08:00 +08:00"));
        assert!(fc3.errors.is_empty(), "{:?}", fc3.errors);
        // 闸本身
        assert!(floor_needs_summary_gate("2026-10-03 23:00 +08:00"));
        assert!(!floor_needs_summary_gate("2026-10-03 22:59 +08:00"));
        assert!(!floor_needs_summary_gate("bad"));
    }

    // ---- 本批（第 3 批）新增判据：照真源 verifyFloors 逐条 ----

    /// 单楼正文用 OK_BODY（过两段制闸、无其它错）
    fn new_floor(head: &str) -> String {
        format!("{head}\n\n{OK_BODY}")
    }

    #[test]
    fn test_owner_mark_family() {
        // (a) 「(楼主)」标在了非楼主的名字旁 ⇒ 红
        let r = verify(&new_floor(
            "> 1楼：(楼主)白露→(楼主)清和 · 2026-10-04 10:00 +08:00",
        ));
        assert!(
            r.errors.iter().any(|e| e.contains("标在了非楼主的名字旁")),
            "{:?}",
            r.errors
        );
        // (b) 楼主自己开楼漏了「(楼主)」标记 ⇒ 红（上一批实证：JS 红／旧 Rust 绿）
        let r = verify(&new_floor(
            "> 1楼：清和→(楼主)清和 · 2026-10-04 10:00 +08:00",
        ));
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("就是楼主，第 2 段作者名前须冠")),
            "{:?}",
            r.errors
        );
        // (c) 楼主冠了标记 ⇒ 绿
        let r = verify(&new_floor(
            "> 1楼：(楼主)清和→(楼主)清和 · 2026-10-04 10:00 +08:00",
        ));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }

    #[test]
    fn test_reply_name_and_owner_mark_bite() {
        let two = |head2: &str| {
            format!(
                "> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n{OK_BODY}\n\n---\n\n{head2}\n\n{OK_BODY}"
            )
        };
        // (a) 被回复者段缺名字（`→1楼`）
        let r = verify(&two("> 2楼：(楼主)清和→1楼 · 2026-10-04 11:00 +08:00"));
        assert!(
            r.errors.iter().any(|e| e.contains("被回复者段缺名字")),
            "{:?}",
            r.errors
        );
        // (b) 名字与那一侧实际作者不符（写了 观澜，实际是 白露）
        let r = verify(&two("> 2楼：(楼主)清和→1楼观澜 · 2026-10-04 11:00 +08:00"));
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("被回复者名字与那一侧的实际作者不符")),
            "{:?}",
            r.errors
        );
        // (c) 「(楼主)」标记与实际不符（1楼作者 白露 不是楼主，却写了 (楼主)）
        let r = verify(&two(
            "> 2楼：(楼主)清和→1楼(楼主)白露 · 2026-10-04 11:00 +08:00",
        ));
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("「(楼主)」标记与实际不符")),
            "{:?}",
            r.errors
        );
        // (d) 双向咬合正确 ⇒ 绿
        let r = verify(&two("> 2楼：(楼主)清和→1楼白露 · 2026-10-04 11:00 +08:00"));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }

    #[test]
    fn test_legacy_head_field_checks() {
        let head = |extra: &str| format!("> 楼: 1\n{extra}\n旧形正文。");
        // (a) 不得出现字段（只许 楼/从/日期/复楼）
        let r = verify(&head(
            "> 编号: 0001\n> 从: 研究部清和\n> 日期: 2026-10-01 10:00 +08:00",
        ));
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("旧形字头不得出现字段「编号」")),
            "{:?}",
            r.errors
        );
        // (b) 缺字段（缺 日期）
        let r = verify(&head("> 从: 研究部清和"));
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("旧形字头缺字段「日期」")),
            "{:?}",
            r.errors
        );
        // (c) 字头行不合定式（连续 `>` 行里有个没冒号的）
        let r = verify(&head(
            "> 从: 研究部清和\n> 日期: 2026-10-01 10:00 +08:00\n> 乱行",
        ));
        assert!(
            r.errors.iter().any(|e| e.contains("旧形字头行不合定式")),
            "{:?}",
            r.errors
        );
        // (d) 「楼」与起始行不一致（字头块里后一条「楼」非纯数字 ⇒ 不是段界行，但覆盖了字头值）
        let r = verify(&head(
            "> 楼: 三\n> 从: 研究部清和\n> 日期: 2026-10-01 10:00 +08:00",
        ));
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("旧形字头「楼」与起始行不一致：三")),
            "{:?}",
            r.errors
        );
        // (e) 「从」非法（不足 <职能><名字> 三字）
        let r = verify(&head("> 从: 清和\n> 日期: 2026-10-01 10:00 +08:00"));
        assert!(
            r.errors.iter().any(|e| e.contains("旧形字头「从」非法")),
            "{:?}",
            r.errors
        );
        // (f) 「从」过三池（名字不在池）
        let r = verify(&head("> 从: 研究部无名\n> 日期: 2026-10-01 10:00 +08:00"));
        assert!(
            r.errors.iter().any(|e| e.contains("不在名字池")),
            "{:?}",
            r.errors
        );
        // (g) 齐备且合规 ⇒ 绿
        let r = verify(&head(
            "> 从: 研究部清和\n> 日期: 2026-10-01 10:00 +08:00\n> 复楼: 无",
        ));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }

    #[test]
    fn test_bailed_head_suppresses_derived() {
        // 段界成立但三分隔物没划开 ⇒ 恰一条头错，且不再报派生的「缺作者名／缺时间」
        let r = verify(&new_floor(
            "> 1楼：白露(楼主)清和 · 2026-10-04 10:00 +08:00",
        ));
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(
            r.errors[0].contains("楼层头不合四段——缺分隔物「→」"),
            "{:?}",
            r.errors
        );
        // 缺「 · 」分隔物
        let r = verify(&new_floor("> 1楼：白露→(楼主)清和 2026-10-04 10:00 +08:00"));
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(r.errors[0].contains("缺分隔物「 · 」"), "{:?}", r.errors);
    }

    #[test]
    fn test_notes_surfaced_with_floor_tag() {
        // 摘要低于宜下限 ⇒ 只打注意、不拒；notes 带楼号（照真源 `${tag} ${x}`）
        let body = "### 摘要\n\n这是一个不到五十字的短摘要。";
        let r = verify(&format!(
            "> 1楼：白露→(楼主)清和 · 2026-10-04 23:30 +08:00\n\n{body}"
        ));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
        assert!(r.notes[0].starts_with("第 1 楼 摘要 "), "{:?}", r.notes);
        assert!(r.notes[0].contains("为宜下限 50"), "{:?}", r.notes);
    }

    #[test]
    fn test_owner_missing_and_author_shape() {
        // 楼主判不出（信封「从:」取不到）⇒ 红
        let r = verify_floors(
            &parse_floors(&new_floor(
                "> 1楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00",
            )),
            None,
            Some(&roster()),
            true,
        );
        assert!(
            r.errors.iter().any(|e| e.contains("取不到「从:」的名字")),
            "{:?}",
            r.errors
        );
        // 作者名须恰好两个汉字（三字名 ⇒ 只报「须恰好两个汉字」，不报「不在名册」）
        let r = verify(&new_floor(
            "> 1楼：无名氏→(楼主)清和 · 2026-10-04 10:00 +08:00",
        ));
        assert!(
            r.errors.iter().any(|e| e.contains("须恰好两个汉字")),
            "{:?}",
            r.errors
        );
        assert!(
            !r.errors.iter().any(|e| e.contains("不在名册")),
            "{:?}",
            r.errors
        );
        // 楼号未递增／缺号
        let text = format!(
            "> 2楼：白露→(楼主)清和 · 2026-10-04 10:00 +08:00\n\n{OK_BODY}\n\n---\n\n> 2楼：白露→(楼主)清和 · 2026-10-04 11:00 +08:00\n\n{OK_BODY}"
        );
        let r = verify(&text);
        assert!(
            r.errors.iter().any(|e| e.contains("楼号未递增")),
            "{:?}",
            r.errors
        );
    }
}
