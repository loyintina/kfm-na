//! host.rs — Host trait：核心逻辑的唯一宿主依赖面（工单④分层铁律）。
//!
//! fs 根 / 命令执行 / 时钟全在这一个 trait 后面。考题注入 FakeHost
//! （tests/host_spec.rs），真机用 StdHost。核心模块（agent/session/tools）
//! 只许见 Host，不许直接碰 std::fs / std::process / SystemTime。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// run_command 的产物（全量进 wire，围栏③）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdOut {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait Host {
    fn read_file(&self, path: &str) -> Result<String, String>;
    /// 整写（父目录自动建）。
    fn write_file(&self, path: &str, content: &str) -> Result<(), String>;
    /// append-only 追加一行（父目录自动建；会话 jsonl 的唯一写入姿势）。
    fn append_line(&self, path: &str, line: &str) -> Result<(), String>;
    /// 列目录内文件名（不含子目录递归；不存在 = 空表）。会话 NNNN 序号扫描用。
    fn list_files(&self, dir: &str) -> Result<Vec<String>, String>;
    /// 执行命令。围栏（cwd 锁/sudo·su 拒）在 tools.rs 的纯函数闸，
    /// host 只管「在配置根里跑」这一件宿主事。
    fn run_command(&self, command: &str) -> Result<CmdOut, String>;
    fn now_rfc3339(&self) -> String;
}

/// 真宿主：std::fs / sh -c（cwd 恒为配置根）/ SystemTime。
/// 平台无关——std 三件套 Android/Termux/host Linux 同一份语义。
pub struct StdHost {
    workdir: PathBuf,
}

impl StdHost {
    pub fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }
}

impl Host for StdHost {
    fn read_file(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("读 {path} 失败: {e}"))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<(), String> {
        if let Some(dir) = Path::new(path).parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("建目录 {dir:?} 失败: {e}"))?;
        }
        std::fs::write(path, content).map_err(|e| format!("写 {path} 失败: {e}"))
    }

    fn append_line(&self, path: &str, line: &str) -> Result<(), String> {
        use std::io::Write as _;
        if let Some(dir) = Path::new(path).parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("建目录 {dir:?} 失败: {e}"))?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("打开 {path} 失败: {e}"))?;
        writeln!(f, "{line}").map_err(|e| format!("写 {path} 失败: {e}"))
    }

    fn list_files(&self, dir: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(format!("列目录 {dir} 失败: {e}")),
        };
        for ent in rd {
            let ent = ent.map_err(|e| format!("读目录项失败: {e}"))?;
            if ent
                .file_type()
                .map_err(|e| format!("读类型失败: {e}"))?
                .is_file()
            {
                out.push(ent.file_name().to_string_lossy().into_owned());
            }
        }
        out.sort();
        Ok(out)
    }

    fn run_command(&self, command: &str) -> Result<CmdOut, String> {
        // 围栏①的宿主半：子进程 cwd 恒为配置根（命令文本里的 cd 逃逸由
        // tools::check_command 纯函数闸拦——两层各管一半）
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.workdir)
            .output()
            .map_err(|e| format!("起子进程失败: {e}"))?;
        Ok(CmdOut {
            exit_code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn now_rfc3339(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::utc::format_utc(secs)
    }
}

/// 假宿主（考题唯一注入点）：内存 fs + 剧本命令 + 固定钟。
/// 定义在库里（非 tests/）= 五组钉共享同一夹具，且证明 Host 面自足。
pub struct FakeHost {
    pub files: std::sync::Mutex<BTreeMap<String, String>>,
    /// 追加账：(path, line) 全序——append-only 语义的可回放证据
    pub appended: std::sync::Mutex<Vec<(String, String)>>,
    /// 命令剧本：精确匹配命令文本 → 产物；miss = exit 127
    pub commands: BTreeMap<String, CmdOut>,
    /// 收到的命令全账（cwd 锁/进 wire 判卷用）
    pub ran: std::sync::Mutex<Vec<String>>,
    pub clock: String,
}

impl FakeHost {
    pub fn new(clock: &str) -> Self {
        Self {
            files: std::sync::Mutex::new(BTreeMap::new()),
            appended: std::sync::Mutex::new(Vec::new()),
            commands: BTreeMap::new(),
            ran: std::sync::Mutex::new(Vec::new()),
            clock: clock.to_string(),
        }
    }

    pub fn with_file(self, path: &str, content: &str) -> Self {
        self.files
            .lock()
            .expect("锁")
            .insert(path.to_string(), content.to_string());
        self
    }

    pub fn with_command(mut self, cmd: &str, out: CmdOut) -> Self {
        self.commands.insert(cmd.to_string(), out);
        self
    }
}

impl Host for FakeHost {
    fn read_file(&self, path: &str) -> Result<String, String> {
        self.files
            .lock()
            .expect("锁")
            .get(path)
            .cloned()
            .ok_or_else(|| format!("读 {path} 失败: 不存在"))
    }

    fn write_file(&self, path: &str, content: &str) -> Result<(), String> {
        self.files
            .lock()
            .expect("锁")
            .insert(path.to_string(), content.to_string());
        Ok(())
    }

    fn append_line(&self, path: &str, line: &str) -> Result<(), String> {
        self.appended
            .lock()
            .expect("锁")
            .push((path.to_string(), line.to_string()));
        Ok(())
    }

    fn list_files(&self, dir: &str) -> Result<Vec<String>, String> {
        let prefix = format!("{dir}/");
        let mut out: Vec<String> = self
            .files
            .lock()
            .expect("锁")
            .keys()
            .filter_map(|p| p.strip_prefix(&prefix).map(str::to_string))
            .collect();
        // append-only 会话文件可能只出现在 appended 账里
        for (p, _) in self.appended.lock().expect("锁").iter() {
            if let Some(name) = p.strip_prefix(&prefix)
                && !out.iter().any(|x| x == name)
            {
                out.push(name.to_string());
            }
        }
        out.sort();
        Ok(out)
    }

    fn run_command(&self, command: &str) -> Result<CmdOut, String> {
        self.ran.lock().expect("锁").push(command.to_string());
        match self.commands.get(command) {
            Some(o) => Ok(o.clone()),
            None => Ok(CmdOut {
                exit_code: 127,
                stdout: String::new(),
                stderr: format!("sh: {command}: 剧本外命令"),
            }),
        }
    }

    fn now_rfc3339(&self) -> String {
        self.clock.clone()
    }
}
