//! service.rs — na-agentd 服务装配：花名册枚举 / send（同步跑到 stop）/
//! tail。核心循环与存储全在 na-agent 库；本层只做三件宿主事：
//! 列 session_root 目录（花名册）、读 provider.json（key 不落日志）、
//! 串行闸（append-only 会话文件的并发卫生，v1 全局锁一把）。

use std::path::Path;
use std::sync::Mutex;

use na_agent::agent::{self, run_turn};
use na_agent::config::LineConfig;
use na_agent::dialect::Message;
use na_agent::host::{Host, StdHost};
use na_agent::httpc::OpenAiClient;
use na_agent::providers;
use na_agent::session::{self, SessionWriter};

pub struct AgentService {
    pub session_root: String,
    /// 信箱根（BAR-212：迁家后两册挂这下面；
    /// main.rs 吃 NA_AGENT_MAIL_ROOT 覆盖，测试直改字段指 tempdir）
    pub mail_root: String,
    pub provider_json: String,
    pub max_rounds: u32,
    /// v1 全局串行闸：append-only 会话文件不许多线程交错写
    pub send_lock: Mutex<()>,
}

/// send 的产物（HTTP 响应面）。
pub struct SendOutcome {
    pub reply: String,
    pub session_path: String,
}

/// 信件列表条目（BAR-174：mtime = 增量同步比对键，unix 秒，取不到给 0；
/// BAR-212 增 time/from/to/title 四个信头解析字段——增量字段，旧客户端
/// 只读 name/bytes/mtime 不断；缺字头字段给 ""）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterMeta {
    pub name: String,
    pub bytes: u64,
    pub mtime: u64,
    pub time: String,
    pub from: String,
    pub to: String,
    pub title: String,
}

impl AgentService {
    pub fn new(session_root: &str, provider_json: &str) -> Self {
        Self {
            session_root: session_root.to_string(),
            mail_root: default_mail_root(),
            provider_json: provider_json.to_string(),
            max_rounds: agent::DEFAULT_MAX_ROUNDS,
            send_lock: Mutex::new(()),
        }
    }

    /// 花名册 = 列目录（信箱是信件面不是 agent 线，除外）；目录不存在 = 空
    pub fn list_lines(&self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        let rd = match std::fs::read_dir(&self.session_root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(format!("列 {} 失败: {e}", self.session_root)),
        };
        for ent in rd {
            let ent = ent.map_err(|e| format!("读目录项失败: {e}"))?;
            if ent
                .file_type()
                .map_err(|e| format!("读类型失败: {e}"))?
                .is_dir()
            {
                let name = ent.file_name().to_string_lossy().into_owned();
                if name != session::MAILBOX_DIR {
                    out.push(name);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// 同步跑到 stop 再返回（工单语义）。全程持串行闸。
    pub fn send(&self, line: &str, message: &str) -> Result<SendOutcome, String> {
        if !session::valid_line_name(line) {
            return Err(format!(
                "线名非法: {line:?}（只许 ASCII [A-Za-z0-9_-]，≤64 字符）"
            ));
        }
        let _guard = self.send_lock.lock().map_err(|_| "串行闸中毒")?;

        let dir = session::line_dir(&self.session_root, line);
        std::fs::create_dir_all(&dir).map_err(|e| format!("建线目录 {dir} 失败: {e}"))?;

        // line.toml：缺即建（创建时间取现在），在则读（provider/model/workdir 可改）
        let toml_path = format!("{dir}/line.toml");
        let cfg = match std::fs::read_to_string(&toml_path) {
            Ok(text) => LineConfig::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = LineConfig::new(&now_rfc3339());
                std::fs::write(&toml_path, cfg.to_toml())
                    .map_err(|e| format!("写 {toml_path} 失败: {e}"))?;
                cfg
            }
            Err(e) => return Err(format!("读 {toml_path} 失败: {e}")),
        };

        let host = StdHost::new(Path::new(&cfg.workdir).to_path_buf());

        // 会话：有最新续写，无则开 NNNN 新会话
        let path = match session::latest_session(&host, &dir)? {
            Some(p) => p,
            None => {
                let seq = session::next_session_seq(&host, &dir)?;
                session::session_path(&dir, seq)
            }
        };
        let mut messages: Vec<Message> = session::replay_messages(&host, &path)?;
        if messages.is_empty() {
            messages.push(Message::system(SYSTEM_PROMPT));
        }
        messages.push(Message::user(message));

        let provider_json = std::fs::read_to_string(&self.provider_json)
            .map_err(|e| format!("读 {} 失败: {e}", self.provider_json))?;
        let provider = providers::resolve(&provider_json, &cfg.provider)?;
        let client = OpenAiClient::new(provider);

        let mut writer = SessionWriter::open(&host, &path);
        writer.user_msg(message)?;
        let reply = run_turn(
            &host,
            &client,
            &mut writer,
            &cfg.model,
            &mut messages,
            self.max_rounds,
        )?;
        Ok(SendOutcome {
            reply,
            session_path: path,
        })
    }

    /// 最新会话的尾部 n 行（原始 jsonl 行，一行一事件，合法 JSON 数组元素）
    pub fn tail(&self, line: &str, n: usize) -> Result<Vec<String>, String> {
        if !session::valid_line_name(line) {
            return Err(format!("线名非法: {line:?}"));
        }
        let dir = session::line_dir(&self.session_root, line);
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        let Some(path) = session::latest_session(&host, &dir)? else {
            return Err(format!("线 {line} 无会话"));
        };
        let text = host.read_file(&path)?;
        Ok(tail_lines(&text, n))
    }

    /// 线内会话列表（BAR-163 工单⑥ B）：[(文件名, 字节数)]，NNNN 升序；
    /// 线不存在 = Err（404 语义归路由层）
    pub fn list_sessions(&self, line: &str) -> Result<Vec<(String, u64)>, String> {
        if !session::valid_line_name(line) {
            return Err(format!("线名非法: {line:?}"));
        }
        let dir = session::line_dir(&self.session_root, line);
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        let mut out = Vec::new();
        for name in host.list_files(&dir)? {
            if is_session_file(&name) {
                let bytes = host
                    .read_file(&format!("{dir}/{name}"))
                    .map(|t| t.len() as u64)
                    .unwrap_or(0);
                out.push((name, bytes));
            }
        }
        if out.is_empty() && !Path::new(&dir).is_dir() {
            return Err(format!("线 {line} 不存在"));
        }
        out.sort();
        Ok(out)
    }

    /// 点名会话文件的尾部 n 行（会话池看内容面）
    pub fn tail_session(&self, line: &str, name: &str, n: usize) -> Result<Vec<String>, String> {
        if !session::valid_line_name(line) {
            return Err(format!("线名非法: {line:?}"));
        }
        if !is_session_file(name) {
            return Err(format!("会话文件名非法: {name:?}（只认 NNNN-*.jsonl）"));
        }
        let dir = session::line_dir(&self.session_root, line);
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        let text = host
            .read_file(&format!("{dir}/{name}"))
            .map_err(|_| format!("会话 {line}/{name} 不存在"))?;
        Ok(tail_lines(&text, n))
    }

    /// 信箱信件列表（README.md 是规范不是信，除外）
    pub fn list_letters(&self) -> Result<Vec<LetterMeta>, String> {
        self.list_inbox_letters("mailbox")
    }

    /// 信件正文
    pub fn letter(&self, name: &str) -> Result<String, String> {
        self.inbox_letter("mailbox", name)
    }

    /// 信箱 key → 根路径映射表（BAR-167，fail-closed：不在表里的 key
    /// 一律 None → 路由层 404，不开任意路径口；
    /// BAR-212 增 main-book/na-book 两册挂 mail_root 下，旧两 key 不动）
    pub fn inbox_root(&self, key: &str) -> Option<String> {
        match key {
            "mailbox" => Some(format!("{}/{}", self.session_root, session::MAILBOX_DIR)),
            "agent-inbox" => Some(agent_inbox_root()),
            "main-book" => Some(format!("{}/00-主册", self.mail_root)),
            "na-book" => Some(format!("{}/10-NA信箱", self.mail_root)),
            _ => None,
        }
    }

    /// 点名信箱的信件列表（README.md 是规范不是信，除外）：
    /// [(名, 字节, mtime unix 秒)]——BAR-174 起吃 file_meta 不再逐封整读；
    /// BAR-212 增信头四字段：只读文件头部几 KB 解析，仍不整读
    pub fn list_inbox_letters(&self, key: &str) -> Result<Vec<LetterMeta>, String> {
        let Some(dir) = self.inbox_root(key) else {
            return Err(format!("信箱 key 未知: {key:?}"));
        };
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        let mut out = Vec::new();
        for name in host.list_files(&dir)? {
            if valid_letter_name(&name) {
                let path = format!("{dir}/{name}");
                let (bytes, mtime) = host
                    .file_meta(&path)
                    .map(|m| (m.bytes, m.mtime))
                    .unwrap_or((0, 0));
                let (title, time, from, to) = parse_letter_head(&read_head(&path));
                out.push(LetterMeta {
                    name,
                    bytes,
                    mtime,
                    time,
                    from,
                    to,
                    title,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// 点名信箱的信件正文
    pub fn inbox_letter(&self, key: &str, name: &str) -> Result<String, String> {
        let Some(dir) = self.inbox_root(key) else {
            return Err(format!("信箱 key 未知: {key:?}"));
        };
        if !valid_letter_name(name) {
            return Err(format!(
                "信件名非法: {name:?}（只认 ASCII *.md 或 v2.1 中文句法名）"
            ));
        }
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        host.read_file(&format!("{dir}/{name}"))
            .map_err(|_| format!("信件 {name} 不存在"))
    }

    /// 点名信箱的摘要批量面（BAR-212 懒加载）：[(名, 摘要, mtime unix 秒)]。
    /// mtime 与列表端点同口径（file_meta，取不到给 0）——客户端摘要缓存
    /// 按 (name, mtime) 对账，信变了摘要作废重取。
    /// 非法名跳过不连坐；信不在盘上同样跳过（列表与点名之间的删除竞态不算错）
    pub fn inbox_summaries(
        &self,
        key: &str,
        names: &[String],
    ) -> Result<Vec<(String, String, u64)>, String> {
        let Some(dir) = self.inbox_root(key) else {
            return Err(format!("信箱 key 未知: {key:?}"));
        };
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        let mut out = Vec::new();
        for name in names {
            if !valid_letter_name(name) {
                continue;
            }
            let path = format!("{dir}/{name}");
            let Ok(text) = host.read_file(&path) else {
                continue;
            };
            let mtime = host.file_meta(&path).map(|m| m.mtime).unwrap_or(0);
            out.push((name.clone(), extract_summary(&text), mtime));
        }
        Ok(out)
    }
}

/// 全局评审信箱根（BAR-167：主册只读引用，na 侧只读不写）。
/// env `NA_AGENT_INBOX_ROOT` 优先，否则 `$HOME/90-信箱/00-主册`——
/// 2026-09-30 边界审计：原缺省写死作者机器路径（chain
/// spec_bar167_端点_agentinbox真根 判卷的是真根可达性，不钉具体路径）
pub fn agent_inbox_root() -> String {
    std::env::var("NA_AGENT_INBOX_ROOT").unwrap_or_else(|_| format!("{}/90-信箱/00-主册", home()))
}

/// $HOME（缺省 `/root`——systemd 服务不设 HOME 时的 uid 0 习惯位；空串按未设算）
fn home() -> String {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "/root".into())
}

/// 信箱迁家后的默认根（BAR-212：main-book/na-book 两册挂这下面；
/// 覆盖口 = NA_AGENT_MAIL_ROOT 环境变量，照 NA_AGENT_SESSION_ROOT 先例；
/// 缺省 `$HOME/90-信箱`——同一审计）
pub fn default_mail_root() -> String {
    std::env::var("NA_AGENT_MAIL_ROOT").unwrap_or_else(|_| format!("{}/90-信箱", home()))
}

/// 信头解析只读文件头部这么多字节（v2.1 字头远在窗口内，不整读）
const HEAD_CAP: usize = 8 * 1024;

/// 摘要面字符上限（BAR-212：按字符安全截断）
pub const SUMMARY_MAX_CHARS: usize = 120;

/// 读文件头部（≤HEAD_CAP 字节；打不开给空串，下游解析全字段落空）
fn read_head(path: &str) -> String {
    use std::io::Read;
    let mut buf = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(HEAD_CAP as u64).read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// v2.1 信头解析（BAR-212）：(H1 标题, 日期, 从, 致)，缺字段一律 ""
fn parse_letter_head(head: &str) -> (String, String, String, String) {
    fn quote_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        let body = line.strip_prefix('>')?.trim_start();
        let v = body.strip_prefix(key)?.strip_prefix(':')?;
        Some(v.trim())
    }
    let (mut title, mut time, mut from, mut to) =
        (String::new(), String::new(), String::new(), String::new());
    for line in head.lines() {
        if title.is_empty()
            && let Some(t) = line.strip_prefix("# ")
        {
            title = t.trim().to_string();
        }
        if time.is_empty()
            && let Some(v) = quote_value(line, "日期")
        {
            time = v.to_string();
        }
        if from.is_empty()
            && let Some(v) = quote_value(line, "从")
        {
            from = v.to_string();
        }
        if to.is_empty()
            && let Some(v) = quote_value(line, "致")
        {
            to = v.to_string();
        }
    }
    (title, time, from, to)
}

/// 摘要提取（BAR-212）：`^## 摘要` 段到下一个 `^##` 前；丢占位提示行
/// （`> 注意` 开头）、空行、LETTER-TOKEN 注释行；剩余行空格连接，
/// 字符安全截断到 SUMMARY_MAX_CHARS。无摘要块 → ""
pub fn extract_summary(text: &str) -> String {
    let mut in_summary = false;
    let mut parts: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.starts_with("##") {
            if in_summary {
                break;
            }
            if line
                .trim_start_matches('#')
                .trim_start()
                .starts_with("摘要")
            {
                in_summary = true;
            }
            continue;
        }
        if !in_summary {
            continue;
        }
        let t = line.trim();
        if t.is_empty() || t.starts_with("> 注意") || t.contains("<!-- LETTER-TOKEN") {
            continue;
        }
        parts.push(t);
    }
    let joined = parts.join(" ");
    joined.chars().take(SUMMARY_MAX_CHARS).collect()
}

/// 尾部 n 非空行
fn tail_lines(text: &str, n: usize) -> Vec<String> {
    let lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    lines.into_iter().rev().take(n).rev().collect()
}

/// 会话文件名闸（路径组件即权限：NNNN-*.jsonl，禁分隔符禁穿越）
fn is_session_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".jsonl") else {
        return false;
    };
    let Some((num, rest)) = stem.split_once('-') else {
        return false;
    };
    num.len() == 4
        && num.bytes().all(|b| b.is_ascii_digit())
        && !rest.is_empty()
        && !rest.bytes().any(|b| b == b'/' || b == b'\\')
}

/// 信件文件名闸（README.md 是规范不是信；禁分隔符）。
/// 两纪元并认（BAR-172，主册 2026-09-28 整体迁移 v2.1 中文名后旧闸把真信全滤光）：
/// 旧 ASCII 名（字母数字/-/_/.）照旧放行；非 ASCII 名必须过 v2.1 文法判卷
/// （`[分拣码]NNNN号<发信人>…的<类型词>.md`，文法唯一出处 = mailbox-core parse_v21_name——
/// 分隔符/非法字天然过不了文法，fail-closed 语义不变）。
pub fn valid_letter_name(name: &str) -> bool {
    if name == "README.md" || name.chars().count() > 128 || !name.ends_with(".md") {
        return false;
    }
    if name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return true;
    }
    mailbox_core::name::is_v21_name(name)
        && mailbox_core::name::parse_v21_name(name).errs.is_empty()
}

/// 系统提示：给模型的角色与工具纪律（写信任务靠它知道信箱规范去
/// 读 README，而不是凭参数幻觉）。
pub const SYSTEM_PROMPT: &str = "你是 na agent（kfm-na 仓的 agent 运行时 v1）。\
工具四件：read_file / write_file / run_command / clock。\
纪律：动手先读任务里点名的文件；写文件前确认规范来源（任务指了 README 就先读 README）；\
run_command 的 cwd 锁在工作区根，禁 sudo/su。\
任务做完用最终回复说清产物路径，不要再调工具。";

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    na_agent::utc::format_utc(secs)
}
