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

/// 信件列表条目（BAR-174：mtime = 增量同步比对键，unix 秒，取不到给 0）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterMeta {
    pub name: String,
    pub bytes: u64,
    pub mtime: u64,
}

impl AgentService {
    pub fn new(session_root: &str, provider_json: &str) -> Self {
        Self {
            session_root: session_root.to_string(),
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
    /// 一律 None → 路由层 404，不开任意路径口）
    pub fn inbox_root(&self, key: &str) -> Option<String> {
        match key {
            "mailbox" => Some(format!("{}/{}", self.session_root, session::MAILBOX_DIR)),
            "agent-inbox" => Some(AGENT_INBOX_ROOT.to_string()),
            _ => None,
        }
    }

    /// 点名信箱的信件列表（README.md 是规范不是信，除外）：
    /// [(名, 字节, mtime unix 秒)]——BAR-174 起吃 file_meta 不再逐封整读
    pub fn list_inbox_letters(&self, key: &str) -> Result<Vec<LetterMeta>, String> {
        let Some(dir) = self.inbox_root(key) else {
            return Err(format!("信箱 key 未知: {key:?}"));
        };
        let host = StdHost::new(Path::new(&dir).to_path_buf());
        let mut out = Vec::new();
        for name in host.list_files(&dir)? {
            if valid_letter_name(&name) {
                let (bytes, mtime) = host
                    .file_meta(&format!("{dir}/{name}"))
                    .map(|m| (m.bytes, m.mtime))
                    .unwrap_or((0, 0));
                out.push(LetterMeta { name, bytes, mtime });
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
}

/// 全局评审信箱根（BAR-167：kfmv4 仓只读引用，na 侧只读不写）
pub const AGENT_INBOX_ROOT: &str = "/root/90-信箱/00-主册";

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
