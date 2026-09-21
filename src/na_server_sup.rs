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

/// ensure 脚本的收场标记（verdict 解析的唯一事实源）
pub const MARK_ALIVE: &str = "NA_SERVER_ALIVE";
pub const MARK_SPAWNED: &str = "NA_SERVER_SPAWNED";
/// 常驻模式收场（2026-09-21 用户拍板「na 来负责监控服务器的这个服务……
/// 它依然是 na 的触手」）：unit 就位且服务活着
pub const MARK_SYSTEMD: &str = "NA_SERVER_SYSTEMD";
pub const MARK_FAIL: &str = "NA_SERVER_FAIL";

/// systemd unit 名（na 侧唯一引用点名；服务器上只此一份）
pub const UNIT_NAME: &str = "kfm-na-server.service";

/// systemd unit 内容（A 档纯函数，**单一源：内容随 na 走**）。2026-09-21
/// 用户拍板形态：**服务常驻在服务器，但它依然是 na 的触手**——na 发起
/// 安装/更新/状态/控制，systemd 只负责「活着」（na 被杀、手机重启、整机
/// 断电重启都不影响它）。三条纪律写进 unit：①只绑回环 127.0.0.1（公网
/// 不可达 = 安全语义，与 HEALTH_URL 同尺）；②`NA_IDLE_EXIT_SECS=0`
/// 永不自退（常驻的全部意义，取代自持模式的 1800s 自退）；③
/// `Restart=always` 收尸（崩了自己起，不靠 na 的探针兜）
pub fn unit_content() -> String {
    format!(
        r#"[Unit]
Description=KFM-NA session backend (na-server · na 的常驻触手)
After=network.target

[Service]
Type=simple
WorkingDirectory={REPO_DIR}
Environment=NA_BIND=127.0.0.1:{NA_SERVER_PORT}
Environment=NA_IDLE_EXIT_SECS=0
ExecStart={REPO_DIR}/target/release/na-server
Restart=always
RestartSec=2
StandardOutput=append:/var/log/kfm-na-server.log
StandardError=append:/var/log/kfm-na-server.log

[Install]
WantedBy=multi-user.target
"#,
        REPO_DIR = REPO_DIR,
        NA_SERVER_PORT = NA_SERVER_PORT,
    )
}

/// ensure 脚本（A 档纯函数）：幂等保证的四段序——
/// ①缺二进制或**源码比二进制新**则建 ②**systemd 在 = 常驻模式**：装/更新
/// unit（内容随 na 走）+ `enable --now`（幂等，不重启在跑的）+ 探活
/// ③无 systemd（Termux 等）= 降级自持：**先探活（活 = 接管，绝不重启
/// 别人的进程）**→ detached 拉起 → 复检 ④两路失败都落 MARK_FAIL。
/// 顺序错了 = 重启别人的 na-server 或拉出死娃报假绿。
///
/// ②的「源码更新」判据（2026-09-21 负载判色案落地）：na-server 是契约的
/// 一端（/api/na/sys 加 cores 键就靠它下发），只判「二进制在不在」会让
/// 契约变更**默默不生效**——本机实测：改了 na-server 源码，在跑的老进程
/// 照旧下发旧契约，负载轨白等一个口径。故源（na-server/na-sys 两侧 src）
/// 有比二进制新的 .rs 就重建；在跑的老进程不动（等它自己 idle 退出或
/// 下次拉起换新），绝不为了新契约掐别人的会话。
pub fn ensure_script() -> String {
    format!(
        r#"H={HEALTH_URL}
UNIT=/etc/systemd/system/{UNIT_NAME}
cd {REPO_DIR} || {{ echo {MARK_FAIL}; exit 1; }}
STALE=$(find crates/na-server/src crates/na-sys/src -name '*.rs' -newer target/release/na-server 2>/dev/null | head -1)
if [ ! -x target/release/na-server ] || [ -n "$STALE" ]; then
  cargo build --release -p na-server >&2 || {{ echo {MARK_FAIL}; exit 1; }}
fi
# ①常驻模式（systemd 在）：装/更新 unit + enable --now；内容变了才重写
# + daemon-reload（幂等，na 每次连接都可安全跑）
if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  # 一次性迁移（2026-09-21 常驻化实踩）：unit 还没 active 而 9021 上有人
  # = 自持时代的遗留 na-server（na 旧路 spawn 的，PPID 1 但 systemd 不认识
  # 它）——不停它就绑不上口，unit 会在 Restart=always 里空转（实测
  # status=101 循环）。**严判**：只认 cmdline 含本仓 target/release/na-server
  # 的进程，别的（别人的服务）一律不动；其上的 ws 会话断一次，由远端
  # tmux 续上（会话不丢）
  if ! systemctl is-active --quiet {UNIT_NAME}; then
    for p in $(ss -tlnpH "sport = :{NA_SERVER_PORT}" 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u); do
      if tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -q 'target/release/na-server'; then
        kill "$p" 2>/dev/null && echo "migrated=stopped-spawn:$p"
      fi
    done
    sleep 1
  fi
  cat > "$UNIT.new" <<'KFM_UNIT_EOF'
{UNIT_CONTENT}KFM_UNIT_EOF
  chmod 644 "$UNIT.new"
  if ! cmp -s "$UNIT.new" "$UNIT"; then mv "$UNIT.new" "$UNIT"; systemctl daemon-reload; fi
  rm -f "$UNIT.new"
  systemctl enable --now {UNIT_NAME} >/dev/null 2>&1
  sleep 1
  if curl -s -m 2 "$H" >/dev/null 2>&1; then echo {MARK_SYSTEMD}; echo "mode=systemd"; exit 0; fi
  echo {MARK_FAIL}; exit 1
fi
# ②降级（无 systemd：Termux 等）：先探活（活 = 接管，绝不重启别人的进程），
# 再自持 spawn + 30 分钟 idle 自退
if curl -s -m 2 "$H" >/dev/null 2>&1; then echo {MARK_ALIVE}; echo "mode=external"; exit 0; fi
setsid nohup env NA_BIND=127.0.0.1:{NA_SERVER_PORT} NA_IDLE_EXIT_SECS=1800 ./target/release/na-server >/tmp/na-server.log 2>&1 </dev/null &
sleep 1
if curl -s -m 2 "$H" >/dev/null 2>&1; then echo {MARK_SPAWNED}; echo "mode=spawn"; else echo {MARK_FAIL}; exit 1; fi
"#,
        HEALTH_URL = HEALTH_URL,
        UNIT_NAME = UNIT_NAME,
        UNIT_CONTENT = unit_content(),
        MARK_FAIL = MARK_FAIL,
        REPO_DIR = REPO_DIR,
        NA_SERVER_PORT = NA_SERVER_PORT,
        MARK_SYSTEMD = MARK_SYSTEMD,
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
    /// 本次我们拉起的（自持降级路）
    Spawned,
    /// 常驻 unit 就位且活着（systemd 路——na 只装/更新，活着归它）
    Systemd,
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
        if l == MARK_SYSTEMD {
            return Verdict::Systemd;
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

/// 状态发布裁决（A 档纯函数，2026-09-21 用户「我就算在 na 客户端里，也会
/// 看到它反复地重连」定案）：ensure 探针的**在途相**（Checking）不是客户端
/// 状态——每 15s 探一次就发布一次，服务卡状态词被翻成「自持在线 ↔ 确认中」
/// 跳动（实测日志：同一秒里 nasup 状态迁移数次），用户读成「反复重连」。
/// 故在途相**不发布**：只有结果相（Up/ExternalUp/Down/TunnelDown）进快照。
/// 首次探针前仍是 Checking（那时「确认中」是真话：我们还不知道）
pub fn publish_state(prev: &SupState, next: SupState) -> SupState {
    match next {
        SupState::Checking => prev.clone(),
        other => other,
    }
}

/// 后端承载模式（2026-09-21 用户拍板「服务常驻在服务器，但依然是 na 的
/// 触手……na 来负责监控服务器的这个状态」）：na 读它决定卡面怎么说话
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupMode {
    /// systemd 常驻（unit 由 na 安装/更新，活着归 systemd）
    Systemd,
    /// 自持 spawn（无 systemd 的机器：Termux 等）
    Spawn,
    /// 借用已在跑的（别人的/上次留守的）
    External,
    /// 未知（还没跑过 ensure）
    Unknown,
}

pub fn mode_word(m: SupMode) -> &'static str {
    match m {
        SupMode::Systemd => "常驻",
        SupMode::Spawn => "自持",
        SupMode::External => "借用",
        SupMode::Unknown => "—",
    }
}

/// ensure 输出 → 承载模式（A 档纯函数）：脚本末尾回一行 `mode=...`
pub fn mode_of(output: &str) -> SupMode {
    for line in output.lines().rev() {
        match line.trim() {
            "mode=systemd" => return SupMode::Systemd,
            "mode=spawn" => return SupMode::Spawn,
            "mode=external" => return SupMode::External,
            _ => {}
        }
    }
    SupMode::Unknown
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
    /// 承载模式（常驻/自持/借用）——na 是触手：它读这个决定说什么
    pub mode: SupMode,
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
/// 复检节拍（秒）：常驻模式下 na 只是**看状态**（活着归 systemd），
/// 故 2026-09-21 从 15s 放宽到 60s——原先 15s 是「伺候一个随时会死的
/// 自持娃」的节拍（服务器 auth.log 里手机每 15s 一次 ssh 登录就是它）
pub const RECHECK_SECS: u64 = 60;

/// 隧道不可用时的待命节拍（A 档常量）
pub const TUNNEL_WAIT_SECS: u64 = 5;

/// 跑一次 ensure（B 档胶水）：ssh 起 `bash -s`，脚本喂 stdin，
/// 带总超时——超时杀娃报 Failed（病态网络不许挂死看门狗）
fn run_exec_once(prefix: &std::path::Path, server: &ServerEntry) -> (Verdict, SupMode) {
    let Ok(args) = exec_args(server) else {
        return (Verdict::Failed("ssh 配置缺件".into()), SupMode::Unknown);
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
        Err(e) => {
            return (
                Verdict::Failed(format!("ssh spawn 失败: {e}")),
                SupMode::Unknown,
            );
        }
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
                return (
                    Verdict::Failed(format!("ssh 退出 {:?}", out.status.code())),
                    SupMode::Unknown,
                );
            }
            (v, mode_of(&text))
        }
        Ok(Err(e)) => (
            Verdict::Failed(format!("ssh 收尸失败: {e}")),
            SupMode::Unknown,
        ),
        Err(_) => (
            Verdict::Failed(format!("exec 超时（{EXEC_TIMEOUT_SECS}s）")),
            SupMode::Unknown,
        ),
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
        // 初始相 = 确认中（首次探针前「我们还不知道」，这是真话）；
        // 此后只发布结果相（publish_state）
        state: SupState::Checking,
        mode: SupMode::Unknown,
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
            let st = publish_state(&g.state, st); // 在途相不发布（见上）
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

            // 在途相由 publish_state 挡在快照外（探测本身不该让卡面跳字）
            let (verdict, mode) = run_exec_once(&prefix, &server);
            {
                let mut g = snap_t.lock().unwrap();
                if g.mode != mode {
                    g.mode = mode;
                    g.epoch += 1; // 卡面重烘（模式词变了）
                }
            }
            match verdict {
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
                Verdict::Systemd => {
                    attempts = 0;
                    we_spawned = false; // 活着归 systemd，不是「我们的娃」
                    set(SupState::ExternalUp, &snap_t);
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
