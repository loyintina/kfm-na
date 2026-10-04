//! sshd_keeper — prefix 侧 sshd 看门狗（BAR-230，追踪信 NA0160）。
//!
//! 为什么存在：sshd 跑在 app 同 uid 下的普通进程，`am force-stop` 或
//! app 重启会把它带走，之后 9022 反连桥「TCP 握得上、banner 拿不到」
//! 且无人重拉（BAR-229 的机制定性；.bashrc 守卫只在交互 bash 启动时
//! 碰运气，强停后没有 bash 会话就永远哑）。与 KfmKeepAliveService 分
//! 工：前台服务防「冻」，本看门狗防「死」。
//!
//! 形态（与 na_server_sup.rs 同族的 B 档胶水）：
//! - **子进程持有式**：`bin/sshd -D -e -f etc/ssh/kfm-sshd.conf` 前台
//!   模式，Child 句柄在手——`try_wait()` 拿真实退出状态，退出按退避
//!   重拉（1s 指数 ×2 封顶 60s；稳定存活 5 分钟退避清零）。
//! - **banner 探活**（冻僵检测，承影 MAIN0115 边界②）：每拍连
//!   127.0.0.1:8024 读首行，`SSH-` 前缀 = 健康；连续两拍无 banner 且
//!   child 仍 alive = 判冻僵 → kill 重拉（两拍防负载抖动误杀）。
//! - **spawn 错误分类**（BAR-229 资产复用）：EACCES/ENOENT = 永久
//!   （prefix 环境态，重试不自愈）→ 报表一行后挂起退出线程；其余按
//!   瞬时退避。
//! - **自身失效信号**（小满 MAIN0116 指路的 kfmv4 教训：看门狗死了也
//!   是绿的）：每 HEARTBEAT_EVERY 拍发一行心跳报表，判据挂「报表断
//!   档」而非「进程在不在」——服务器侧可见断档即知看门狗死。
//!
//! 判卷口径（C 档）：kill sshd 后自动复活；9022 banner 恒在。

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::na_server_sup::is_permanent_spawn_err;

/// 巡检节拍（秒）
const RECHECK_SECS: u64 = 30;
/// 心跳节拍：每 N 拍一行（30s × 10 = 5 分钟一拍，断档阈值挂 20 分钟级）
const HEARTBEAT_EVERY: u32 = 10;
/// banner 探活超时（连接与读首行各一）
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// 冻僵判定的连续失败拍数（防负载抖动误杀）
const FREEZE_STRIKES: u32 = 2;
/// 稳定存活多久后退避清零
const STABLE_RESET: Duration = Duration::from_secs(300);

/// sshd 端口（与 tunnel.rs NA_SSHD_PORT 同源——那边是 pub const，此
/// 处直接引用见 start_probe；本地副本只为 probe 函数自含）
const SSHD_PORT: u16 = crate::tunnel::NA_SSHD_PORT;

static STARTED: OnceLock<()> = OnceLock::new();

/// banner 首行判健（A 档纯逻辑，钉在 sshd_keeper_spec）
pub fn banner_ok(line: &str) -> bool {
    line.starts_with("SSH-")
}

/// 外来占口定性判据（A 档纯逻辑，钉在 sshd_keeper_spec）：持有娃不在
/// 而探活健康 = 口被外来 sshd 占着服务（承影 NA0160 5 楼真机边角）。
/// 一集一报：已报过（noted）不再刷；娃活着不适用此口径。
pub fn should_note_foreign(child_alive: bool, banner_healthy: bool, already_noted: bool) -> bool {
    !child_alive && banner_healthy && !already_noted
}

/// 幂等启动：重复调用只记一行报表。prefix 不可用时由壳侧拦（不在此
/// 判——哨兵语义单源在 bootstrap::prefix_ready）。
pub fn start(prefix: PathBuf) {
    if STARTED.set(()).is_err() {
        crate::report::report("sshd", "看门狗已在跑，忽略重复启动");
        return;
    }
    crate::report::report(
        "sshd",
        "看门狗启动（BAR-230）：持有式 sshd -D + banner 探活",
    );
    std::thread::spawn(move || keep_loop(prefix));
}

fn spawn_sshd(prefix: &PathBuf) -> std::io::Result<Child> {
    let conf = prefix.join("etc/ssh/kfm-sshd.conf");
    Command::new(prefix.join("bin/sshd"))
        .arg("-D")
        .arg("-e")
        .arg("-f")
        .arg(&conf)
        .env("PATH", prefix.join("bin"))
        .env("LD_LIBRARY_PATH", prefix.join("lib"))
        .env("PREFIX", prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// 连 8024 读首行 banner；连不上/读不到 = None
fn probe_banner() -> Option<String> {
    let addr = format!("127.0.0.1:{SSHD_PORT}");
    let Ok(mut s) = TcpStream::connect_timeout(&addr.parse().ok()?, PROBE_TIMEOUT) else {
        return None;
    };
    s.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    let mut line = String::new();
    let n = BufReader::new(&mut s).read_line(&mut line).ok()?;
    if n == 0 {
        None
    } else {
        Some(line.trim_end().to_string())
    }
}

fn keep_loop(prefix: PathBuf) {
    let mut child: Option<Child> = None;
    let mut backoff_secs: u64 = 1;
    let mut stable_since: Option<std::time::Instant> = None;
    let mut freeze_strikes: u32 = 0;
    // 外来占口定性旗（一集一报，娃绑上口即复位）
    let mut foreign_note = false;
    let mut tick: u32 = 0;
    loop {
        tick = tick.wrapping_add(1);
        // 心跳（自身失效信号：判据挂报表断档，不挂进程在不在）
        if tick.is_multiple_of(HEARTBEAT_EVERY) {
            crate::report::report("sshd", &format!("心跳 {tick}"));
        }
        // ① 子进程退出审理
        if let Some(c) = child.as_mut() {
            match c.try_wait() {
                Ok(Some(st)) => {
                    crate::report::report(
                        "sshd",
                        &format!("sshd 退出（{st}）——{backoff_secs}s 后重拉"),
                    );
                    child = None;
                    stable_since = None;
                }
                Ok(None) => {}
                Err(e) => {
                    crate::report::report("sshd", &format!("try_wait 失败: {e}——按退出论"));
                    let _ = c.kill();
                    child = None;
                    stable_since = None;
                }
            }
        }
        // ② banner 探活（child 在而无 banner = 冻僵嫌疑）
        if child.is_some() {
            let healthy = probe_banner().map(|l| banner_ok(&l)).unwrap_or(false);
            if healthy {
                freeze_strikes = 0;
                if stable_since.is_none() {
                    stable_since = Some(std::time::Instant::now());
                }
            } else {
                freeze_strikes += 1;
                if freeze_strikes >= FREEZE_STRIKES {
                    crate::report::report(
                        "sshd",
                        "banner 探活连续失败判冻僵——kill 重拉（承影 MAIN0115 边界②）",
                    );
                    if let Some(c) = child.as_mut() {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    child = None;
                    stable_since = None;
                }
            }
        } else {
            // 承影 NA0160 5 楼三（判卷边角）：娃不在而 banner 健康 = 口被
            // 外来 sshd 占（run-as+setsid 起的那类不随 force-stop 死）——
            // 报一行定性，免得下次翻进程表；持有娃按退避继续重试绑口。
            let banner = probe_banner().map(|l| banner_ok(&l)).unwrap_or(false);
            if should_note_foreign(false, banner, foreign_note) {
                crate::report::report(
                    "sshd",
                    "banner 在但持有娃不在——口被外来 sshd 占，看门狗让位重试中（BAR-230 承影 5 楼）",
                );
                foreign_note = true;
            }
        }
        // ③ 稳定清零退避
        if stable_since.is_some_and(|t| t.elapsed() >= STABLE_RESET) {
            backoff_secs = 1;
            stable_since = None;
        }
        // ④ 缺娃重拉（退避睡醒再 spawn）
        if child.is_none() {
            std::thread::sleep(Duration::from_secs(backoff_secs.min(60)));
            match spawn_sshd(&prefix) {
                Ok(c) => {
                    crate::report::report("sshd", &format!("sshd 已重拉（pid {}）", c.id()));
                    foreign_note = false;
                    child = Some(c);
                    freeze_strikes = 0;
                }
                Err(e) if is_permanent_spawn_err(&e) => {
                    crate::report::report(
                        "sshd",
                        &format!(
                            "sshd spawn 永久失败（{e}）——看门狗挂起（BAR-230），修复 prefix 后重启"
                        ),
                    );
                    return;
                }
                Err(e) => {
                    crate::report::report("sshd", &format!("sshd spawn 失败: {e}——退避重试"));
                    backoff_secs = (backoff_secs * 2).min(60);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(RECHECK_SECS));
    }
}
