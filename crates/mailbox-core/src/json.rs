//! 迷你 JSON：保序解析（对象键序 = 文本序）+ JSON.stringify 同款序列化。
//! 台账行/索引行的 key 顺序咬 JS 对象字面量，靠这层保证。

#[derive(Debug, Clone, PartialEq)]
pub enum JVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JVal>),
    Obj(Vec<(String, JVal)>),
}

impl JVal {
    pub fn get(&self, key: &str) -> Option<&JVal> {
        match self {
            JVal::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JVal::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&[JVal]> {
        match self {
            JVal::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&[(String, JVal)]> {
        match self {
            JVal::Obj(o) => Some(o),
            _ => None,
        }
    }
}

pub fn jobj(pairs: Vec<(&str, JVal)>) -> JVal {
    JVal::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn jstr(s: &str) -> JVal {
    JVal::Str(s.to_string())
}

pub fn jopt_str(v: Option<&str>) -> JVal {
    match v {
        Some(s) => JVal::Str(s.to_string()),
        None => JVal::Null,
    }
}

// ---------------------------------------------------------------
// 解析
// ---------------------------------------------------------------

pub fn parse_json(s: &str) -> Result<JVal, String> {
    let mut p = Parser {
        chars: s.chars().collect(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(format!("JSON 尾部多余字符（位置 {}）", p.pos));
    }
    Ok(v)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }
    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("JSON 期望 '{c}'（位置 {}）", self.pos))
        }
    }
    fn value(&mut self) -> Result<JVal, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(JVal::Str(self.string()?)),
            Some('t') => self.lit("true", JVal::Bool(true)),
            Some('f') => self.lit("false", JVal::Bool(false)),
            Some('n') => self.lit("null", JVal::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            _ => Err(format!("JSON 非法值（位置 {}）", self.pos)),
        }
    }
    fn lit(&mut self, word: &str, v: JVal) -> Result<JVal, String> {
        for c in word.chars() {
            if self.peek() != Some(c) {
                return Err(format!("JSON 非法字面量（位置 {}）", self.pos));
            }
            self.pos += 1;
        }
        Ok(v)
    }
    fn object(&mut self) -> Result<JVal, String> {
        self.expect('{')?;
        let mut pairs = vec![];
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(JVal::Obj(pairs));
        }
        loop {
            self.skip_ws();
            let k = self.string()?;
            self.skip_ws();
            self.expect(':')?;
            let v = self.value()?;
            pairs.push((k, v));
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(JVal::Obj(pairs));
                }
                _ => return Err(format!("JSON 对象缺 ',' 或 '}}'（位置 {}）", self.pos)),
            }
        }
    }
    fn array(&mut self) -> Result<JVal, String> {
        self.expect('[')?;
        let mut items = vec![];
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(JVal::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(JVal::Arr(items));
                }
                _ => return Err(format!("JSON 数组缺 ',' 或 ']'（位置 {}）", self.pos)),
            }
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("JSON 字符串未闭合".into()),
                Some('"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    self.pos += 1;
                    let esc = self.peek().ok_or("JSON 转义截断")?;
                    self.pos += 1;
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                // 高代理：尝试低代理配对
                                if self.peek() == Some('\\') {
                                    self.pos += 1;
                                    if self.peek() == Some('u') {
                                        self.pos += 1;
                                        let lo = self.hex4()?;
                                        0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                    } else {
                                        return Err("JSON 代理对截断".into());
                                    }
                                } else {
                                    return Err("JSON 孤代理".into());
                                }
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp).ok_or("JSON 非法码点")?);
                        }
                        _ => return Err(format!("JSON 非法转义 \\{esc}")),
                    }
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.peek().ok_or("JSON \\u 截断")?;
            let d = c.to_digit(16).ok_or("JSON \\u 非 hex")?;
            v = v * 16 + d;
            self.pos += 1;
        }
        Ok(v)
    }
    fn number(&mut self) -> Result<JVal, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-')
        {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>()
            .map(JVal::Num)
            .map_err(|_| format!("JSON 数字非法：{s}"))
    }
}

// ---------------------------------------------------------------
// 序列化（JSON.stringify 兼容：紧凑、键序保序、非 ASCII 直出）
// ---------------------------------------------------------------

pub fn to_json_string(v: &JVal) -> String {
    let mut out = String::new();
    write_val(v, &mut out);
    out
}

fn write_val(v: &JVal, out: &mut String) {
    match v {
        JVal::Null => out.push_str("null"),
        JVal::Bool(true) => out.push_str("true"),
        JVal::Bool(false) => out.push_str("false"),
        JVal::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                out.push_str(&format!("{}", *n as i64));
            } else {
                out.push_str(&format!("{n}"));
            }
        }
        JVal::Str(s) => write_str(s, out),
        JVal::Arr(items) => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_val(it, out);
            }
            out.push(']');
        }
        JVal::Obj(pairs) => {
            out.push('{');
            for (i, (k, val)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_val(val, out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
