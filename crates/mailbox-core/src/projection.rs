//! gen 投影：README 两区段渲染 + letters-index.jsonl + 归属扫描
//! （移植 gen-agent-inbox.mjs 拼表格/活信清单/派生索引/归属行扫描器）。

use crate::header::{header_get, parse_header, parse_to_field};
use crate::json::{JVal, jobj, jopt_str, jstr, to_json_string};
use crate::name::{is_v21_name, parse_v21_name};
use crate::roster::{Roster, split_func};
use crate::status::is_debt;
use crate::token::sha256_hex16;
use crate::verify::LetterText;

pub const MARK_START: &str = "<!-- gen:agent-inbox:start -->";
pub const MARK_END: &str = "<!-- gen:agent-inbox:end -->";
pub const PENDING_START: &str = "<!-- gen:pending:start -->";
pub const PENDING_END: &str = "<!-- gen:pending:end -->";

const V21_NEED: [&str; 5] = ["日期", "从", "致", "复", "状态"];
const LEGACY_NEED: [&str; 7] = ["日期", "致", "流型", "预期表态方", "收敛判据", "回", "状态"];
const HEADER_NAMES: [&str; 10] = [
    "编号",
    "日期",
    "从",
    "致",
    "复",
    "流型",
    "预期表态方",
    "收敛判据",
    "回",
    "状态",
];

#[derive(Debug)]
struct Row<'a> {
    letter: &'a LetterText,
    v21: bool,
    h: Vec<(String, String)>,
    parsed: Option<crate::name::V21Parsed>,
    /// 投影第一列（v2.1 截前 10 字符；存量信原样）
    date_col: String,
}

impl Row<'_> {
    fn cell(&self, key: &str) -> Option<&str> {
        header_get(&self.h, key)
    }
    fn to_cell(&self) -> &str {
        self.cell("致").unwrap_or("")
    }
    fn hui_cell(&self) -> &str {
        if self.v21 {
            self.cell("复").unwrap_or("")
        } else {
            self.cell("回").unwrap_or("")
        }
    }
    fn status(&self) -> &str {
        self.cell("状态").unwrap_or("")
    }
    /// 编号列：v2.1 = 分拣码+号；存量 = NNNN- 前缀
    fn no_of(&self) -> Option<String> {
        if self.v21 {
            let p = self.parsed.as_ref()?;
            let no = p.no.as_ref()?;
            Some(format!("{}{}", p.sorting.as_deref().unwrap_or(""), no))
        } else {
            let re = regex::Regex::new(r"^[A-Z]{0,4}(\d{4})-").unwrap();
            re.captures(&self.letter.file).map(|m| m[1].to_string())
        }
    }
}

fn build_rows<'a>(letters: &'a [LetterText], errors: &mut Vec<String>) -> Vec<Row<'a>> {
    let mut rows = vec![];
    for l in letters {
        let f = l.file.as_str();
        let h = parse_header(&l.text, &HEADER_NAMES);
        let v21 = is_v21_name(f);
        let miss: Vec<&str> = if v21 {
            V21_NEED
                .into_iter()
                .filter(|k| header_get(&h, k).is_none())
                .collect()
        } else {
            LEGACY_NEED
                .into_iter()
                .filter(|k| header_get(&h, k).is_none())
                .collect()
        };
        if !miss.is_empty() {
            errors.push(if v21 {
                format!(
                    "{f} 信封缺字段：{}——v2.1 必填 日期/从/致/复/状态（编号可省）（见 mailbox-contract-v2.1.md §三）",
                    miss.join("、")
                )
            } else {
                format!(
                    "{f} 机读头缺字段：{}——新信必填七字段（见 README 机读头 schema 条）",
                    miss.join("、")
                )
            });
            continue;
        }
        if v21 {
            let p = parse_v21_name(f);
            if !p.errs.is_empty() {
                errors.push(format!("{f} 文件名文法：{}", p.errs.join("；")));
                continue;
            }
            let date_col = header_get(&h, "日期")
                .unwrap_or("")
                .chars()
                .take(10)
                .collect();
            rows.push(Row {
                letter: l,
                v21: true,
                parsed: Some(p),
                date_col,
                h,
            });
        } else {
            let date_col = header_get(&h, "日期").unwrap_or("").to_string();
            rows.push(Row {
                letter: l,
                v21: false,
                parsed: None,
                date_col,
                h,
            });
        }
    }
    // 日期序 → 文件名序（JS localeCompare 在本域退化为字典序：日期等长数字前缀）
    rows.sort_by(|a, b| {
        a.date_col
            .cmp(&b.date_col)
            .then_with(|| a.letter.file.cmp(&b.letter.file))
    });
    rows
}

/// 栏位 → 链接前缀（非在册行的链接指向所在栏——台账仍是单一出处，
/// 表的每一行都指向真文件；契约 §八 第 9 条：撤回栏 archive-withdrawn/）
fn loc_prefix(dir: &str) -> &'static str {
    match dir {
        "archive-v1" => "archive-v1/",
        "archive-v2.2" => "archive-v2.2/",
        "withdrawn" => "archive-withdrawn/",
        _ => "",
    }
}

/// 「回哪条/状态」列的非在册链接补前缀（fixHui）——archive-v1、archive-v2.2 与
/// archive-withdrawn 三栏都补（JS LOC_PREFIX 同款）
fn fix_hui(cell: &str, prefixed: &[(&str, &str)]) -> String {
    let mut out = cell.to_string();
    for (n, p) in prefixed {
        out = out.replace(&format!("]({n})"), &format!("]({p}{n})"));
        out = out.replace(&format!("`{n}`"), &format!("`{p}{n}`"));
    }
    out
}

/// 文末台账区段（gen:agent-inbox）
fn render_ledger(rows: &[Row]) -> String {
    let prefixed: Vec<(&str, &str)> = rows
        .iter()
        .filter(|r| r.letter.dir != "active")
        .map(|r| (r.letter.file.as_str(), loc_prefix(&r.letter.dir)))
        .collect();
    let mut lines = vec![
        MARK_START.to_string(),
        "| 日期 | 信件 | 回哪条 | 状态 |".to_string(),
        "|------|------|--------|------|".to_string(),
    ];
    for r in rows {
        let f = r.letter.file.as_str();
        let link = format!("{}{f}", loc_prefix(&r.letter.dir));
        // 撤回件的状态是原件状态（窗口＝零回应，故仍「待*」）——加尾注免得
        // 读者把它当活信（契约 §八 第 9 条④活信清单剔除的表内对应）
        let status_cell = format!(
            "{}{}",
            fix_hui(r.status(), &prefixed),
            if r.letter.dir == "withdrawn" {
                "（已撤回）"
            } else {
                ""
            }
        );
        lines.push(format!(
            "| {} | [`{}`]({}) | {} | {} |",
            r.date_col,
            f,
            link,
            fix_hui(r.hui_cell(), &prefixed),
            status_cell
        ));
    }
    lines.push(MARK_END.to_string());
    lines.join("\n")
}

fn cell_of(v: Option<&str>) -> &str {
    match v {
        Some(s) if !s.is_empty() => s,
        _ => "—",
    }
}

/// 顶部活信清单区段（gen:pending；只列在册、待*，按编号升序）
fn render_pending(rows: &[Row]) -> String {
    let mut pending: Vec<&Row> = rows
        .iter()
        .filter(|r| r.letter.dir == "active" && is_debt(r.status()))
        .collect();
    pending.sort_by(|a, b| {
        a.no_of()
            .unwrap_or_default()
            .cmp(&b.no_of().unwrap_or_default())
            .then_with(|| a.letter.file.cmp(&b.letter.file))
    });
    let mut lines = vec![PENDING_START.to_string()];
    if pending.is_empty() {
        lines.push("暂无".to_string());
    } else {
        lines.push(format!(
            "**当前待回信（活信清单，生成器维护，勿手改）**：{} 封",
            pending.len()
        ));
        lines.push(String::new());
        lines.push("| 编号 | 日期 | 从 | 致 | 事由 | 状态 |".to_string());
        lines.push("|---|---|---|---|---|---|".to_string());
        for r in pending {
            let subject = r.parsed.as_ref().and_then(|p| p.subject.as_deref());
            lines.push(format!(
                "| {} | {} | {} | {} | {} | {} |",
                cell_of(r.no_of().as_deref()),
                cell_of(Some(&r.date_col)),
                cell_of(r.cell("从")),
                cell_of(Some(r.to_cell())),
                cell_of(subject),
                cell_of(Some(r.status()))
            ));
        }
    }
    lines.push(PENDING_END.to_string());
    lines.join("\n")
}

/// 「类别/要办/算完」信纸块字段提取（blockField）
pub fn block_field(text: &str, label: &str) -> Option<String> {
    for line in text.split('\n') {
        let mut t = line.trim_start();
        if let Some(rest) = t.strip_prefix(['-', '*']) {
            t = rest.trim_start();
        }
        let Some(after) = t.strip_prefix(label) else {
            continue;
        };
        let Some(after) = after.trim_start().strip_prefix([':', '：']) else {
            continue;
        };
        let v = after.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn to_items_jval(value: &str) -> (Vec<JVal>, bool) {
    let mut errs = vec![];
    let items = parse_to_field(value, &mut errs);
    let to_all = items
        .iter()
        .any(|it| it.name == "全体" && it.func.is_none());
    let arr = items
        .iter()
        .map(|it| {
            jobj(vec![
                ("func", jopt_str(it.func.as_deref())),
                ("name", jstr(&it.name)),
                ("inherited", JVal::Bool(it.inherited)),
            ])
        })
        .collect();
    (arr, to_all)
}

/// 派生索引单条记录（v2.1 / legacy 字段集与 JS 相同，key 序同 JS 对象字面量）。
/// `book_sorting` = 本册身份码（契约 §六《册身份》）：v2.1 行 `sorting` 取
/// `文件名解析出的码 || 本册身份码`——册码是册的固有属性，不再硬编码兜底 MAIN。
/// 取值由 IO 壳（mailbox-cli）从 `<册根>/.mailbox.json` 解析后传入（core 零 IO）。
fn index_record(r: &Row, roster: Option<&Roster>, book_sorting: &str) -> JVal {
    let text = &r.letter.text;
    let hash = jstr(&sha256_hex16(text));
    let f = r.letter.file.as_str();
    if !r.v21 {
        let no = regex::Regex::new(r"^[A-Z]{0,4}(\d{4})-")
            .unwrap()
            .captures(f)
            .map(|m| m[1].to_string());
        let toks: Vec<JVal> = r
            .to_cell()
            .split(['，', ',', '、', ' ', '\t', '\n', '\r'])
            .filter(|s| !s.is_empty())
            .map(jstr)
            .collect();
        let to_all = r
            .to_cell()
            .split(['，', ',', '、', ' ', '\t', '\n', '\r'])
            .any(|t| t == "all");
        let base = f.strip_suffix(".md").unwrap_or(f);
        let type_ = [
            "landing-report",
            "submission",
            "review",
            "response",
            "report",
            "verdict",
            "notice",
            "landing",
        ]
        .into_iter()
        .find(|t| base.ends_with(&format!("-{t}")));
        return jobj(vec![
            ("no", jopt_str(no.as_deref())),
            ("file", jstr(f)),
            ("dir", jstr(&r.letter.dir)),
            ("era", jstr("legacy")),
            ("time", jopt_str(r.cell("日期"))),
            ("to_lines", JVal::Arr(toks)),
            ("to_all", JVal::Bool(to_all)),
            ("type", jopt_str(type_)),
            ("subject", JVal::Null),
            ("kind", jopt_str(r.cell("流型"))),
            ("expect", jopt_str(r.cell("预期表态方"))),
            ("criteria", jopt_str(r.cell("收敛判据"))),
            ("status", jopt_str(r.cell("状态"))),
            ("hash", hash),
        ]);
    }
    let p = r.parsed.as_ref().unwrap();
    let from_val = r.cell("从").unwrap_or("");
    let from_name = if from_val.is_empty() {
        None
    } else {
        Some(
            from_val
                .chars()
                .skip(from_val.chars().count().saturating_sub(2))
                .collect::<String>(),
        )
    };
    let raw_func: String = from_val
        .chars()
        .take(from_val.chars().count().saturating_sub(2))
        .collect();
    let mut from_project: Option<String> = None;
    let mut from_func = raw_func.clone();
    if let Some(roster) = roster {
        let (proj, rest) = split_func(roster, &raw_func);
        if let Some(proj) = proj {
            from_project = Some(proj.to_string());
            from_func = rest.to_string();
        }
        if from_project.is_none() {
            from_project = from_name
                .as_deref()
                .and_then(|n| roster.name_rec(n))
                .and_then(|rec| rec.project.clone());
        }
    }
    let (to, to_all) = to_items_jval(r.cell("致").unwrap_or(""));
    let reply_raw: String = r
        .cell("复")
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let reply_to = if reply_raw.is_empty() || reply_raw.starts_with('无') {
        None
    } else {
        Some(reply_raw)
    };
    fn non_empty(s: &str) -> Option<&str> {
        if s.is_empty() { None } else { Some(s) }
    }
    jobj(vec![
        ("no", jopt_str(p.no.as_deref())),
        (
            "sorting",
            jstr(if p.sorting.as_deref().unwrap_or("").is_empty() {
                book_sorting
            } else {
                p.sorting.as_deref().unwrap()
            }),
        ),
        ("file", jstr(f)),
        ("dir", jstr(&r.letter.dir)),
        ("era", jstr("v2.1")),
        ("time", jopt_str(r.cell("日期"))),
        ("from_func", jopt_str(non_empty(&from_func))),
        ("from_name", jopt_str(from_name.as_deref())),
        ("from_project", jopt_str(from_project.as_deref())),
        ("to", JVal::Arr(to)),
        ("to_all", JVal::Bool(to_all)),
        ("reply_to", jopt_str(reply_to.as_deref())),
        ("type", jopt_str(p.type_word.as_deref())),
        ("subject", jopt_str(p.subject.as_deref())),
        ("kind", jopt_str(block_field(text, "类别").as_deref())),
        ("expect", jopt_str(block_field(text, "要办").as_deref())),
        ("criteria", jopt_str(block_field(text, "算完").as_deref())),
        ("status", jopt_str(r.cell("状态"))),
        ("hash", hash),
    ])
}

#[derive(Debug, Default)]
pub struct GenOutput {
    pub errors: Vec<String>,
    pub pending_section: String,
    pub ledger_section: String,
    pub index_text: String,
    pub rows: usize,
    pub active: usize,
    /// archive-v1 + archive-v2.2 两归档栏合计（撤回栏单列，不计入 9.0「N 封信」口径）
    pub archive: usize,
    /// 撤回栏（archive-withdrawn/）件数——物理在册可审计，但不计 9.0 计数
    /// （口径 = 在册 + 归档；契约 §八 第 9 条补注⑥）
    pub withdrawn: usize,
}

/// 全量渲染（行构建 → 两区段 + 索引文本；字段缺失/文法错记 errors 并跳过该行）。
/// `book_sorting` = 本册身份码（契约 §六《册身份》），v2.1 行无文件名码时取它；
/// 主册/无身份文件的历史行为等价于传 `"MAIN"`。
pub fn render_gen(
    letters: &[LetterText],
    roster: Option<&Roster>,
    book_sorting: &str,
) -> GenOutput {
    let mut out = GenOutput::default();
    let rows = build_rows(letters, &mut out.errors);
    out.rows = rows.len();
    out.active = rows.iter().filter(|r| r.letter.dir == "active").count();
    out.withdrawn = rows.iter().filter(|r| r.letter.dir == "withdrawn").count();
    out.archive = rows.len() - out.active - out.withdrawn;
    out.pending_section = render_pending(&rows);
    out.ledger_section = render_ledger(&rows);
    out.index_text = if rows.is_empty() {
        String::new()
    } else {
        rows.iter()
            .map(|r| to_json_string(&index_record(r, roster, book_sorting)))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    out
}

/// 区段替换：doc 中 start..end（含两标记）整段换成 section（含标记）。
pub fn splice_section(doc: &str, start: &str, end: &str, section: &str) -> Result<String, String> {
    let Some(s) = doc.find(start) else {
        return Err(format!("README 缺标记区段（缺 start）：{start}"));
    };
    let Some(e) = doc[s..].find(end).map(|i| s + i) else {
        return Err(format!("README 标记区段缺 end：{start}"));
    };
    Ok(format!("{}{}{}", &doc[..s], section, &doc[e + end.len()..]))
}

#[derive(Debug, Clone, Copy)]
pub enum ScanTarget<'a> {
    /// --for=<职能|名字|旧线名>
    For(&'a str),
    /// --by=<名字>（限 v2.1 信）
    By(&'a str),
}

#[derive(Debug, Clone)]
pub struct ScanHit {
    pub book: String,
    pub archived: bool,
    pub file: String,
    pub status: String,
}

/// 归属行扫描：状态以「待」开头且致含目标（或全体/all 广播）的信。
/// books = (册标签, 信件集合)；跨册由调用方喂多册。
pub fn scan_debts(
    books: &[(&str, &[LetterText])],
    target: ScanTarget,
) -> (Vec<String>, Vec<ScanHit>) {
    let mut errors = vec![];
    let mut hits = vec![];
    for (label, letters) in books {
        let rows = build_rows(letters, &mut errors);
        for r in &rows {
            // 撤回件不参与（撤回＝整封作废，不是欠账；活信清单同此口径）
            if r.letter.dir == "withdrawn" || !is_debt(r.status()) {
                continue;
            }
            let hit = match target {
                ScanTarget::By(name) => {
                    r.v21 && r.parsed.as_ref().and_then(|p| p.from_name.as_deref()) == Some(name)
                }
                ScanTarget::For(t) => {
                    if r.v21 {
                        let mut errs = vec![];
                        let items = parse_to_field(r.cell("致").unwrap_or(""), &mut errs);
                        if items
                            .iter()
                            .any(|it| it.name == "全体" && it.func.is_none())
                        {
                            true // 全体广播
                        } else {
                            items
                                .iter()
                                .any(|it| it.func.as_deref() == Some(t) || it.name == t)
                        }
                    } else {
                        let toks: Vec<&str> = r
                            .to_cell()
                            .split(['，', ',', '、', ' ', '\t', '\n', '\r'])
                            .filter(|s| !s.is_empty())
                            .collect();
                        toks.contains(&t) || toks.contains(&"all")
                    }
                }
            };
            if hit {
                hits.push(ScanHit {
                    book: label.to_string(),
                    archived: r.letter.dir != "active",
                    file: r.letter.file.clone(),
                    status: r.status().to_string(),
                });
            }
        }
    }
    (errors, hits)
}
