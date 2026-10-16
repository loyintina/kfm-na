//! 名册（roster.json）解析与三池校验（移植 JS checkPools/splitFunc）。

use crate::json::{JVal, parse_json};
use crate::name::{has_connect_char, is_han_str};

#[derive(Debug, Default, Clone)]
pub struct NameRec {
    /// None = 名册未登记 functions 键（JS Array.isArray 判据 → 跳过组合校验）
    pub functions: Option<Vec<String>>,
    /// 旧职能名白名单（2026-10-15 改「两部一委」：存量信用旧名可寻址、黄提示不拦；0166 §一）
    pub legacy_functions: Option<Vec<String>>,
    pub project: Option<String>,
    /// 名册 primary 职能（撤回留痕署名取它，不取操作者自报——契约 §八 第 9 条③）
    pub primary: Option<String>,
}

#[derive(Debug, Default)]
pub struct Roster {
    pub projects: Vec<String>,
    /// 职能池（保序 = JSON 文本序）
    pub functions: Vec<String>,
    /// 存量边界（ISO/信封式皆可）：信日期早于此值 ⇒ 可用旧职能名（0166 §一）
    pub legacy_functions_until: Option<String>,
    /// 名字池（保序）
    pub names: Vec<(String, NameRec)>,
}

impl Roster {
    pub fn from_json_str(s: &str) -> Option<Roster> {
        parse_json(s).ok().map(|v| Roster::from_jval(&v))
    }
    pub fn from_jval(v: &JVal) -> Roster {
        let mut r = Roster::default();
        if let Some(arr) = v.get("projects").and_then(JVal::as_arr) {
            r.projects = arr
                .iter()
                .filter_map(JVal::as_str)
                .map(str::to_string)
                .collect();
        }
        if let Some(obj) = v.get("functions").and_then(JVal::as_obj) {
            r.functions = obj.iter().map(|(k, _)| k.clone()).collect();
        }
        r.legacy_functions_until = v
            .get("legacy_functions_until")
            .and_then(JVal::as_str)
            .map(str::to_string);
        if let Some(obj) = v.get("names").and_then(JVal::as_obj) {
            for (k, nv) in obj {
                let functions = nv.get("functions").and_then(JVal::as_arr).map(|a| {
                    a.iter()
                        .filter_map(JVal::as_str)
                        .map(str::to_string)
                        .collect()
                });
                let legacy_functions = nv.get("legacy_functions").and_then(JVal::as_arr).map(|a| {
                    a.iter()
                        .filter_map(JVal::as_str)
                        .map(str::to_string)
                        .collect()
                });
                let project = nv.get("project").and_then(JVal::as_str).map(str::to_string);
                let primary = nv.get("primary").and_then(JVal::as_str).map(str::to_string);
                r.names.push((
                    k.clone(),
                    NameRec {
                        functions,
                        legacy_functions,
                        project,
                        primary,
                    },
                ));
            }
        }
        r
    }
    pub fn name_rec(&self, name: &str) -> Option<&NameRec> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, r)| r)
    }
    /// 名字 → 署名职能（JS funcOfName 同款）：名册 primary > functions[0] > fallback
    /// （代撤的撤回留痕署名取名册，不取操作者自报——契约 §八 第 9 条③）
    pub fn func_of_name(&self, name: &str, fallback: &str) -> String {
        let Some(rec) = self.name_rec(name) else {
            return fallback.to_string();
        };
        rec.primary
            .clone()
            .or_else(|| rec.functions.as_ref().and_then(|f| f.first().cloned()))
            .unwrap_or_else(|| fallback.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct PoolEntry {
    pub who: String,
    pub func: Option<String>,
    pub name: String,
}

/// 项目-职能 复合限定拆分（splitFunc）：raw 含 '-' 且前缀 ∈ 项目池 → (项目, 职能)
pub fn split_func<'a>(roster: &Roster, raw: &'a str) -> (Option<&'a str>, &'a str) {
    if let Some(i) = raw.find('-') {
        let p = &raw[..i];
        if roster.projects.iter().any(|x| x == p) {
            return (Some(p), &raw[i + 1..]);
        }
    }
    (None, raw)
}

// ---------------------------------------------------------------
// BAR-254 信头人名两级切分（MAIN0173 白露楼2/楼4 判据 + 楼6 加固：
// 服务端预切、名册绝不下推手机——切分唯一出处 = 本函数 × roster.json）
// ---------------------------------------------------------------

/// 切分项类别：规范人名／广播／无法解析
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispKind {
    Person,
    Broadcast,
    Unknown,
}

/// 一个切分项（致字段的一项；从字段恒单项）。
/// person：func＝职能（及式继承＝前项职能）、name＝二字名；
/// broadcast/unknown：func＝None、name＝原样整项（不猜不截）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispItem {
    pub kind: DispKind,
    pub func: Option<String>,
    pub name: String,
    /// 及式裸名继承前项职能（白露楼4：解析层必须归属，否则欠账算错人）
    pub inherited: bool,
}

/// 一级分隔符全集（MAIN0173 楼2：「、」与「及」以及契约里的「等」；
/// 、的半全角族 ，,；; 同 BAR-214 split_recipients 旧约）
const SEPARATORS: [char; 7] = ['、', '，', ',', '；', ';', '及', '等'];

/// 职能池最长前缀匹配（池 = roster.functions 键集；键集天然含 legacy
/// 评审/研究/开发部等存量旧名——MAIN0173 §三 样本③的依靠）
fn longest_func<'a>(roster: &Roster, item: &'a str) -> Option<&'a str> {
    let mut best: Option<&str> = None;
    for f in &roster.functions {
        if let Some(rest) = item.strip_prefix(f.as_str())
            && !rest.is_empty()
            && best.is_none_or(|b| f.len() > b.len())
        {
            best = Some(&item[..f.len()]);
        }
    }
    best
}

fn is_pool_name(roster: &Roster, s: &str) -> bool {
    s.chars().count() == 2 && roster.names.iter().any(|(n, _)| n == s)
}

/// 两级切分：field（从/致/抄字段原串）→ 切分项表。
/// 一级按 SEPARATORS 分项（空项滤除）；二级项内：
/// ①整项「全体」或「职能+全体」= 广播，原样不切；
/// ②职能池最长前缀命中 + 余恰两字 ∈ 名字池 = person；
/// ③及式（前导分隔符 = 及）裸名 ∈ 名字池且前项是 person ⇒ 继承前项职能；
/// ④其余 = unknown，原样不猜不截。
/// roster = None（名册读不到）= 空池：逐项原样照抄，一项不死
/// （白露楼6 加固1：新旧不匹配时不许显示空白）。
pub fn split_display_items(roster: Option<&Roster>, field: &str) -> Vec<DispItem> {
    let mut out: Vec<DispItem> = Vec::new();
    let mut seg = String::new();
    let mut sep_before: Option<char> = None;
    for ch in field.chars() {
        if SEPARATORS.contains(&ch) {
            let item = std::mem::take(&mut seg);
            let item = item.trim();
            if !item.is_empty() {
                let classified = classify_item(roster, item, sep_before, out.last());
                out.push(classified);
            }
            sep_before = Some(ch);
        } else {
            seg.push(ch);
        }
    }
    let item = seg.trim();
    if !item.is_empty() {
        let classified = classify_item(roster, item, sep_before, out.last());
        out.push(classified);
    }
    out
}

fn classify_item(
    roster: Option<&Roster>,
    item: &str,
    sep_before: Option<char>,
    prev: Option<&DispItem>,
) -> DispItem {
    let unknown = || DispItem {
        kind: DispKind::Unknown,
        func: None,
        name: item.to_string(),
        inherited: false,
    };
    let broadcast = || DispItem {
        kind: DispKind::Broadcast,
        func: None,
        name: item.to_string(),
        inherited: false,
    };
    if item == "全体" {
        return broadcast();
    }
    let Some(roster) = roster else {
        return unknown();
    };
    if let Some(func) = longest_func(roster, item) {
        let rest = &item[func.len()..];
        if rest == "全体" {
            return broadcast();
        }
        if is_pool_name(roster, rest) {
            return DispItem {
                kind: DispKind::Person,
                func: Some(func.to_string()),
                name: rest.to_string(),
                inherited: false,
            };
        }
        return unknown();
    }
    // 及式裸名继承前项职能（白露楼4：显示层可省职能，解析层必须归属）
    if sep_before == Some('及')
        && is_pool_name(roster, item)
        && let Some(p) = prev
        && p.kind == DispKind::Person
    {
        return DispItem {
            kind: DispKind::Person,
            func: p.func.clone(),
            name: item.to_string(),
            inherited: true,
        };
    }
    unknown()
}

/// 显示串渲染（from_disp/to_disp 两键的生成规则）：person 取名、
/// broadcast/unknown 取原样整项，「、」相连
pub fn render_display(items: &[DispItem]) -> String {
    items
        .iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join("、")
}

/// 池与名册校验：职能出池 = 硬错；名字未登记 = 硬错（strict；否则降级警告）；
/// 组合不在名册 = 硬错。roster 缺失 = 整体降级为仅文法校验（警告）。
pub fn check_pools(
    roster: Option<&Roster>,
    entries: &[PoolEntry],
    strict: bool,
    errs: &mut Vec<String>,
    warns: &mut Vec<String>,
) {
    check_pools_with_legacy(roster, entries, strict, errs, warns, false)
}

/// 同 check_pools，但允许"存量旧职能名"（0166 §一）：`legacy_allowed` 由调用方按
/// 「信日期 < roster.legacy_functions_until」算出；命中 legacy_functions ⇒ 黄提示、不拦。
pub fn check_pools_with_legacy(
    roster: Option<&Roster>,
    entries: &[PoolEntry],
    strict: bool,
    errs: &mut Vec<String>,
    warns: &mut Vec<String>,
    legacy_allowed: bool,
) {
    let Some(roster) = roster else {
        warns.push(
            "roster.json 缺失——三池校验退化为仅文法校验（建主册 roster.json，路径见 --main-book / KFM_MAIN_BOOK；或用 --roster 指定）"
                .to_string(),
        );
        return;
    };
    let funcs_joined = roster.functions.join("/");
    let names_joined = if roster.names.is_empty() {
        "（空）".to_string()
    } else {
        roster
            .names
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join("/")
    };
    for e in entries {
        if e.name == "全体" {
            continue;
        }
        let fn_ = match &e.func {
            Some(raw) => {
                let (_, fn_) = split_func(roster, raw);
                if !roster.functions.iter().any(|f| f == fn_) {
                    errs.push(format!(
                        "{}职能「{fn_}」不在职能池（{funcs_joined}）——裸职能或职能名非法（职能名须带「部」后缀）",
                        e.who
                    ));
                }
                Some(fn_)
            }
            None => {
                errs.push(format!(
                    "{}缺职能串（须 <职能><名字>，如 研究部空谷）",
                    e.who
                ));
                None
            }
        };
        if e.name.chars().count() != 2 || !is_han_str(&e.name) {
            errs.push(format!("{}名字「{}」须恰好两个汉字", e.who, e.name));
            continue;
        }
        if has_connect_char(&e.name) {
            errs.push(format!("{}名字「{}」含连接字", e.who, e.name));
            continue;
        }
        match roster.name_rec(&e.name) {
            None => {
                let msg = format!(
                    "{}名字「{}」不在名字池（池中现有 {names_joined}；池唯一出处 = roster.json）",
                    e.who, e.name
                );
                if strict {
                    errs.push(msg)
                } else {
                    warns.push(msg)
                }
            }
            Some(rec) => {
                if let (Some(fn_), Some(fns)) = (fn_, &rec.functions)
                    && !fns.iter().any(|f| f == fn_)
                {
                    let is_legacy = rec
                        .legacy_functions
                        .as_ref()
                        .is_some_and(|l| l.iter().any(|f| f == fn_));
                    if legacy_allowed && is_legacy {
                        warns.push(format!(
                            "{}「{}{}」用的是旧职能名——按名册 legacy_functions（2026-10-15 改「两部一委」前的存量口径）豁免，不拦；新信请用新职能名",
                            e.who, fn_, e.name
                        ));
                    } else {
                        errs.push(format!(
                            "{}「{}{}」组合不在名册内——{} 登记职能为 {}",
                            e.who,
                            fn_,
                            e.name,
                            e.name,
                            fns.join("/")
                        ));
                    }
                }
            }
        }
    }
}
