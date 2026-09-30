//! 名册（roster.json）解析与三池校验（移植 JS checkPools/splitFunc）。

use crate::json::{JVal, parse_json};
use crate::name::{has_connect_char, is_han_str};

#[derive(Debug, Default, Clone)]
pub struct NameRec {
    /// None = 名册未登记 functions 键（JS Array.isArray 判据 → 跳过组合校验）
    pub functions: Option<Vec<String>>,
    pub project: Option<String>,
    /// 名册 primary 职能（撤回留痕署名取它，不取操作者自报——契约 §八 第 9 条③）
    pub primary: Option<String>,
}

#[derive(Debug, Default)]
pub struct Roster {
    pub projects: Vec<String>,
    /// 职能池（保序 = JSON 文本序）
    pub functions: Vec<String>,
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
        if let Some(obj) = v.get("names").and_then(JVal::as_obj) {
            for (k, nv) in obj {
                let functions = nv.get("functions").and_then(JVal::as_arr).map(|a| {
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

/// 池与名册校验：职能出池 = 硬错；名字未登记 = 硬错（strict；否则降级警告）；
/// 组合不在名册 = 硬错。roster 缺失 = 整体降级为仅文法校验（警告）。
pub fn check_pools(
    roster: Option<&Roster>,
    entries: &[PoolEntry],
    strict: bool,
    errs: &mut Vec<String>,
    warns: &mut Vec<String>,
) {
    let Some(roster) = roster else {
        warns.push(
            "roster.json 缺失——三池校验退化为仅文法校验（建 /root/90-信箱/00-主册/roster.json 或用 --roster 指定）"
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
