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
//! - **conf 自给＋灭口拆除＋秒退熔断**（BAR-259，追踪信 NA0188）：
//!   spawn 前 ensure_conf——kfm-sshd.conf 缺（overlay 装机带走／
//!   postinst 配置步没落）按模板自生成，失败报红不 spawn；stderr 落
//!   tmp/sshd-err.log（Stdio::null 灭口拆掉）；spawn 后秒退（<2s）
//!   连败 8 次熔断永久挂起＋报红（9 机 exit 1 环 1s 重拉永动 8 天
//!   7960 行的教训——1s 退避永动这个类就此杀掉）。
//! - **自身失效信号**（小满 MAIN0116 指路的 kfmv4 教训：看门狗死了也
//!   是绿的）：每 HEARTBEAT_EVERY 拍发一行心跳报表，判据挂「报表断
//!   档」而非「进程在不在」——服务器侧可见断档即知看门狗死。
//!
//! 判卷口径（C 档）：kill sshd 后自动复活；9022 banner 恒在。

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
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
/// 秒退判据（BAR-259 熔断）：存活不足此数 = 秒退连败一记
const QUICK_DEATH: Duration = Duration::from_secs(2);
/// 秒退连败上限：到顶熔断永久挂起（BAR-230 永久挂起同机制）
const FUSE_STRIKES_MAX: u32 = 8;

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

/// 让位期 spawn 裁决（A 档纯逻辑，BAR-247 三环，钉在 sshd_keeper_spec）：
/// 娃不在且口上有健康 banner（外来 sshd 在服务）= **真让位不 spawn**。
/// 旧制让位只停在报表层、每拍仍照拉——spawn→绑口撞墙退 255→再拉，
/// 与外来占口者 1:1 空转（11 机实录 4625 对）。口腾出（banner 消失）
/// 才回 spawn 路接管。
pub fn should_spawn(child_alive: bool, banner_healthy: bool) -> bool {
    !child_alive && !banner_healthy
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
        .stderr(sshd_err_stdio(prefix))
        .spawn()
}

/// kfm-sshd.conf 渲染（BAR-259）：照 11 机在位件回描，路径全随 prefix
/// 走；端口与 tunnel.rs NA_SSHD_PORT 同源（改口只改一处）。
pub fn conf_render(prefix: &std::path::Path) -> String {
    let p = prefix.display();
    format!(
        "Port {}\nListenAddress 127.0.0.1\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nStrictModes no\nAuthorizedKeysFile {}/etc/ssh/na_authorized_keys\nHostKey {}/etc/ssh/ssh_host_ed25519_key\nPidFile {}/var/run/sshd8024.pid\nSshdSessionPath {}/libexec/sshd-session\nSshdAuthPath {}/libexec/sshd-auth\nSubsystem sftp {}/libexec/sftp-server\nSetEnv HOME=/storage/emulated/0/Android/data/dev.kfm.na/files PREFIX={} TMPDIR={}/tmp LD_LIBRARY_PATH={}/lib PATH={}/bin:/system/bin\n",
        SSHD_PORT, p, p, p, p, p, p, p, p, p, p
    )
}

/// spawn 前保证 conf 在场（BAR-259）：缺则按模板自生成——na 的命不交给
/// overlay 完整性（9 机 postinst 配置步没落，exit 1 环跑了 8 天无人知）。
/// 二进制缺 = spawn 永久分类的领地（BAR-229/230），本函数不抢戏直接 Ok；
/// 生成失败 = Err（调用方报红＋永久挂起，重试不自愈同环境死）。
pub fn ensure_conf(prefix: &Path) -> Result<(), String> {
    if !prefix.join("bin/sshd").exists() {
        return Ok(());
    }
    let conf = prefix.join("etc/ssh/kfm-sshd.conf");
    if conf.exists() {
        return Ok(());
    }
    if let Some(dir) = conf.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    std::fs::write(&conf, conf_render(prefix))
        .map_err(|e| format!("写 {}: {e}", conf.display()))?;
    if prefix.join("etc/ssh/na_authorized_keys").exists() {
        crate::report::report("sshd", "kfm-sshd.conf 缺——按模板自生成（BAR-259）");
    } else {
        crate::report::report(
            "sshd",
            "kfm-sshd.conf 缺——按模板自生成（BAR-259）；注意 na_authorized_keys 不在位，探针钥匙将被拒（须 postinst 补）",
        );
    }
    Ok(())
}

/// err.log 单文件上限（超出即轮转清空——只留最近若干轮秒退的证词）
const ERR_LOG_CAP: u64 = 64 * 1024;

/// 打开 stderr 落盘（BAR-259／承影 NA0188 楼5 副伤修：**追加**，不再每次
/// create 截断——截断会把上一轮秒退的证词抹掉，正好抹掉要定罪的那一轮）。
/// 超帽才轮转（删旧文件重开）。
pub fn open_err_log(prefix: &Path) -> std::io::Result<std::fs::File> {
    let dir = prefix.join("tmp");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("sshd-err.log");
    if std::fs::metadata(&path)
        .map(|m| m.len() > ERR_LOG_CAP)
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(&path);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// stderr 落盘（BAR-259：Stdio::null 灭口拆掉）——追加写，证词跨轮留在
/// tmp/sshd-err.log。prefix 不在场（假 prefix 钉/环境死）或落不了盘 →
/// 回 null 兜底不挡 spawn。
fn sshd_err_stdio(prefix: &Path) -> Stdio {
    if !prefix.exists() {
        return Stdio::null();
    }
    match open_err_log(prefix) {
        Ok(f) => Stdio::from(f),
        Err(_) => Stdio::null(),
    }
}

/// 秒退连败熔断裁决（BAR-259，A 档纯逻辑，钉在 sshd_keeper_spec）：
/// survived＝本轮存活时长（None＝冻僵 kill／try_wait 错这类我们动手杀
/// 的，不计连败）；存活 <QUICK_DEATH 计一记、否则清账；到顶＝熔断。
pub fn fuse_step(survived: Option<Duration>, streak: u32) -> (u32, bool) {
    let s = match survived {
        Some(d) if d < QUICK_DEATH => streak + 1,
        _ => 0,
    };
    (s, s >= FUSE_STRIKES_MAX)
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
    // 外来占口在位旗（BAR-247 真让位闸：本拍 banner 健康而娃不在 = true，
    // ④ 的 spawn 被它闸住——只探口不抢口）
    let mut foreign_serving = false;
    // BAR-259 秒退熔断账：连败数＋本轮 spawn 时刻
    let mut quick_streak: u32 = 0;
    let mut spawned_at: Option<std::time::Instant> = None;
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
                    // BAR-259 秒退连败熔断：spawn 成功但秒退（如 conf 缺件）
                    // 不归 spawn 永久分类管，1s 退避会永动——连败到顶即挂起
                    let survived = spawned_at.take().map(|t| t.elapsed());
                    let (s, blow) = fuse_step(survived, quick_streak);
                    quick_streak = s;
                    child = None;
                    stable_since = None;
                    if blow {
                        crate::report::report(
                            "sshd",
                            &format!(
                                "sshd 秒退连败 {quick_streak} 次（每次 <{}s）——熔断挂起（BAR-259），证词见 tmp/sshd-err.log，修复后重启",
                                QUICK_DEATH.as_secs()
                            ),
                        );
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    crate::report::report("sshd", &format!("try_wait 失败: {e}——按退出论"));
                    let _ = c.kill();
                    child = None;
                    stable_since = None;
                    spawned_at = None;
                    quick_streak = 0;
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
                    // 冻僵 kill 是我们动手杀的，不计秒退连败（BAR-259）
                    spawned_at = None;
                    quick_streak = 0;
                }
            }
        } else {
            // 承影 NA0160 5 楼三（判卷边角）：娃不在而 banner 健康 = 口被
            // 外来 sshd 占（run-as+setsid 起的那类不随 force-stop 死）——
            // 报一行定性，免得下次翻进程表；BAR-247 起让位升格为真让位
            // （④ 被 foreign_serving 闸住，不再每拍硬拉撞 255）。
            let banner = probe_banner().map(|l| banner_ok(&l)).unwrap_or(false);
            foreign_serving = banner;
            if should_note_foreign(false, banner, foreign_note) {
                crate::report::report(
                    "sshd",
                    "banner 在但持有娃不在——口被外来 sshd 占，看门狗真让位（BAR-247），口腾即接管",
                );
                foreign_note = true;
            }
        }
        // ③ 稳定清零退避（连败账同清——稳定过的 sshd 不算带病）
        if stable_since.is_some_and(|t| t.elapsed() >= STABLE_RESET) {
            backoff_secs = 1;
            stable_since = None;
            quick_streak = 0;
        }
        // ④ 缺娃重拉（退避睡醒再 spawn）——BAR-247 真让位闸：外来 sshd
        // 占着口（banner 健康）时不 spawn，本拍只探口；口腾出下拍接管
        if should_spawn(child.is_some(), foreign_serving) {
            std::thread::sleep(Duration::from_secs(backoff_secs.min(60)));
            // BAR-259：spawn 前 conf 自给——缺则按模板自生成，失败报红
            // 挂起（生成失败＝环境死同族，重试不自愈）
            if let Err(msg) = ensure_conf(&prefix) {
                crate::report::report(
                    "sshd",
                    &format!(
                        "kfm-sshd.conf 自生成失败（{msg}）——看门狗挂起（BAR-259），修复 prefix 后重启"
                    ),
                );
                return;
            }
            match spawn_sshd(&prefix) {
                Ok(c) => {
                    crate::report::report("sshd", &format!("sshd 已重拉（pid {}）", c.id()));
                    foreign_note = false;
                    spawned_at = Some(std::time::Instant::now());
                    child = Some(c);
                    freeze_strikes = 0;
                    // BAR-259 修（承影 NA0188 楼5 判卷红）：秒退必须在 spawn 后
                    // **当拍**取样——巡检拍是 30s，等下一拍 try_wait 再算存活永远
                    // ≥2s，熔断臂因此永不触发（原实现就是这么形同虚设的）。这里
                    // 用 QUICK_DEATH 窗以 100ms 粒度探，探到即按秒退计。
                    let mut quick_exit: Option<std::process::ExitStatus> = None;
                    let mut probe_killed = false;
                    let probe_steps = (QUICK_DEATH.as_millis() / 100) as u32;
                    for _ in 0..probe_steps {
                        std::thread::sleep(Duration::from_millis(100));
                        let Some(c) = child.as_mut() else { break };
                        match c.try_wait() {
                            Ok(Some(st)) => {
                                quick_exit = Some(st);
                                break;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                crate::report::report(
                                    "sshd",
                                    &format!("try_wait 失败: {e}——按退出论"),
                                );
                                let _ = c.kill();
                                let _ = c.wait();
                                probe_killed = true;
                                break;
                            }
                        }
                    }
                    if probe_killed {
                        child = None;
                        spawned_at = None;
                        stable_since = None;
                        quick_streak = 0;
                    } else if let Some(st) = quick_exit {
                        let survived = spawned_at.take().map(|t| t.elapsed());
                        let (s, blow) = fuse_step(survived, quick_streak);
                        quick_streak = s;
                        crate::report::report(
                            "sshd",
                            &format!(
                                "sshd 秒退（{st}，存活 {}ms）——第 {quick_streak} 次",
                                survived.map(|d| d.as_millis()).unwrap_or(0)
                            ),
                        );
                        child = None;
                        stable_since = None;
                        if blow {
                            crate::report::report(
                                "sshd",
                                &format!(
                                    "sshd 秒退连败 {quick_streak} 次（每次 <{}s）——熔断挂起（BAR-259），证词见 tmp/sshd-err.log，修复后重启",
                                    QUICK_DEATH.as_secs()
                                ),
                            );
                            return;
                        }
                    }
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
