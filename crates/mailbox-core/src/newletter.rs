//! new 生成件：文件名拼接 / 信纸骨架 / 令牌插入 / 编号分配 / 台账行
//! （移植 new-letter.mjs v2.1 模式，逐字节对齐）。

use crate::json::{JVal, jobj, jstr, to_json_string};
use crate::verify::PLAIN_HEAD;

/// 文件名：`[分拣码]NNNN号<发信人>致<seg>[复X][关于Y]的<类型>.md`
/// （seg = display 全列「及」连接；>2 条前二 + 等）
pub fn build_v21_file_name(
    no: &str,
    sorting: &str,
    from_name: &str,
    display: &[String],
    reply: Option<&str>,
    about: Option<&str>,
    type_word: &str,
) -> String {
    let seg = if display.len() > 2 {
        format!("{}等", display[..2].join("及"))
    } else {
        display.join("及")
    };
    let reply_seg = reply.map(|r| format!("复{r}")).unwrap_or_default();
    let about_seg = about.map(|a| format!("关于{a}")).unwrap_or_default();
    format!("{sorting}{no}号{from_name}致{seg}{reply_seg}{about_seg}的{type_word}.md")
}

pub struct SkeletonParams<'a> {
    pub title: &'a str,
    /// 本地戳 `YYYY-MM-DD HH:MM ±HH:MM`（CLI 层注入）
    pub date_stamp: &'a str,
    pub from_func: &'a str,
    pub from_name: &'a str,
    pub display: &'a [String],
    pub reply: Option<&'a str>,
    pub status: &'a str,
    pub kind: &'a str,
    pub expect: &'a str,
    pub criteria: &'a str,
}

/// 信纸骨架（`\n\n\n` 令牌插入位在信封之后；末元素空串 → 结尾单换行）
pub fn build_v21_skeleton(p: &SkeletonParams) -> String {
    let lines = [
        format!("# {}", p.title),
        String::new(),
        format!("> 日期: {}", p.date_stamp),
        format!("> 从: {}{}", p.from_func, p.from_name),
        format!("> 致: {}", p.display.join("、")),
        format!("> 复: {}", p.reply.unwrap_or("无（首信）")),
        format!("> 状态: {}", p.status),
        String::new(),
        String::new(), // 令牌行插入位（发令牌后 \n\n\n 替换填入）
        String::new(),
        PLAIN_HEAD.to_string(),
        String::new(),
        "（待填：面向隐藏读者。三句话内说清是什么事、要不要他做事；不写工作术语。）".to_string(),
        String::new(),
        "### 要办与算完".to_string(),
        String::new(),
        format!("- 类别：{}", p.kind),
        format!("- 要办：{}", p.expect),
        format!("- 算完：{}", p.criteria),
        String::new(),
        "## 正文".to_string(),
        String::new(),
        "（待填：结论与推导一起写——精确留给承重的数字和名字，其余白话；每条发现标「明写／推断／补全」。）"
            .to_string(),
        String::new(),
    ];
    lines.join("\n")
}

/// 把第一个 `\n\n\n` 替换为 `\n<token行>\n`
pub fn insert_token(text: &str, token_line: &str) -> String {
    text.replacen("\n\n\n", &format!("\n{token_line}\n"), 1)
}

/// 编号分配：文件名（两目录合集）+ 台账编号（含撤销票）取最大流水 +1，4 位零填充
pub fn next_number(files: &[String], ledger_nos: &[&str]) -> String {
    let mut max = 0u32;
    let file_re = regex::Regex::new(r"^[A-Z]{0,4}(\d{4})(?:号|-)").unwrap();
    for f in files {
        if let Some(m) = file_re.captures(f) {
            max = max.max(m[1].parse().unwrap_or(0));
        }
    }
    let no_re = regex::Regex::new(r"(\d{4})$").unwrap();
    for no in ledger_nos {
        if let Some(m) = no_re.captures(no) {
            max = max.max(m[1].parse().unwrap_or(0));
        }
    }
    format!("{:04}", max + 1)
}

/// 台账行 JSON（key 序 no,file,nonce,fp,tpl,createdAt,from[,renamedFrom]）
pub fn ledger_record_line(
    no: &str,
    file: &str,
    nonce: &str,
    fp: &str,
    created_at: &str,
    from: &str,
    renamed_from: Option<&str>,
) -> String {
    let mut pairs = vec![
        ("no", jstr(no)),
        ("file", jstr(file)),
        ("nonce", jstr(nonce)),
        ("fp", jstr(fp)),
        ("tpl", jstr("v2")),
        ("createdAt", jstr(created_at)),
        ("from", jstr(from)),
    ];
    if let Some(rf) = renamed_from {
        pairs.push(("renamedFrom", jstr(rf)));
    }
    to_json_string(&jobj(pairs))
}

/// 换票撤销字段补写：给既有台账行（JVal 保序）追加 revokedAt/revokeReason
pub fn revoke_fields(revoked_at: &str, reason: &str) -> Vec<(String, JVal)> {
    vec![
        ("revokedAt".to_string(), jstr(revoked_at)),
        ("revokeReason".to_string(), jstr(reason)),
    ]
}
