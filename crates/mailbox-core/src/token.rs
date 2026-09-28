//! 令牌：fp 计算 / 令牌行 / 台账解析（移植 JS fingerprint/issueToken/台账段）。

use crate::json::{JVal, parse_json};
use sha2::Digest;

pub fn sha256_hex16(s: &str) -> String {
    let digest = sha2::Sha256::digest(s.as_bytes());
    let mut hex = String::with_capacity(16);
    for b in &digest[..8] {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// fp = sha256("{no}|{nonce}|{file}|v2") 取 hex 前 16
pub fn fingerprint(no: &str, nonce: &str, file: &str) -> String {
    sha256_hex16(&format!("{no}|{nonce}|{file}|v2"))
}

pub fn token_line(no: &str, nonce: &str, fp: &str) -> String {
    format!("<!-- LETTER-TOKEN v2 no={no} nonce={nonce} fp={fp} -->")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenHit {
    pub no: String,
    pub nonce: String,
    pub fp: String,
}

/// 在全文里找令牌行 `<!-- LETTER-TOKEN v2 no=… nonce=… fp=… -->`
pub fn find_token(text: &str) -> Option<TokenHit> {
    let re = regex::Regex::new(
        r"<!-- LETTER-TOKEN v2 no=([A-Z]{0,4}\d{4}) nonce=([0-9a-f]{16}) fp=([0-9a-f]{16}) -->",
    )
    .unwrap();
    let m = re.captures(text)?;
    Some(TokenHit {
        no: m[1].to_string(),
        nonce: m[2].to_string(),
        fp: m[3].to_string(),
    })
}

#[derive(Debug, Clone)]
pub struct Ticket {
    pub no: String,
    pub file: Option<String>,
    pub nonce: Option<String>,
    pub fp: Option<String>,
    pub revoked_at: Option<String>,
    pub renamed_from: Option<String>,
}

impl Ticket {
    fn from_jval(v: &JVal) -> Ticket {
        let g = |k: &str| v.get(k).and_then(JVal::as_str).map(str::to_string);
        Ticket {
            no: g("no").unwrap_or_default(),
            file: g("file"),
            nonce: g("nonce"),
            fp: g("fp"),
            revoked_at: g("revokedAt"),
            renamed_from: g("renamedFrom"),
        }
    }
}

#[derive(Debug, Default)]
pub struct Ledger {
    /// 现行票（每编号唯一；重复记 errs 并以后者覆盖，与 JS Map.set 同款）
    pub current: Vec<Ticket>,
    /// 撤销票（留痕，不参与现行判据）
    pub revoked: Vec<Ticket>,
    pub errs: Vec<String>,
    pub warns: Vec<String>,
}

impl Ledger {
    pub fn find_current(&self, no: &str) -> Option<&Ticket> {
        self.current.iter().find(|t| t.no == no)
    }
}

fn no_ok(no: &str) -> bool {
    regex::Regex::new(r"^[A-Z]{0,4}\d{4}$")
        .unwrap()
        .is_match(no)
}

/// 台账 jsonl 解析（坏行/非法编号记 errs；撤销票分流、缺 file 记 warns）
pub fn parse_ledger(text: &str) -> Ledger {
    let mut led = Ledger::default();
    for (i, line) in text.split('\n').enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let n = i + 1;
        let Ok(v) = parse_json(line) else {
            led.errs
                .push(format!("letter-tokens.jsonl 第 {n} 行不是合法 JSON"));
            continue;
        };
        let t = Ticket::from_jval(&v);
        if !no_ok(&t.no) {
            led.errs.push(format!("台账第 {n} 行编号非法：{}", t.no));
        }
        if t.revoked_at.is_some() {
            if t.file.is_none() {
                led.warns.push(format!(
                    "台账第 {n} 行撤销票缺 file（换票留痕不完整）：no={}",
                    t.no
                ));
            }
            led.revoked.push(t);
            continue;
        }
        if led.current.iter().any(|x| x.no == t.no) {
            led.errs.push(format!(
                "台账编号重复：{}（同一编号只许一张现行票据；换票请给旧票记 revokedAt）",
                t.no
            ));
            led.current.retain(|x| x.no != t.no);
        }
        led.current.push(t);
    }
    led
}

/// 台账全部编号（含撤销票）——next_number 的台账侧输入
pub fn ledger_nos(text: &str) -> Vec<String> {
    text.split('\n')
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| parse_json(l).ok())
        .filter_map(|v| v.get("no").and_then(JVal::as_str).map(str::to_string))
        .collect()
}
