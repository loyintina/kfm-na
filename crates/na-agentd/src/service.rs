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
        let lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        Ok(lines.into_iter().rev().take(n).rev().collect())
    }
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
