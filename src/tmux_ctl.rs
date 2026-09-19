//! tmux_ctl.rs — tmux 控制核（解析页 tmux 插件的纯逻辑层，A 档）
//!
//! 对象模型（2026-09-19 用户拍板，nz tmux-tabs v2.1 会话版同构）：
//! **标签 = 服务器全部 tmux 会话**。本册零 IO——命令字串构造与 pty
//! 输出解析的纯函数；执行走 tmux_exec（短命 ws 会话），切换 attach 走
//! 壳重开远程连接（嵌套禁止：tmux 客户端内不能再 attach，nz P7）。
//!
//! 安全闸两条：
//! - `{-t '名'` 精确匹配：tmux 目标默认前缀模糊匹配，kill/reflow 打到
//!   错会话不可挽回；
//! - 新建名过 sanitize_name：引号/竖线/分号/控制字符全拒（命令经
//!   `sh -c` 执行，注入面必须死）。

/// list-sessions 的 -F 格式串（cmd_list 与解析共用单一源）
pub const LIST_FORMAT: &str = "#{session_name}|#{session_windows}|#{session_attached}";

/// 一行会话记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSession {
    pub name: String,
    pub windows: u32,
    /// 任意客户端附着（session_attached 是计数，多客户端 >1 也算）
    pub attached: bool,
}

/// 剥 ANSI 转义序列（CSI/OSC 之外的裸 ESC 序列也吞——解析只认净文本）
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ ... 字母  (CSI) ；ESC ] ... BEL/\ (OSC)；ESC + 单字符
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if c2.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    let mut prev = '\0';
                    for c2 in chars.by_ref() {
                        if c2 == '\u{7}' || (prev == '\u{1b}' && c2 == '\\') {
                            break;
                        }
                        prev = c2;
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 解析 list-sessions 输出：pty 脏文本（\r\n/ANSI/空行/shell 噪声）→
/// 干净会话表。行格式 `名|窗数|附着计数`——末两段归字段，其余归名字
/// （名字可含竖线）；任一段不合法 = 整行作废
pub fn parse_session_list(out: &str) -> Vec<TmuxSession> {
    let mut v = Vec::new();
    for raw in out.lines() {
        let line = strip_ansi(raw);
        let line = line.trim();
        let Some(p1) = line.rfind('|') else { continue };
        let Some(p0) = line[..p1].rfind('|') else {
            continue;
        };
        let name = &line[..p0];
        let wins = &line[p0 + 1..p1];
        let att = &line[p1 + 1..];
        if name.is_empty() {
            continue;
        }
        let (Ok(windows), Ok(attached)) = (wins.parse::<u32>(), att.parse::<u32>()) else {
            continue;
        };
        v.push(TmuxSession {
            name: name.to_string(),
            windows,
            attached: attached > 0,
        });
    }
    v
}

/// 列全部会话（短命 ws 执行，尾带 exit 收会话）
pub fn cmd_list() -> String {
    format!("tmux list-sessions -F '{LIST_FORMAT}'; exit")
}

/// 新建会话：Some(名) = 指定名（sanitize 后才许进）；None = tmux 自动编号。
/// -P -F 回打印新会话名（自动编号时壳要知道 attach 谁）
pub fn cmd_new(name: Option<&str>) -> String {
    match name {
        Some(n) => format!("tmux new-session -d -P -F '#{{session_name}}' -s '{n}'; exit"),
        None => "tmux new-session -d -P -F '#{session_name}'; exit".into(),
    }
}

/// 关闭会话（'=' 精确匹配——默认前缀模糊会杀错会话）
pub fn cmd_kill(name: &str) -> String {
    format!("tmux kill-session -t '={name}'; exit")
}

/// 重排：窗口尺寸钉到 na 网格。manual 下立即生效；largest/latest 下
/// tmux 自动翻 manual（2026-09-19 服务器实证 rc=0）——重排即钉死
pub fn cmd_reflow(session: &str, cols: u32, rows: u32) -> String {
    format!("tmux resize-window -t '={session}' -x {cols} -y {rows}; exit")
}

/// 附着命令（壳重开远程连接的启动命令）——常驻命令，**不带 exit**
/// （带了附着即退）。new-session -A：不存在则建、存在则附
pub fn cmd_attach(name: &str) -> String {
    format!("tmux new-session -A -s '{name}'")
}

/// 引号感知分词（'...'/"..." 内空白不切）——`-s 'my srv'` 这类命令串
/// 按 shell 语义取词；引号不配对时退化按空白切（提取场景宁缺毋滥）
fn shell_tokens(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    for c in command.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                } else if c.is_whitespace() {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(c);
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 从远程启动命令串提取附着会话名（`-s <名>` / `-t <名>`，可带引号）。
/// 插件的「本端附着」指示靠它——不靠 attached 旗（那是任意客户端计数）
pub fn session_name_of(command: &str) -> Option<String> {
    let toks = shell_tokens(command);
    for (i, t) in toks.iter().enumerate() {
        if (t == "-s" || t == "-t")
            && let Some(v) = toks.get(i + 1)
        {
            let v = v.trim_matches('\'').trim_matches('"');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// 新建名清洗：trim 后空 = None（走自动编号）；引号/竖线/冒号/分号/
/// 控制字符 = None（注入面与解析面全堵）；>32 字符截断
pub fn sanitize_name(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars()
        .any(|c| matches!(c, '\'' | '|' | ':' | ';') || c.is_control())
    {
        return None;
    }
    Some(t.chars().take(32).collect())
}
