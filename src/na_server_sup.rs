//! na_server_sup.rs — 主体拉起链看门狗（na-server.md §二 生命周期）
//!
//! na 是主体：隧道可用后，经 SSH exec 在服务器上**幂等确保** na-server
//! 活着（先在服务器本地探 health，活则接管不重启，死则建起再拉起）。
//! 归属语义照抄隧道：我们拉起的 = Up（自持在线）；连上时发现已在跑的
//! （上次留守/别人起的）= ExternalUp（外部借用），只接管不杀。
//!
//! 分层：上半 A 档纯逻辑（ensure 脚本/exec 参数/verdict 解析/状态词，
//! tests/na_server_sup_spec.rs 钉死）；下半 B 档进程胶水（ssh exec +
//! 看门狗单线程滴答）。

use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, OnceLock};

use crate::settings::ServerEntry;
use crate::tunnel::{NA_SERVER_PORT, backoff_secs, check_ssh_fields};

/// 服务器侧仓库路径（二进制建造与落点；v1 常量，多设备化时进 settings）
pub const REPO_DIR: &str = "/root/kfm-na";

/// 服务器本地 health 口（curl 探活只打回环——公网不可达是安全语义）
pub const HEALTH_URL: &str = "http://127.0.0.1:9021/api/na/health";

/// ensure 脚本的三种收场标记（verdict 解析的唯一事实源）
pub const MARK_ALIVE: &str = "NA_SERVER_ALIVE";
pub const MARK_SPAWNED: &str = "NA_SERVER_SPAWNED";
pub const MARK_FAIL: &str = "NA_SERVER_FAIL";

/// ensure 脚本（A 档纯函数）：幂等保证的三段序——
/// ①先探活（活 = 接管，绝不重启别人的进程）②缺二进制则建 ③detached
/// 拉起后复检。顺序错了 = 重启别人的 na-server 或拉出死娃报假绿。
pub fn ensure_script() -> String {
    format!(
        r#"H={HEALTH_URL}
if curl -s -m 2 "$H" >/dev/null 2>&1; then echo {MARK_ALIVE}; exit 0; fi
cd {REPO_DIR} || {{ echo {MARK_FAIL}; exit 1; }}
if [ ! -x target/release/na-server ]; then
  cargo build --release -p na-server >&2 || {{ echo {MARK_FAIL}; exit 1; }}
fi
setsid nohup env NA_BIND=127.0.0.1:{NA_SERVER_PORT} NA_IDLE_EXIT_SECS=1800 ./target/release/na-server >/tmp/na-server.log 2>&1 </dev/null &
sleep 1
if curl -s -m 2 "$H" >/dev/null 2>&1; then echo {MARK_SPAWNED}; else echo {MARK_FAIL}; exit 1; fi
"#,
        HEALTH_URL = HEALTH_URL,
        MARK_ALIVE = MARK_ALIVE,
        MARK_FAIL = MARK_FAIL,
        REPO_DIR = REPO_DIR,
        NA_SERVER_PORT = NA_SERVER_PORT,
        MARK_SPAWNED = MARK_SPAWNED,
    )
}

/// ssh exec 参数（A 档纯函数）：脚本走 stdin（`bash -s`），参数面与
/// 隧道同尺（check_ssh_fields 单一源）——无 -N/-L（这不是隧道），
/// ConnectTimeout 兜底病态网络（批模式下 ssh 自身无总超时）
pub fn exec_args(s: &ServerEntry) -> Result<Vec<String>, String> {
    check_ssh_fields(s)?;
    Ok(vec![
        "-i".into(),
        s.ssh.key_path.clone(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-p".into(),
        s.ssh.port.to_string(),
        format!("{}@{}", s.ssh.user, s.ssh.host),
        "bash".into(),
        "-s".into(),
    ])
}

/// ensure 收场（A 档）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 已在跑（接管——我们没拉它）
    Alive,
    /// 本次我们拉起的
    Spawned,
    /// 拉起/探活失败
    Failed(String),
}

/// verdict 解析（A 档纯函数）：取输出里**最后一个**标记行（脚本中途
/// 的 cargo 输出可能含任何字，以收尾标记为准）；无标记 = Failed
pub fn verdict_of(output: &str) -> Verdict {
    for line in output.lines().rev() {
        let l = line.trim();
        if l == MARK_ALIVE {
            return Verdict::Alive;
        }
        if l == MARK_SPAWNED {
            return Verdict::Spawned;
        }
        if l == MARK_FAIL {
            return Verdict::Failed("ensure 脚本自报失败".into());
        }
    }
    Verdict::Failed("输出无收场标记".into())
}

/// 看门狗状态（A 档）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupState {
    /// 我们拉起的 na-server 在线
    Up,
    /// 接管的 na-server 在线（别人/上次留守拉的）
    ExternalUp,
    /// ensure exec 在途
    Checking,
    /// 隧道不可用，拉起链待命（不算失败次数——这不是我们的病）
    TunnelDown,
    /// ensure 失败，退避重试中
    Down { attempts: u32, last_error: String },
}

/// 状态词（A 档纯函数）：服务卡状态行的唯一文案源
pub fn state_word(st: &SupState) -> String {
    match st {
        SupState::Up => "自持在线".into(),
        SupState::ExternalUp => "外部借用".into(),
        SupState::Checking => "确认中".into(),
        SupState::TunnelDown => "待隧道".into(),
        SupState::Down { attempts, .. } if *attempts == 0 => "未启动".into(),
        SupState::Down { attempts, .. } => format!("退避 ×{attempts}"),
    }
}

// ---- 数据面（UI 只读这道门）----

static SUP_SNAP: OnceLock<Arc<Mutex<SupSnap>>> = OnceLock::new();

/// 对外快照（服务卡数据源）
#[derive(Debug, Clone)]
pub struct SupSnap {
    pub state: SupState,
    /// user@host:port（卡上显示用）
    pub target: String,
    /// 代际戳：状态每变一次 +1
    pub epoch: u64,
}

/// 读拉起链快照（没起看门狗 = None——后端非 na-server 同相）
pub fn snap() -> Option<Arc<Mutex<SupSnap>>> {
    SUP_SNAP.get().cloned()
}

// ---- B 档：进程胶水（ssh exec + 看门狗），判卷 = redroid 实录 + report 行 ----

/// exec 总超时（A 档常量）：首次可能含 cargo build（release ~2min），
/// 给 300s；常态探活是亚秒级
pub const EXEC_TIMEOUT_SECS: u64 = 300;

/// 在线复查节拍（A 档常量）
pub const RECHECK_SECS: u64 = 15;

/// 隧道不可用时的待命节拍（A 档常量）
pub const TUNNEL_WAIT_SECS: u64 = 5;

/// 跑一次 ensure（B 档胶水）：ssh 起 `bash -s`，脚本喂 stdin，
/// 带总超时——超时杀娃报 Failed（病态网络不许挂死看门狗）
fn run_exec_once(prefix: &std::path::Path, server: &ServerEntry) -> Verdict {
    let Ok(args) = exec_args(server) else {
        return Verdict::Failed("ssh 配置缺件".into());
    };
    let ssh_bin = prefix.join("bin/ssh");
    let child = std::process::Command::new(&ssh_bin)
        .args(&args)
        .env("PATH", prefix.join("bin"))
        .env("LD_LIBRARY_PATH", prefix.join("lib"))
        .env("PREFIX", prefix)
        .env("TERMUX__PREFIX", prefix)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Verdict::Failed(format!("ssh spawn 失败: {e}")),
    };
    if let Some(mut sin) = child.stdin.take() {
        use std::io::Write as _;
        let _ = sin.write_all(ensure_script().as_bytes());
        drop(sin); // 关 stdin = 脚本开跑的信号
    }
    // wait_with_output 阻塞 → 交子线程 + 信道超时
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(EXEC_TIMEOUT_SECS)) {
        Ok(Ok(out)) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let v = verdict_of(&text);
            if matches!(v, Verdict::Failed(_)) && !out.status.success() {
                return Verdict::Failed(format!("ssh 退出 {:?}", out.status.code()));
            }
            v
        }
        Ok(Err(e)) => Verdict::Failed(format!("ssh 收尸失败: {e}")),
        Err(_) => Verdict::Failed(format!("exec 超时（{EXEC_TIMEOUT_SECS}s）")),
    }
}

/// 起拉起链看门狗（单线程）：隧道可用才动手——隧道都没有，拉起也连不上。
/// **幂等**：重复调用返回已在跑的那份。
pub fn start(prefix: PathBuf, server: ServerEntry) -> Arc<Mutex<SupSnap>> {
    if let Some(existing) = SUP_SNAP.get() {
        crate::report::report("nasup", "看门狗已在跑，忽略重复启动");
        return Arc::clone(existing);
    }
    let snap = Arc::new(Mutex::new(SupSnap {
        state: SupState::Down {
            attempts: 0,
            last_error: "未启动".into(),
        },
        target: format!(
            "{}@{}:{}",
            server.ssh.user, server.ssh.host, server.ssh.port
        ),
        epoch: 0,
    }));
    SUP_SNAP.set(Arc::clone(&snap)).ok();
    if exec_args(&server).is_err() {
        let mut g = snap.lock().unwrap();
        g.state = SupState::Down {
            attempts: 0,
            last_error: "ssh 配置缺件".into(),
        };
        g.target = "—".into();
        g.epoch += 1;
        drop(g);
        crate::report::report("nasup", "ssh 配置缺件，拉起链不启动（设置页补齐）");
        return snap;
    }
    crate::report::report(
        "nasup",
        &format!(
            "起 na-server 拉起链看门狗：{}@{}:{}",
            server.ssh.user, server.ssh.host, server.ssh.port
        ),
    );
    let snap_t = Arc::clone(&snap);
    std::thread::spawn(move || {
        let mut attempts: u32 = 0;
        // 我们拉起过 = true（ALIVE 时分「自持」与「借用」的唯一依据）
        let mut we_spawned = false;
        let set = |st: SupState, snap_t: &Arc<Mutex<SupSnap>>| {
            let mut g = snap_t.lock().unwrap();
            if g.state != st {
                crate::report::report("nasup", &format!("状态 {:?} → {:?}", g.state, st));
                g.state = st;
                g.epoch += 1;
            }
        };
        loop {
            // 门一：隧道可用才动手（隧道都没有，拉起也连不上）
            let tunnel_ok = crate::tunnel::snap()
                .and_then(|t| t.lock().ok().map(|g| crate::tunnel::usable(&g.state)))
                .unwrap_or(false);
            if !tunnel_ok {
                set(SupState::TunnelDown, &snap_t);
                std::thread::sleep(std::time::Duration::from_secs(TUNNEL_WAIT_SECS));
                continue;
            }

            set(SupState::Checking, &snap_t);
            match run_exec_once(&prefix, &server) {
                Verdict::Alive => {
                    attempts = 0;
                    set(
                        if we_spawned {
                            SupState::Up
                        } else {
                            SupState::ExternalUp
                        },
                        &snap_t,
                    );
                }
                Verdict::Spawned => {
                    attempts = 0;
                    we_spawned = true;
                    crate::report::report("nasup", "na-server 已由我们拉起");
                    set(SupState::Up, &snap_t);
                }
                Verdict::Failed(e) => {
                    attempts += 1;
                    we_spawned = false; // 拉死了：在跑的那个不是我们的娃
                    crate::report::report(
                        "nasup",
                        &format!("ensure 失败（{e}），第 {attempts} 次"),
                    );
                    set(
                        SupState::Down {
                            attempts,
                            last_error: e,
                        },
                        &snap_t,
                    );
                    std::thread::sleep(std::time::Duration::from_secs(backoff_secs(attempts)));
                    continue;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(RECHECK_SECS));
        }
    });
    snap
}
