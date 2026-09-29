//! verify：单信模式（new-letter --verify）与全册执法（check-letter-token 主循环）。

use crate::header::{header_get, parse_header, parse_to_field};
use crate::name::{is_v21_name, legacy_no_of, parse_v21_name, v21_no_of};
use crate::roster::{PoolEntry, Roster, check_pools};
use crate::status::{
    build_status_re, has_v21_stamp, is_valid_v21_date, parse_status_words, status_is_pending, to_ms,
};
use crate::token::{self, Ledger, fingerprint, parse_ledger};
use std::collections::{HashMap, HashSet};

pub const PLAIN_HEAD: &str =
    "## 摘要\n\n> 注意（写给隐藏读者）：三句话内说清是什么事、要不要我做事；不写工作术语。";

const V21_FIELDS: [&str; 6] = ["编号", "日期", "从", "致", "复", "状态"];
const LEGACY_FIELDS: [&str; 7] = ["日期", "致", "流型", "预期表态方", "收敛判据", "回", "状态"];

#[derive(Debug, Clone)]
pub struct LetterText {
    pub file: String,
    /// "active" | "archive-v1" | "withdrawn"（archive-withdrawn/ 撤回栏：
    /// 不参与执法，但孤儿判据认这一栏——契约 §八 第 9 条《撤回票》）
    pub dir: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Diags {
    pub errs: Vec<String>,
    pub warns: Vec<String>,
    pub v21_count: usize,
    pub legacy_count: usize,
    pub current_tickets: usize,
    pub revoked_tickets: usize,
    /// 撤销票（换票的一半，被某张票以 renamedFrom 引用）
    pub replaced_tickets: usize,
    /// 撤回票（整封作废，无人引用；契约 §八 第 9 条）
    pub withdrawn_tickets: usize,
}

impl Diags {
    pub fn is_clean(&self) -> bool {
        self.errs.is_empty()
    }
}

pub struct BookCheck<'a> {
    pub letters: &'a [LetterText],
    pub tokens_text: Option<&'a str>,
    pub roster: Option<&'a Roster>,
    /// 状态词表出处文本（README；缺失=空串，状态词前缀不查只警告）
    pub readme_text: &'a str,
    /// 存量信命名前缀（如 "kfm-na|kfmv4" / na 册 "kfm-na|na"）
    pub name_prefix: &'a str,
    pub v1_files: &'a HashSet<String>,
    pub strict_pools: bool,
}

// ---------------------------------------------------------------
// 摘要块（制度名"白话面／白话结论"，信里呈现为 `## 摘要`）
// ---------------------------------------------------------------

/// 摘要块存在（`^##\s*摘要` 前缀匹配；旧形态 `## 白话结论…` 已于 2026-09-29
/// 由用户拍板废止——两册全部信已统一，故只认 `## 摘要`，与 JS 三件套同步）
pub fn has_plain_block(text: &str) -> bool {
    plain_re().is_match(text)
}

fn plain_re() -> regex::Regex {
    regex::Regex::new(r"(?m)^##\s*摘要").unwrap()
}

/// 摘要块正文（标题行之后至下一个标题前；JS split 同款）
pub fn plain_body(text: &str) -> String {
    let re = plain_re();
    let Some(m) = re.find(text) else {
        return String::new();
    };
    let body = &text[m.end()..];
    let cut = regex::Regex::new(r"\n#{1,3}\s").unwrap();
    match cut.find(body) {
        Some(c) => body[..c.start()].to_string(),
        None => body.to_string(),
    }
}

/// 占位符 `（待填`
pub fn has_placeholder(s: &str) -> bool {
    s.contains("（待填")
}

// ---------------------------------------------------------------
// 令牌与台账咬合（两纪元共用）
// ---------------------------------------------------------------

fn check_token_book(d: &mut Diags, ledger: &Ledger, f: &str, text: &str, expect_no: Option<&str>) {
    let Some(tm) = token::find_token(text) else {
        d.errs.push(format!(
            "b. 缺令牌：{f} 没有 LETTER-TOKEN v2 行——本信未由 new-letter.mjs 签发（水印即票据）"
        ));
        return;
    };
    if let Some(e) = expect_no
        && tm.no != e
    {
        d.errs.push(format!(
            "b. 编号不一致：文件名 {e} vs 令牌 {}（{f}）",
            tm.no
        ));
    }
    let Some(rec) = ledger.find_current(&tm.no) else {
        d.errs.push(format!(
            "c. 票据不在台账：{f} 的 no={} 未在 letter-tokens.jsonl 登记",
            tm.no
        ));
        return;
    };
    if rec.file.as_deref() != Some(f) {
        d.errs.push(format!(
            "c. 台账 file 与实际不符：台账={} 实际={f}（票据被套用或改名未换票）",
            rec.file.as_deref().unwrap_or("")
        ));
    }
    if rec.nonce.as_deref() != Some(tm.nonce.as_str()) {
        d.errs.push(format!("c. nonce 与台账不符：{f}"));
    }
    if rec.fp.as_deref() != Some(tm.fp.as_str()) {
        d.errs.push(format!("c. fp 与台账不符：{f}"));
    }
    if fingerprint(&tm.no, &tm.nonce, f) != tm.fp {
        d.errs
            .push(format!("c. 指纹重算不符：{f}（编号/nonce/文件名被改过）"));
    }
}

fn legacy_type_ok(f: &str) -> bool {
    regex::Regex::new(
        r"-(submission|review|response|report|verdict|notice|landing|landing-report)\.md$",
    )
    .unwrap()
    .is_match(f)
}

fn legacy_name_ok(f: &str, name_prefix: &str) -> bool {
    regex::Regex::new(&format!(r"^\d{{4}}-({name_prefix})-[\x20-\x7E]+"))
        .map(|re| re.is_match(f))
        .unwrap_or(false)
}

fn last_chars(s: &str, n: usize) -> String {
    let skip = s.chars().count().saturating_sub(n);
    s.chars().skip(skip).collect()
}

fn drop_last_chars(s: &str, n: usize) -> String {
    let keep = s.chars().count().saturating_sub(n);
    s.chars().take(keep).collect()
}

fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn no4(s: &str) -> String {
    last_chars(s, 4)
}

// ---------------------------------------------------------------
// 全册执法
// ---------------------------------------------------------------

/// 全册执法（check-letter-token.mjs 主循环逐条移植，含跨信判据）
pub fn verify_book(b: &BookCheck) -> Diags {
    let mut d = Diags::default();
    let ledger = b.tokens_text.map(parse_ledger).unwrap_or_default();
    for e in &ledger.errs {
        d.errs.push(format!("c. {e}"));
    }
    for w in &ledger.warns {
        d.warns.push(format!("c. {w}"));
    }

    // 状态词表（唯一出处 = README 规则区）
    let words = parse_status_words(b.readme_text);
    if words.is_empty() {
        d.warns.push(
            "README 规则区「合法状态词表」解析失败——状态词前缀本次不查（词表唯一出处结构可能变了）"
                .to_string(),
        );
    }
    let status_re = build_status_re(&words);

    // 编号全集（复 存在性判据）——撤回栏信不算「在本信箱可见」（不参与执法同口径）
    let mut known: HashSet<String> = HashSet::new();
    for l in b.letters {
        if l.dir == "withdrawn" {
            continue;
        }
        if let Some(full) = v21_no_of(&l.file).or_else(|| legacy_no_of(&l.file)) {
            known.insert(no4(&full));
            known.insert(full);
        }
    }
    for t in &ledger.current {
        known.insert(no4(&t.no));
        known.insert(t.no.clone());
    }

    let mut seen_no: HashMap<String, String> = HashMap::new();
    let mut v21_times: Vec<(String, String, f64)> = vec![];
    let full_no_re = regex::Regex::new(r"^[A-Z]{0,4}\d{4}$").unwrap();

    for l in b.letters {
        // 撤回栏（archive-withdrawn/）不参与执法（撤回＝整封作废）——不进枚举，
        // 只在本函数末尾的孤儿/半状态/防重判据里认这个位置（契约 §八 第 9 条）
        if l.dir == "withdrawn" {
            continue;
        }
        if b.v1_files.contains(&l.file) {
            continue; // v1 名单：只读，不追改不补水印
        }
        let f = l.file.as_str();
        let text = l.text.as_str();

        // ========== v2.1 新纪元 ==========
        if is_v21_name(f) {
            d.v21_count += 1;
            let p = parse_v21_name(f);
            for e in &p.errs {
                d.errs
                    .push(format!("a. 文件名文法：{f} — {e}（⛳ MECH-FLOW-12）"));
            }
            let full_no =
                p.no.as_ref()
                    .map(|no| format!("{}{}", p.sorting.as_deref().unwrap_or(""), no));
            if let Some(n) = &full_no {
                if let Some(prev) = seen_no.get(n) {
                    d.errs.push(format!("e. 编号重复：{n}（{prev} 与 {f}）"));
                } else {
                    seen_no.insert(n.clone(), f.to_string());
                }
            }
            check_token_book(&mut d, &ledger, f, text, full_no.as_deref());

            let h = parse_header(text, &V21_FIELDS);
            let miss: Vec<&str> = ["日期", "从", "致", "复", "状态"]
                .into_iter()
                .filter(|k| header_get(&h, k).is_none())
                .collect();
            if !miss.is_empty() {
                d.errs.push(format!(
                    "a. 信封缺字段：{f} 缺 {}——v2.1 必填 日期/从/致/复/状态（编号可省、以文件名为准）（⛳ MECH-FLOW-15）",
                    miss.join("、")
                ));
            }
            if let (Some(hv), Some(n)) = (header_get(&h, "编号"), &full_no) {
                let hv_clean: String = hv.chars().filter(|c| !c.is_whitespace()).collect();
                if hv_clean != *n {
                    d.errs.push(format!(
                        "a. 信封「编号」与文件名不一致：{hv} vs {n}（编号以文件名为准）（⛳ MECH-FLOW-15）"
                    ));
                }
            }
            if let Some(dv) = header_get(&h, "日期") {
                if !is_valid_v21_date(dv) {
                    d.errs.push(format!(
                        "a. 信封「日期」格式非法：{f} 「{dv}」（须 YYYY-MM-DD HH:MM ±HH:MM，带时区）（⛳ MECH-FLOW-15）"
                    ));
                } else {
                    v21_times.push((
                        full_no.clone().unwrap_or_else(|| f.to_string()),
                        f.to_string(),
                        to_ms(dv),
                    ));
                }
            }
            let mut pool_errs = vec![];
            let mut entries = vec![];
            let from_val = header_get(&h, "从").unwrap_or("");
            if !from_val.is_empty() && from_val.chars().count() < 3 {
                d.errs.push(format!(
                    "a. 信封「从」非法：{f} 「{from_val}」（须 <职能><名字>，如 运维部蔚然；裸职能非法）（⛳ MECH-FLOW-15）"
                ));
            } else if !from_val.is_empty() {
                entries.push(PoolEntry {
                    who: format!("发信人（{f}）"),
                    func: Some(drop_last_chars(from_val, 2)),
                    name: last_chars(from_val, 2),
                });
            }
            if !from_val.is_empty()
                && let Some(pn) = &p.from_name
                && last_chars(from_val, 2) != *pn
            {
                d.errs.push(format!(
                    "a. 信封「从」名字 {} 与文件名发信人 {pn} 不一致（{f}）",
                    last_chars(from_val, 2)
                ));
            }
            let to_items = match header_get(&h, "致") {
                Some(v) => parse_to_field(v, &mut pool_errs),
                None => vec![],
            };
            if header_get(&h, "致").is_some() && to_items.is_empty() {
                pool_errs.push("信封「致」为空".to_string());
            }
            for it in &to_items {
                entries.push(PoolEntry {
                    who: format!("收件人（{f}）"),
                    func: it.func.clone(),
                    name: it.name.clone(),
                });
            }
            for e in pool_errs {
                d.errs.push(format!("a. {e}（{f}）"));
            }
            check_pools(
                b.roster,
                &entries,
                b.strict_pools,
                &mut d.errs,
                &mut d.warns,
            );

            // 裸职能定点报错（如 收件人只写「研究部」）
            if let Some(roster) = b.roster {
                let mut raws: Vec<(String, &str)> = vec![];
                if let Some(to) = &p.to {
                    for it in to.strip_suffix('等').unwrap_or(to).split('及') {
                        raws.push((it.to_string(), "文件名「致」段"));
                    }
                }
                if let Some(v) = header_get(&h, "致") {
                    for it in v
                        .split(['、', '，', ',', '及'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        raws.push((it.to_string(), "信封「致」"));
                    }
                }
                for (raw, where_) in raws {
                    let bare = raw.strip_suffix('等').unwrap_or(&raw);
                    if !bare.is_empty() && roster.functions.iter().any(|x| x == bare) {
                        d.errs.push(format!(
                            "c. {where_}裸职能非法：{raw}——收件人必须写 <职能><名字>（如 {bare}空谷）或 <职能>全体（{f}）（⛳ MECH-FLOW-19）"
                        ));
                    }
                }
            }

            // 「复」合法值 = NNNN／<分拣码>NNNN／无（<说明>）
            if let Some(rv) = header_get(&h, "复") {
                let rv_clean: String = rv.chars().filter(|c| !c.is_whitespace()).collect();
                if !rv_clean.starts_with('无') {
                    if !full_no_re.is_match(&rv_clean) {
                        d.errs.push(format!(
                            "a. 信封「复」格式非法：{f} 「{rv}」（须 NNNN／<分拣码>NNNN／无（<说明>））（⛳ MECH-FLOW-15）"
                        ));
                    } else if !known.contains(&rv_clean) {
                        if rv_clean.starts_with(|c: char| c.is_ascii_uppercase()) {
                            d.warns.push(format!(
                                "a. 跨信箱被复信 {rv_clean} 不在本信箱可见范围（{f}）——跨信箱引用写全码，请人工确认"
                            ));
                        } else {
                            d.errs.push(format!(
                                "a. 被复信不存在：{f} 「{rv}」在本信箱两栏中找不到（⛳ MECH-FLOW-15）"
                            ));
                        }
                    }
                }
            }
            if let (Some(re), Some(sv)) = (&status_re, header_get(&h, "状态"))
                && !re.is_match(sv)
            {
                d.errs.push(format!(
                    "a. 状态词出表：{f} 「{}…」前缀不在 README 合法词表（⛳ MECH-FLOW-15）",
                    first_chars(sv, 24)
                ));
            }
            if let Some(sv) = header_get(&h, "状态")
                && !status_is_pending(sv)
                && !has_v21_stamp(sv)
            {
                d.errs.push(format!(
                        "a. 代际戳缺失/格式非法：{f} 「{}…」非待* 状态须带（YYYY-MM-DD HH:MM ±HH:MM 职能@名字 更新：备注）（⛳ MECH-FLOW-17）",
                        first_chars(sv, 26)
                    ));
            }
            if has_placeholder(&plain_body(text)) {
                d.errs.push(format!(
                    "d0. 占位未填：{f} 摘要块仍是占位文本——读者拿不到真话"
                ));
            }
            if !has_plain_block(text) {
                d.errs.push(format!(
                    "d. 缺摘要块：{f} —— v2.1 新信必须有（「## 摘要」；2026-09-29 起旧形态「## 白话结论」已废止）"
                ));
            }
            continue;
        }

        // ========== 存量冻结（旧 ASCII 形态）：v2 原判据 ==========
        d.legacy_count += 1;
        let no = legacy_no_of(f);
        match &no {
            None => {
                d.errs.push(format!(
                    "a. 新纪元信缺编号前缀：{f}（v1 名单外的一律须 NNNN-… 或中文句法 <编号>号<发信人>的<类型>.md；跑生成器签发）"
                ));
            }
            Some(n) => {
                if !legacy_name_ok(f, b.name_prefix) {
                    d.errs.push(format!(
                        "a. 命名违规：{f}（编号后须以 {} 开头、ASCII）（⛳ MECH-FLOW-12）",
                        b.name_prefix
                    ));
                }
                if !legacy_type_ok(f) {
                    d.errs.push(format!(
                        "a. 命名违规：{f} 未以合法类型词结尾（submission/review/response/report/verdict/notice/landing/landing-report）"
                    ));
                }
                if let Some(prev) = seen_no.get(n) {
                    d.errs.push(format!("e. 编号重复：{n}（{prev} 与 {f}）"));
                } else {
                    seen_no.insert(n.clone(), f.to_string());
                }
            }
        }
        check_token_book(&mut d, &ledger, f, text, no.as_deref());

        let head = parse_header(text, &LEGACY_FIELDS);
        for k in ["预期表态方", "收敛判据", "状态"] {
            if let Some(v) = header_get(&head, k)
                && has_placeholder(v)
            {
                d.errs.push(format!(
                    "d0. 占位未填：{f} 机读头「{k}」仍是生成器占位（{}…）——发信前请填实",
                    first_chars(v, 18)
                ));
            }
        }
        if has_placeholder(&plain_body(text)) {
            d.errs.push(format!(
                "d0. 占位未填：{f} 摘要块仍是占位文本——读者拿不到真话"
            ));
        }
        if !has_plain_block(text) {
            d.errs.push(format!(
                "d. 缺摘要块：{f} —— v2 新信必须有（「## 摘要」；2026-09-29 起旧形态「## 白话结论」已废止）"
            ));
        }
    }

    // v2.1 信封日期单调（≥上一封；按编号序——编号数字段序，BAR-179：编号
    // 在 v2.1 是跨分拣码共享的数字流水（next_number 跨前缀统一 max+1），
    // 数字序 = 签发序；字符串序在混排前缀（0018 与 NA0015）下会把后签发的
    // 纯数字信排到 NA 信前造成误判——JS 同款潜伏病，主册前缀划一从未踩到）
    {
        let no_key = |no: &str| {
            let digits: String = no.chars().skip_while(|c| c.is_ascii_uppercase()).collect();
            (digits.parse::<u64>().unwrap_or(u64::MAX), no.to_string())
        };
        let mut seq: Vec<&(String, String, f64)> =
            v21_times.iter().filter(|t| !t.2.is_nan()).collect();
        seq.sort_by_key(|t| no_key(&t.0));
        for w in seq.windows(2) {
            if w[1].2 < w[0].2 {
                d.errs.push(format!(
                    "a. 信封日期非单调：{} 的日期早于上一封 {}（⛳ MECH-FLOW-15）",
                    w[1].1, w[0].1
                ));
            }
        }
    }

    // e. 孤儿票据（台账有、文件无）——位置认三栏：在册 / archive-v1 /
    // archive-withdrawn（契约 §八 第 9 条：撤回件的 file 指向撤回栏，判据不认
    // 这个位置就会误报「file 不存在」）。反向也钉死：现行票的 file 落在撤回栏
    // ＝「移了档没撤票」的半状态，照红（撤回缺第 4 步）
    let active_files: HashSet<&str> = b
        .letters
        .iter()
        .filter(|l| l.dir == "active")
        .map(|l| l.file.as_str())
        .collect();
    let archive_files: HashSet<&str> = b
        .letters
        .iter()
        .filter(|l| l.dir == "archive-v1")
        .map(|l| l.file.as_str())
        .collect();
    let withdrawn_files: HashSet<&str> = b
        .letters
        .iter()
        .filter(|l| l.dir == "withdrawn")
        .map(|l| l.file.as_str())
        .collect();
    let book_has = |name: &str| {
        !name.is_empty()
            && (active_files.contains(name)
                || archive_files.contains(name)
                || withdrawn_files.contains(name))
    };
    // 撤回栏防重完整性（check-agent-inbox 同制）：同一封信不得同时在撤回栏与
    // 在册/归档栏（撤回用 git mv，一件只落一处）
    for f in &withdrawn_files {
        if active_files.contains(f) || archive_files.contains(f) {
            d.errs.push(format!(
                "台账完整性：{f} 同时在撤回栏（archive-withdrawn/）与在册/归档栏出现——撤回用 git mv，不复制"
            ));
        }
    }
    for t in &ledger.current {
        let tf = t.file.as_deref().unwrap_or("");
        if active_files.contains(tf) || archive_files.contains(tf) {
            continue;
        }
        if withdrawn_files.contains(tf) {
            d.errs.push(format!(
                "e. 半状态：no={} 现行票未撤，但 file={tf} 已在 archive-withdrawn/——撤回缺第 4 步（给该票记 revokedAt＋revokeReason，契约 §八 第 9 条④）",
                t.no
            ));
        } else {
            d.errs.push(format!(
                "e. 孤儿票据：no={} 登记的 file={tf} 在本册三栏（在册/archive-v1/archive-withdrawn）均不存在（改名未换票？）",
                t.no
            ));
        }
    }
    // 两种「带 revokedAt 的票」靠配对关系判别（契约 §八 第 7 条 vs 第 9 条，
    // 不靠新字段）：有 revokedAt 且被某张票以 renamedFrom 引用 → 撤销票（换票的
    // 一半，要求配对）；无人引用 → 撤回票（整封作废，放行换票判据，但文件须在册
    // ——「删文件」正是撤回要躲开的病）。配对键 = renamedFrom 文件名本身
    // （BAR-180：§八.8 换票同步去码改号，比 no 必假失配）。引用方取**全部票**
    // （含撤销票），非只取现行票——换票可多跳成链（主册 0003/0014 各是迁移→归位
    // 两跳，链首票的「新票」自己后来也成了撤销票）；只认现行票会把链首票误判成
    // 撤回票，再报成「file 不存在」——假红，主册实测
    let paired_by: HashSet<&str> = ledger
        .current
        .iter()
        .chain(ledger.revoked.iter())
        .filter_map(|r| r.renamed_from.as_deref())
        .filter(|s| !s.is_empty())
        .collect();
    for t in &ledger.current {
        let Some(rf) = &t.renamed_from else { continue };
        let paired = ledger
            .revoked
            .iter()
            .any(|r| r.file.as_deref() == Some(rf.as_str()));
        if !paired {
            d.warns.push(format!(
                "c. 换票留痕不完整：no={} 现行票记 renamedFrom={rf}，但台账找不到该旧票的 revokedAt 撤销记录（契约 §八）",
                t.no
            ));
        }
    }
    let mut withdrawn_tickets = 0usize;
    for r in &ledger.revoked {
        if paired_by.contains(r.file.as_deref().unwrap_or("")) {
            continue; // 撤销票：换票的一半，文件已改名离场，另判
        }
        withdrawn_tickets += 1;
        let tf = r.file.as_deref().unwrap_or("");
        if !book_has(tf) {
            d.errs.push(format!(
                "e. 孤儿票据（撤回票）：no={} 登记的 file={tf} 在本册三栏均不存在——撤回只移档不删（契约 §八 第 9 条①）",
                r.no
            ));
        }
    }

    d.current_tickets = ledger.current.len();
    d.revoked_tickets = ledger.revoked.len();
    d.withdrawn_tickets = withdrawn_tickets;
    d.replaced_tickets = ledger.revoked.len() - withdrawn_tickets;
    d
}

// ---------------------------------------------------------------
// 单信模式（new-letter.mjs --verify）
// ---------------------------------------------------------------

/// 单信模式：按纪元自动选规则；不查跨信判据（编号唯一/日期单调/孤儿票/复存在性）
pub fn verify_single(
    file: &str,
    text: &str,
    tokens_text: Option<&str>,
    roster: Option<&Roster>,
    strict: bool,
) -> Diags {
    let mut d = Diags::default();
    let ledger = tokens_text.map(parse_ledger);

    let check_token = |d: &mut Diags, expect_no: Option<&str>| {
        let Some(tm) = token::find_token(text) else {
            d.errs
                .push("缺令牌行（LETTER-TOKEN v2 …）——本信非生成器签发".to_string());
            return;
        };
        if let Some(e) = expect_no
            && tm.no != e
        {
            d.errs
                .push(format!("编号不一致：文件名 {e} vs 令牌 {}", tm.no));
        }
        let rec = ledger.as_ref().and_then(|l| l.find_current(&tm.no));
        let Some(rec) = rec else {
            d.errs.push(format!(
                "令牌 no={} 不在本信箱台账 letter-tokens.jsonl（未消费/伪造）",
                tm.no
            ));
            return;
        };
        if rec.file.as_deref() != Some(file) {
            d.errs.push(format!(
                "台账登记的 file={} 与本文件 {file} 不一致",
                rec.file.as_deref().unwrap_or("")
            ));
        }
        if rec.nonce.as_deref() != Some(tm.nonce.as_str()) {
            d.errs.push("令牌 nonce 与台账不一致".to_string());
        }
        if rec.fp.as_deref() != Some(tm.fp.as_str()) {
            d.errs.push("令牌 fp 与台账不一致".to_string());
        }
        if fingerprint(&tm.no, &tm.nonce, file) != tm.fp {
            d.errs
                .push("令牌指纹重算不符（编号/nonce/文件名被改过）".to_string());
        }
    };

    if is_v21_name(file) {
        // ---- v2.1 新纪元 ----
        let p = parse_v21_name(file);
        d.errs.extend(p.errs.iter().cloned());
        let full_no =
            p.no.as_ref()
                .map(|no| format!("{}{}", p.sorting.as_deref().unwrap_or(""), no));
        check_token(&mut d, full_no.as_deref());
        let h = parse_header(text, &V21_FIELDS);
        let miss: Vec<&str> = ["日期", "从", "致", "复", "状态"]
            .into_iter()
            .filter(|k| header_get(&h, k).is_none())
            .collect();
        if !miss.is_empty() {
            d.errs.push(format!(
                "信封缺字段：{}（v2.1 必填 日期/从/致/复/状态；编号可省、以文件名为准）",
                miss.join("、")
            ));
        }
        if let (Some(hv), Some(n)) = (header_get(&h, "编号"), &full_no) {
            let hv_clean: String = hv.chars().filter(|c| !c.is_whitespace()).collect();
            if hv_clean != *n {
                d.errs
                    .push(format!("信封「编号」与文件名不一致：{hv} vs {n}"));
            }
        }
        if let Some(dv) = header_get(&h, "日期")
            && !is_valid_v21_date(dv)
        {
            d.errs.push(format!(
                "信封「日期」格式非法：{dv}（须 YYYY-MM-DD HH:MM ±HH:MM）"
            ));
        }
        if let Some(sv) = header_get(&h, "状态")
            && !sv.starts_with('待')
            && !has_v21_stamp(sv)
        {
            d.errs.push(format!(
                "信封「状态」非待* 时须带代际戳（YYYY-MM-DD HH:MM ±HH:MM 职能@名字 更新：…）：{sv}"
            ));
        }
        let mut entries = vec![];
        let from_val = header_get(&h, "从").unwrap_or("");
        if from_val.chars().count() < 3 {
            d.errs.push(format!(
                "信封「从」非法：{from_val}（须 <职能><名字>，如 运维部蔚然）"
            ));
        } else {
            entries.push(PoolEntry {
                who: "发信人".to_string(),
                func: Some(drop_last_chars(from_val, 2)),
                name: last_chars(from_val, 2),
            });
        }
        if let Some(pn) = &p.from_name
            && !from_val.is_empty()
            && last_chars(from_val, 2) != *pn
        {
            d.errs.push(format!(
                "信封「从」名字 {} 与文件名发信人 {pn} 不一致",
                last_chars(from_val, 2)
            ));
        }
        let to_items = parse_to_field(header_get(&h, "致").unwrap_or(""), &mut d.errs);
        if to_items.is_empty() {
            d.errs.push("信封「致」为空".to_string());
        }
        for it in &to_items {
            entries.push(PoolEntry {
                who: "收件人".to_string(),
                func: it.func.clone(),
                name: it.name.clone(),
            });
        }
        check_pools(roster, &entries, strict, &mut d.errs, &mut d.warns);
        if let Some(rv) = header_get(&h, "复") {
            let rv_clean: String = rv.chars().filter(|c| !c.is_whitespace()).collect();
            if !rv_clean.starts_with('无')
                && !regex::Regex::new(r"^[A-Z]{0,4}\d{4}$")
                    .unwrap()
                    .is_match(&rv_clean)
            {
                d.errs.push(format!(
                    "信封「复」格式非法：{rv}（须 NNNN／<分拣码>NNNN／无（<说明>））"
                ));
            }
        }
    } else {
        // ---- 存量冻结（旧 ASCII 形态） ----
        let nm = regex::Regex::new(r"^(\d{4})-(kfm-na|kfmv4)-.*\.md$")
            .unwrap()
            .captures(file)
            .map(|m| m[1].to_string());
        if nm.is_none() {
            d.errs.push(format!(
                "文件名不合 v2 命名：{file}（须 NNNN-kfm-na|kfmv4-…-<类型词>.md）"
            ));
        }
        check_token(&mut d, nm.as_deref());
        for k in ["预期表态方", "收敛判据", "状态"] {
            let re = regex::Regex::new(&format!(r"(?m)^>\s*{k}\s*[:：]\s*(.+)$")).unwrap();
            if let Some(m) = re.captures(text)
                && has_placeholder(&m[1])
            {
                d.errs.push(format!(
                    "机读头「{k}」仍是占位（{}…）——发信前填实",
                    first_chars(&m[1], 18)
                ));
            }
        }
    }

    if !has_plain_block(text) {
        d.errs
            .push("缺摘要块（`## 摘要`）（机读头/令牌之后、证据之前必须有；2026-09-29 起旧形态 `## 白话结论` 已废止）".to_string());
    }
    if has_placeholder(&plain_body(text)) {
        d.errs.push("摘要块仍是占位文本".to_string());
    }
    d
}

/// 欠账判定 re-export（scan/projection 共用）
pub use token::sha256_hex16 as content_hash16;
