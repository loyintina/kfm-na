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

/// 抓会话全滚动缓冲（外置视口快照，2026-09-25 tmux 像素级滚动）：
/// -p 打 stdout、-e 带 SGR 颜色/样式、-S - 从滚动缓冲顶起（屏上之外的
/// 内容在服务器 tmux 手里，这是唯一取回通道）。目标 `'=name:'`——
/// 精确匹配会话 + 活动窗格（BAR-152 实机定罪：`=name` 裸用在 pane
/// 目标上不成立，「can't find pane」报错文本直接当快照进了浏览态，
/// 整页消失）。尾带 && echo 成功标记——tmux 的 exit 码经 sh -c 传不
/// 回来，报错文本与快照同走 stdout，标记是唯一可靠验收（
/// capture_parse 判卷）
pub fn cmd_capture(name: &str) -> String {
    format!(
        "tmux display-message -p -t '={name}:' '#{{history_size}} #{{history_limit}}' && tmux capture-pane -p -e -S - -t '={name}:' && echo KFM_CAP_OK; exit"
    )
}

/// 增量刷新小抓（v3 immutable-history 合并模型，2026-09-25 用户拍板
/// 「视口在中央也该跟贴底一样活」）：历史不可变——新输出只把屏顶行
/// 挤进历史尾，所以刷新只需抓**当前屏**（不带 -S -，rows 行级小文本，
/// 服务器实证尾空行保留、行数恒等于窗格高），合并时旧全文前
/// hist_old+k 行（=新历史）照抄、新屏接续。头行 `history_size
/// history_limit`——算 k = hist_new - hist_old + 判撞顶回落。
/// 验收同 cmd_capture（&& 链 + 尾标）
pub fn cmd_capture_screen(name: &str) -> String {
    format!(
        "tmux display-message -p -t '={name}:' '#{{history_size}} #{{history_limit}}' && tmux capture-pane -p -e -t '={name}:' && echo KFM_CAP_OK; exit"
    )
}

/// 快照验收（BAR-152）：尾带 KFM_CAP_OK 标记 = 真快照，剥标记返回；
/// 无标记 = 抓取失败（报错文本/空输出），None——调用方原地待命，
/// 不许把垃圾喂进浏览态
pub fn capture_strip_marker(out: &str) -> Option<String> {
    let body = out.trim_end().strip_suffix("KFM_CAP_OK")?;
    Some(body.trim_end_matches(['\r', '\n']).to_string())
}

/// 快照验收 + 史量解析（v3）：剥尾标（BAR-153）后头行 =
/// `history_size history_limit`（两数缺一不可 = 垃圾，None），返回
/// (hist, limit, 正文——首尾 \r\n 剥净，无尾换行）。limit 的用途：
/// hist 撞顶 = 史顶在丢旧行，增量合并会把已丢行当存活——撞顶必须
/// 回落全量（merge_capture 判负闸在调用方）
pub fn capture_parse(out: &str) -> Option<(usize, usize, String)> {
    let body = capture_strip_marker(out)?;
    let (head, text) = body.split_once('\n')?;
    let mut it = head.split_whitespace();
    let hist = it.next()?.parse::<usize>().ok()?;
    let limit = it.next()?.parse::<usize>().ok()?;
    Some((hist, limit, text.to_string()))
}

/// 增量合并（v3 immutable-history 模型，A 档纯文本）：新全文 =
/// 旧全文 first(hist_old + k) 行（=新历史：旧历史 + 旧屏顶 k 行——
/// 被新输出挤进历史尾的那 k 行）+ 新屏 rows 行。k = hist_new -
/// hist_old。判负回落（None = 调用方改全量抓）：k<0（对端清史）/
/// 旧全文行数 ≠ hist_old+rows（resize 过境/对端重排）/新屏行数
/// ≠ rows（同因）。history_limit 撞顶时 k 必然伴随丢史顶——合并
/// 会把已丢行当存活，故调用方须保证 hist 未撞顶才走本路（撞顶
/// 回全量）
pub fn merge_capture(
    old_full: &str,
    hist_old: usize,
    rows: usize,
    hist_new: usize,
    screen: &str,
) -> Option<String> {
    let k = hist_new.checked_sub(hist_old)?;
    if k > rows {
        return None; // 刷新间隔内 scrolled 超一屏：挤进历史的行没全被
        // 旧屏抓到过（旧屏只有 rows 行可抄）——回落全量
    }
    let old_lines: Vec<&str> = old_full.split('\n').collect();
    if old_lines.len() != hist_old + rows {
        return None;
    }
    let screen_lines: Vec<&str> = screen.split('\n').collect();
    if screen_lines.len() != rows {
        return None;
    }
    let mut out = String::with_capacity(old_full.len() + screen.len());
    for (i, line) in old_lines[..hist_old + k].iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    for line in &screen_lines {
        out.push('\n');
        out.push_str(line);
    }
    Some(out)
}

/// 重孵附着裁决（BAR-144）：自动重孵拿哪条启动命令——
/// 附着账在 → 附回账上那个会话（用户切去的 nz，不是设置里的默认）；
/// 账空 → 配置原命令照旧。病灶实录：respawn_session 远程臂一刀切
/// default_config，用户 attach nz 后隧道一抖，重孵即被拽回默认会话。
/// 纯函数（A 档）：attached=None 必须原样还回 default_cmd（一字不动，
/// 脱离 tmux 后的裸 shell 重孵语义靠这一臂保住）。
pub fn respawn_attach_cmd(attached: Option<&str>, default_cmd: Option<&str>) -> Option<String> {
    match attached {
        Some(name) => Some(cmd_attach(name)),
        None => default_cmd.map(str::to_string),
    }
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

// ---- 控制模式（tmux -C）协议解析层（2026-09-25 服务器实证 tmux 3.4）----
//
// tmux -C attach 后服务器把输出与事件全推成行协议：
// - `%output %<pane数字id> <载荷>`：窗格新输出；载荷里不可见字节转义成
//   **三位八进制**（\015=\r、\134=反斜杠自身、\011=tab、\033=ESC），可
//   打印 ASCII 原样；
// - `%begin <号> <号> <旗>` / `%end ...` / `%error ...`：命令回应块边界
//   （出错时 %error 与 %begin 配对）；行号/旗标只是配对凭据，不进枚举；
// - `%exit`：服务器收线（客户端断开），流随即将结束；
// - 其余 % 开头行（%session-changed/%window-add/%layout-change/...）：
//   通知类，只分类不逐字钉。

/// 控制模式附着命令（常驻，**不带 exit**——同 cmd_attach 纪律，带了
/// 附着即退）。`-t '=名:'`：精确匹配会话 + 活动窗格（BAR-152 实机定罪，
/// 裸 '=名' 在 pane 目标上不成立）。`-f ignore-size`：不许抢窗口尺寸
/// ——控制客户端的 pty 默认 80x24，window-size=latest 下 attach 会把
/// 用户窗口掰成客户端尺（2026-09-25 服务器实证：带旗 attach/capture
/// 全程 40x10 纹丝不动）
pub fn cmd_ctrl_attach(name: &str) -> String {
    format!("tmux -C attach-session -f ignore-size -t '={name}:'")
}

/// 带内播种命令对（v4 推流画布）：直接在控制通道发，回应块与 %output
/// 严格不交错（2026-09-25 实证）——块前字节已在快照里、块后字节续喂，
/// 零丢失零重复天然对齐。第一块 = 头行（史量/上限/光标位/pane id，
/// KFMHDR 前缀认领），第二块 = capture 全文（无壳无尾标——块内正文
/// 即纯净 capture 输出，capture_strip_marker 验收链不适用此径）
pub fn cmd_ctrl_seed() -> String {
    "display-message -p 'KFMHDR #{history_size} #{history_limit} #{cursor_x} #{cursor_y} #{pane_id}'\ncapture-pane -p -e -S -\n".to_string()
}

/// 播种头行（cmd_ctrl_seed 第一块正文）：KFMHDR <史量> <上限> <光标列>
/// <光标行> %<pane>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedHeader {
    pub hist: usize,
    pub limit: usize,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub pane: u64,
}

/// 头行解析：五件缺一件/件件不成数 = None（播种作废，下拍重来）
pub fn parse_seed_header(line: &str) -> Option<SeedHeader> {
    let body = line.strip_prefix("KFMHDR ")?;
    let mut it = body.split(' ');
    let hist = it.next()?.parse().ok()?;
    let limit = it.next()?.parse().ok()?;
    let cursor_x = it.next()?.parse().ok()?;
    let cursor_y = it.next()?.parse().ok()?;
    let pane = it.next()?.strip_prefix('%')?.parse().ok()?;
    Some(SeedHeader {
        hist,
        limit,
        cursor_x,
        cursor_y,
        pane,
    })
}

/// 控制模式行事件（parse_ctrl_line 产物）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtrlEvent {
    /// %output：窗格输出。载荷是字节不是 String——UTF-8 多字节可能被
    /// 拆在两行 %output 里，拼接还原归调用方
    Output { pane: u64, bytes: Vec<u8> },
    /// %begin：命令回应块开始（行号/旗标只是配对凭据，不进枚举）
    BlockBegin,
    /// %end：命令回应块正常结束
    BlockEnd,
    /// %error：命令回应块出错结束（块内正文是错误文本）
    BlockError,
    /// %exit：服务器收线，流将结束
    Exit,
    /// 其余 % 开头行（通知类；畸形 % 行也落这里——通知本就不消费
    /// 内容，落这里最无害，且不许 panic）
    Notify,
    /// 非 % 行：命令回应块内正文/杂散输出
    Plain,
}

/// %output 载荷反转义：`\` 后跟恰好三位八进制数字 → 对应字节；其余
/// `\` 原样保留（防御——畸形转义不许丢字节更不许 panic）。返回 Vec<u8>
/// 而非 String：UTF-8 多字节可跨行拆，单行载荷不一定是合法 UTF-8
pub fn ctrl_unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            let d = &b[i + 1..i + 4];
            if d.iter().all(|c| c.is_ascii_digit() && *c < b'8') {
                // 三位八进制最大 \777=511 超 u8——截断兜底（tmux 实证
                // 只发 \000-\377，走不到这）
                let v = u16::from(d[0] - b'0') * 64
                    + u16::from(d[1] - b'0') * 8
                    + u16::from(d[2] - b'0');
                out.push((v & 0xff) as u8);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// 一行 → 事件。行尾 \r 先剥（pty 流可能带 \r\n）。畸形 % 行（%output
/// 缺载荷/pane id 非数字/% 后无内容）落 Notify，不许 panic
pub fn parse_ctrl_line(line: &str) -> CtrlEvent {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let Some(rest) = line.strip_prefix('%') else {
        return CtrlEvent::Plain;
    };
    if rest == "exit" || rest.starts_with("exit ") {
        return CtrlEvent::Exit;
    }
    if rest == "begin" || rest.starts_with("begin ") {
        return CtrlEvent::BlockBegin;
    }
    if rest == "end" || rest.starts_with("end ") {
        return CtrlEvent::BlockEnd;
    }
    if rest == "error" || rest.starts_with("error ") {
        return CtrlEvent::BlockError;
    }
    if let Some(body) = rest.strip_prefix("output ")
        && let Some((id, payload)) = body.split_once(' ')
        && let Some(id) = id.strip_prefix('%')
        && let Ok(pane) = id.parse::<u64>()
    {
        return CtrlEvent::Output {
            pane,
            bytes: ctrl_unescape(payload),
        };
    }
    CtrlEvent::Notify
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
