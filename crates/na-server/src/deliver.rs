//! deliver.rs — 输入栏投递面（BAR-252，MAIN0168 定位重定向第一步，追踪信
//! NA0181；设计 = 楼1 清和，裁决 = 楼2 白露四点全裁）。
//!
//! POST /api/deliver（JSON 四键 v/target/action/text）把手机输入栏的一句
//! 话投递进用户正在看的服务器 tmux 窗。**核验与投递一处成案**（无 TOCTOU）：
//! ① 登记表核验（~/.kimi-session-map 每请求重读，文件小不缓存）——不在表
//!    拒「未登记」；service 类拒「服务窗不投递」；
//! ② 活态核验——tmux 无此会话拒「会话不在」；pane_pid 子孙树扫不到
//!    harness 拒「未跑 harness」（kimi 行认 comm kimi/kimi-code；tianshu
//!    行认 cmdline 含 tianshu 的 node；天枢空壳窗落此档）；
//! ③ 键序——send＝**塞话完整键序**（Esc → 0.3s → 字面文本 → 0.3s → Enter
//!    → 1s → Enter；BAR-252／MAIN0175 楼23 用户真机实测定罪：旧 send 只有
//!    「字面文本＋Enter」两条，目标窗只出现文本、没提交）；键序**唯一源**＝
//!    scripts/kfm-inject-lib.sh 的 KFM_INJECT_STEPS（include_str! 编译期读入，
//!    kfmv4 agent-send.sh 共用同一份，不许各写一套）；interrupt＝Escape；
//!    queue＝tianshu 发 M-Enter、kimi 降级 send（action_effective 透出，
//!    回执须能说「排队（已降级为发送）」——白露楼2①）；
//! ④ 留痕——na-server 日志一行（时刻/目标/动作/结果/字节数）＋响应 JSON
//!    （ok/target/action/action_effective/detail/post_screen）回客户端做回执；
//!    post_screen = 键序执行后 600ms 取目标窗尾部三行（判据③「对方真进入
//!    处理」的机器可判证据，BAR-252）。
//!
//! 拒投 detail 写**原因＋出路**（白露楼2②「只说原因不给出路不算可行动的
//! 回执」）；客户端照用 detail 原文不再造一遍（单源纪律，两处文案必漂）。
//!
//! 纯函数面（A 档钉在 tests/deliver_spec.rs）：解析/登记表/活态/键序/回执
//! 体。execute 是薄 IO 胶（读表/tmux/ps），真机判卷归承影。

/// 投递动作（词表与客户端 src/deliver.rs 同源两份，改要同票）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 字面文本 + Enter
    Send,
    /// Escape（打断）
    Interrupt,
    /// 排队：tianshu M-Enter；kimi 无排队键 → 降级 send
    Queue,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Send => "send",
            Action::Interrupt => "interrupt",
            Action::Queue => "queue",
        }
    }

    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "send" => Some(Action::Send),
            "interrupt" => Some(Action::Interrupt),
            "queue" => Some(Action::Queue),
            _ => None,
        }
    }
}

/// 投递请求（v 缺省 = 1；text 缺省 = 空串——interrupt 不带文本）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Req {
    pub target: String,
    pub action: Action,
    pub text: String,
}

/// 解析请求体（A 档纯函数）。坏 JSON/坏版本/坏动作/空目标/目标带控制符
/// 或超长 → Err（detail 原文进响应）
pub fn parse_req(body: &str) -> Result<Req, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("请求体不是合法 JSON: {e}"))?;
    match v.get("v") {
        None | Some(serde_json::Value::Null) => {}
        Some(x) if x.as_u64() == Some(1) => {}
        _ => return Err("协议版本 v 只认 1".into()),
    }
    let target = v
        .get("target")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if target.is_empty() {
        return Err("target 空——投递要指明窗".into());
    }
    if target.chars().count() > 64 || target.chars().any(|c| c.is_control()) {
        return Err("target 非法（超长或带控制符）".into());
    }
    let action = v
        .get("action")
        .and_then(|x| x.as_str())
        .ok_or("action 缺或非串")?;
    let action =
        Action::parse(action).ok_or_else(|| "action 只认 send/interrupt/queue".to_string())?;
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(Req {
        target,
        action,
        text,
    })
}

/// harness 类（登记表第④列；缺省 = kimi，兼容旧三列行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Harness {
    Kimi,
    Tianshu,
    /// 常驻服务窗（proxy/pulse216 等）——不接回不投递，生死归属主自管
    Service,
    /// 表上写了不认识的类——fail-closed 拒投
    Other(String),
}

/// 登记表一行（本件只用 ①名 ④harness；②③ 是接回档案位）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegRow {
    pub name: String,
    pub harness: Harness,
}

/// 解析 ~/.kimi-session-map（A 档纯函数）：tab 分列，# 注释与空行跳过；
/// 第④列缺省 kimi；无 tab 的行不是数据行（跳过不炸）
pub fn parse_registry(text: &str) -> Vec<RegRow> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_end();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = t.split('\t').map(str::trim).collect();
        if fields.len() < 2 || fields[0].is_empty() {
            continue;
        }
        let harness = match fields.get(3).copied().unwrap_or("") {
            "" | "kimi" => Harness::Kimi,
            "tianshu" => Harness::Tianshu,
            "service" => Harness::Service,
            other => Harness::Other(other.to_string()),
        };
        out.push(RegRow {
            name: fields[0].to_string(),
            harness,
        });
    }
    out
}

/// ① 登记表核验（A 档纯函数）：不在表/service 类/未知类 → Err(detail
/// 带出路)；可投递类 → &row
pub fn lookup<'a>(rows: &'a [RegRow], target: &str) -> Result<&'a RegRow, String> {
    let Some(row) = rows.iter().find(|r| r.name == target) else {
        return Err(format!("窗 {target} 未登记——先登记进窗口表，或改投 AI 页"));
    };
    match &row.harness {
        Harness::Service => Err(format!(
            "窗 {target} 是常驻服务窗（不接投递）——改投对话窗，或发往 AI 页"
        )),
        Harness::Other(h) => Err(format!(
            "窗 {target} 的 harness 类 {h:?} 不认识——改投 AI 页"
        )),
        Harness::Kimi | Harness::Tianshu => Ok(row),
    }
}

/// tmux list-panes -a 一行（A 档纯函数解析对象）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub session: String,
    pub pane_pid: u32,
}

/// 解析 `tmux list-panes -a -F '#{session_name}\t#{pane_pid}'` 输出；
/// 坏行跳过不整批炸
pub fn parse_panes(text: &str) -> Vec<Pane> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((name, pid)) = line.split_once('\t') else {
            continue;
        };
        let Ok(pane_pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        out.push(Pane {
            session: name.trim().to_string(),
            pane_pid,
        });
    }
    out
}

/// ps -eo pid=,ppid=,comm=,args= 一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub args: String,
}

/// 解析 ps 输出（A 档纯函数）：前三列定长，余下整段是 args
pub fn parse_ps(text: &str) -> Vec<Proc> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(comm)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) else {
            continue;
        };
        out.push(Proc {
            pid,
            ppid,
            comm: comm.to_string(),
            args: it.collect::<Vec<_>>().join(" "),
        });
    }
    out
}

/// pane_pid 的子孙进程集（含自身——自身是 shell，扫进去无害）。BFS 防环
/// （ppid 成环的畸形 ps 输出不兜会死循环）
pub fn descendants(root: u32, procs: &[Proc]) -> Vec<&Proc> {
    let mut seen = std::collections::HashSet::from([root]);
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(cur) = frontier.pop() {
        for p in procs {
            if p.ppid == cur && seen.insert(p.pid) {
                frontier.push(p.pid);
            }
        }
        if let Some(me) = procs.iter().find(|p| p.pid == cur) {
            out.push(me);
        }
    }
    out
}

/// ② 活态核验（A 档纯函数）：会话不在 → 拒；子孙树扫不到 harness → 拒
/// （天枢空壳落此档）；通过 → pane_pid
pub fn check_alive(row: &RegRow, panes: &[Pane], procs: &[Proc]) -> Result<u32, String> {
    let target = &row.name;
    let Some(pane) = panes.iter().find(|p| p.session == *target) else {
        return Err(format!(
            "窗 {target} 不在（tmux 无此会话）——先起窗，或改投 AI 页"
        ));
    };
    let tree = descendants(pane.pane_pid, procs);
    let alive = match &row.harness {
        Harness::Kimi => tree
            .iter()
            .any(|p| p.comm == "kimi" || p.comm == "kimi-code"),
        Harness::Tianshu => tree
            .iter()
            .any(|p| p.comm == "node" && p.args.contains("tianshu")),
        // Service/Other 在 lookup 已拒，到不了这
        Harness::Service | Harness::Other(_) => false,
    };
    if alive {
        Ok(pane.pane_pid)
    } else {
        Err(format!("窗 {target} 未跑 harness——先起窗，或改投 AI 页"))
    }
}

/// 键序唯一源（BAR-252／MAIN0175 楼23）：手机自举构建时仓外不可见，故
/// 必须随 na 仓走；kfmv4 agent-send.sh source 同一份文件。
pub const INJECT_LIB: &str = include_str!("../../../scripts/kfm-inject-lib.sh");

/// 唯一源里的一步语义（A 档纯函数解析产物）：按键 / 字面文本 / 停顿
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepSpec {
    /// tmux 按键名（Esc/Enter/M-Enter）
    Key(&'static str),
    /// send-keys -l -- <文本>
    Text,
    /// 停顿毫秒
    Sleep(u64),
}

/// 待执行步骤（clippy type_complexity 要求抽别名，termview.rs:2019 同律）：
/// execute 照序 spawn tmux 或线程停顿
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStep {
    Cmd(Vec<String>),
    Sleep(u64),
}

/// 键序计划：步骤序列 + 实际生效动作（降级透出）+ 降级备注
pub type KeyPlan = (Vec<KeyStep>, Action, Option<String>);

/// 声明词 → tmux 键名（唯一源只写 esc/enter/m-enter，键名映射收在这里）
fn key_name(token: &str) -> Option<&'static str> {
    match token {
        "esc" => Some("Escape"),
        "enter" => Some("Enter"),
        "m-enter" => Some("M-Enter"),
        _ => None,
    }
}

/// 解析唯一源声明（A 档纯函数）：取 `KFM_INJECT_STEPS="..."` 一行，
/// 空白分隔逐项转 StepSpec；缺声明/未知词/坏秒数 → Err。
pub fn parse_key_steps(decl: &str) -> Result<Vec<StepSpec>, String> {
    let line = decl
        .lines()
        .find(|l| l.contains("KFM_INJECT_STEPS="))
        .ok_or("键序唯一源缺 KFM_INJECT_STEPS 声明")?;
    let start = line.find('"').ok_or("KFM_INJECT_STEPS 声明缺双引号")?;
    let end = line.rfind('"').ok_or("KFM_INJECT_STEPS 声明缺闭合双引号")?;
    if end <= start + 1 {
        return Err("KFM_INJECT_STEPS 声明为空".into());
    }
    let mut out = Vec::new();
    for token in line[start + 1..end].split_whitespace() {
        if token == "text" {
            out.push(StepSpec::Text);
        } else if let Some(key) = key_name(token) {
            out.push(StepSpec::Key(key));
        } else if let Some(secs) = token.strip_prefix("sleep") {
            let secs: f64 = secs
                .parse()
                .map_err(|_| format!("键序唯一源坏秒数: {token}"))?;
            if !secs.is_finite() || secs <= 0.0 {
                return Err(format!("键序唯一源秒数必须为正: {token}"));
            }
            out.push(StepSpec::Sleep((secs * 1000.0).round() as u64));
        } else {
            return Err(format!("键序唯一源未知步: {token}"));
        }
    }
    if out.is_empty() {
        return Err("键序唯一源声明为空".into());
    }
    Ok(out)
}

/// 展开为待执行命令（A 档纯函数）：`-t =<名>:` 等号＝精确匹配、尾冒号＝
/// 指到会话的活动窗格（BAR-152 铁钉：`=名` 裸用不带冒号在 pane 目标上
/// 不成立「can't find pane」，同坑注释见 tmux_ctl.rs cmd_capture）；
/// `-l --` = 字面文本不解析键名。
pub fn expand_key_steps(specs: &[StepSpec], target: &str, text: &str) -> Vec<KeyStep> {
    specs
        .iter()
        .map(|spec| match spec {
            StepSpec::Key(key) => KeyStep::Cmd(vec![
                "send-keys".to_string(),
                "-t".to_string(),
                target.to_string(),
                key.to_string(),
            ]),
            StepSpec::Text => KeyStep::Cmd(vec![
                "send-keys".to_string(),
                "-t".to_string(),
                target.to_string(),
                "-l".to_string(),
                "--".to_string(),
                text.to_string(),
            ]),
            StepSpec::Sleep(ms) => KeyStep::Sleep(*ms),
        })
        .collect()
}

/// ③ 键序（A 档纯函数）：send 与 queue-kimi 降级臂都走唯一源展开的完整
/// 塞话键序（Esc → 停顿 → 字面文本 → 停顿 → Enter → 停顿 → Enter）。
/// queue 在 tianshu 上仍是文本＋M-Enter；kimi 降级 send（note 透出降级，
/// 响应 action_effective=send）。
pub fn build_keys(
    action: Action,
    harness: &Harness,
    target: &str,
    text: &str,
) -> Result<KeyPlan, String> {
    // BAR-152 锚：目标串必须以冒号收尾（=name: 指会话活动窗格）
    let t = format!("={target}:");
    let send_plan = || -> Result<Vec<KeyStep>, String> {
        let specs = parse_key_steps(INJECT_LIB)?;
        Ok(expand_key_steps(&specs, &t, text))
    };
    match action {
        Action::Send => {
            if text.is_empty() {
                return Err("空文本不投递（interrupt 请用 action=interrupt）".into());
            }
            Ok((send_plan()?, Action::Send, None))
        }
        Action::Interrupt => {
            // 打断只发一次 Esc（不读唯一源——那是塞话提交键序，不是打断）
            let esc = key_name("esc").ok_or("键名表缺 esc")?;
            Ok((
                vec![KeyStep::Cmd(vec![
                    "send-keys".to_string(),
                    "-t".to_string(),
                    t,
                    esc.to_string(),
                ])],
                Action::Interrupt,
                None,
            ))
        }
        Action::Queue => {
            if text.is_empty() {
                return Err("空文本不投递（排队也要先有形）".into());
            }
            match harness {
                Harness::Tianshu => {
                    let m_enter = key_name("m-enter").ok_or("键名表缺 m-enter")?;
                    Ok((
                        vec![
                            KeyStep::Cmd(vec![
                                "send-keys".to_string(),
                                "-t".to_string(),
                                t.clone(),
                                "-l".to_string(),
                                "--".to_string(),
                                text.to_string(),
                            ]),
                            KeyStep::Cmd(vec![
                                "send-keys".to_string(),
                                "-t".to_string(),
                                t,
                                m_enter.to_string(),
                            ]),
                        ],
                        Action::Queue,
                        None,
                    ))
                }
                // kimi 无排队键——降级发送，回执必须说得出口（白露楼2①）
                _ => Ok((
                    send_plan()?,
                    Action::Send,
                    Some("kimi 无排队键——已降级为发送".to_string()),
                )),
            }
        }
    }
}

/// ④ 回执体（A 档纯函数）：五键全在，detail 空串不省略（客户端凭键认
/// 形状——与 sys_json「键永远在」同律）。post_screen 空串表示失败臂或
/// 取证失败——键始终在，判卷人凭键认形状。
pub fn resp_json_with_screen(
    ok: bool,
    target: &str,
    action: &str,
    effective: &str,
    detail: &str,
    post_screen: &str,
) -> String {
    serde_json::json!({
        "ok": ok,
        "target": target,
        "action": action,
        "action_effective": effective,
        "detail": detail,
        "post_screen": post_screen,
    })
    .to_string()
}

/// 旧五键构造（纯函数面的兼容入口；post_screen 置空）
pub fn resp_json(ok: bool, target: &str, action: &str, effective: &str, detail: &str) -> String {
    resp_json_with_screen(ok, target, action, effective, detail, "")
}

/// 剥 ANSI/控制符（A 档纯函数）：CSI（颜色/光标）、OSC（标题/超链接）、
/// 其他 ESC 序列与 \r 一并去掉，只留可读文本与换行。
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\u{1b}' => match it.next() {
                Some('[') => {
                    while let Some(&n) = it.peek() {
                        it.next();
                        if ('\u{40}'..='\u{7e}').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(n) = it.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            if it.peek() == Some(&'\\') {
                                it.next();
                            }
                            break;
                        }
                    }
                }
                Some(_) | None => {}
            },
            '\r' => {}
            c if c.is_control() && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// 目标窗尾部取证（A 档纯函数）：剥 ANSI 后取最后 3 个非空行、总长截到
/// 200 字。判据③「按下后对方窗口真进入处理」的机器可判证据（BAR-252）。
pub fn screen_tail(raw: &str) -> String {
    let clean = strip_ansi(raw);
    let lines: Vec<&str> = clean
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines.len().saturating_sub(3);
    let joined = lines[start..].join("\n");
    joined.chars().take(200).collect()
}

/// 留痕行（A 档纯函数）：时刻/目标/动作/实际动作/结果/字节数（NA0181
/// 楼1 判据④原样）
pub fn log_line(
    epoch_secs: i64,
    target: &str,
    action: &str,
    effective: &str,
    ok: bool,
    bytes: usize,
    detail: &str,
) -> String {
    format!(
        "{} deliver target={} action={} effective={} ok={} bytes={}{}",
        iso8(epoch_secs),
        target,
        action,
        effective,
        ok,
        bytes,
        if detail.is_empty() {
            String::new()
        } else {
            format!(" detail={detail}")
        },
    )
}

/// epoch 秒 → UTC+8 ISO（A 档纯函数；na-server 不引 chrono，civil 算法
/// 手推——钉锁 1970 原点和跨日）
pub fn iso8(epoch_secs: i64) -> String {
    let t = epoch_secs + 8 * 3600;
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    // Howard Hinnant civil_from_days（三月起算：mp 正演常数 +2 与逆演
    // (153*mp+2)/5 互逆，钉锁 1970 原点/跨日/epoch 前负值）
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} +08:00")
}

/// 登记表路径（env NA_SESSION_MAP 可改；缺省 $HOME/.kimi-session-map）
pub fn registry_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("NA_SESSION_MAP") {
        return p.into();
    }
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "/root".into());
    std::path::PathBuf::from(home).join(".kimi-session-map")
}

fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn run_cmd(args: &[String]) -> Result<(), String> {
    let out = std::process::Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| format!("tmux spawn 失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "tmux {} 退出 {:?}: {}",
            args[0],
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn sh_out(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd} spawn 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} 退出 {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 判据③取证（B 档薄 IO）：键序执行后 600ms 取一次目标窗尾部——对方
/// 真进入处理（有新输出/输入框清空）的机器可判证据。capture 失败 = 空串
/// （投递本身已成功，不因取证失败翻成失败）。
fn capture_post_screen(target: &str) -> String {
    std::thread::sleep(std::time::Duration::from_millis(600));
    let t = format!("={target}:");
    match sh_out("tmux", &["capture-pane", "-p", "-t", &t, "-S", "-8"]) {
        Ok(raw) => screen_tail(&raw),
        Err(_) => String::new(),
    }
}

/// 薄 IO 胶（B 档；纯逻辑全在上面）：解析 → 登记表 → 活态 → 键序 →
/// 执行 → 取证 → 留痕 → 回执。返回 (HTTP 状态码, 响应体)：
/// 请求坏形 400；登记表读不到/tmux 故障 500；业务拒投 200 ok:false
/// （请求本身合法，是这扇窗此刻不接——客户端照 detail 给横幅+回填）。
pub fn execute(body: &str) -> (u16, String) {
    let now = now_epoch_secs();
    let finish = |status: u16,
                  target: &str,
                  action: &str,
                  effective: &str,
                  ok: bool,
                  bytes: usize,
                  detail: &str,
                  post_screen: &str| {
        eprintln!(
            "[na-server] {}",
            log_line(now, target, action, effective, ok, bytes, detail)
        );
        (
            status,
            resp_json_with_screen(ok, target, action, effective, detail, post_screen),
        )
    };
    let req = match parse_req(body) {
        Ok(r) => r,
        Err(d) => return finish(400, "", "", "", false, body.len(), &d, ""),
    };
    let (target, action, bytes) = (req.target.as_str(), req.action.as_str(), req.text.len());
    let rej = |detail: String| finish(200, target, action, action, false, bytes, &detail, "");
    // ① 登记表核验（每请求重读——表小，不缓存；改了表即时生效）
    let map_text = match std::fs::read_to_string(registry_path()) {
        Ok(t) => t,
        Err(e) => {
            return finish(
                500,
                target,
                action,
                action,
                false,
                bytes,
                &format!("登记表读取失败: {e}"),
                "",
            );
        }
    };
    let rows = parse_registry(&map_text);
    let row = match lookup(&rows, target) {
        Ok(r) => r,
        Err(d) => return rej(d),
    };
    // ② 活态核验
    let panes = match sh_out(
        "tmux",
        &["list-panes", "-a", "-F", "#{session_name}\t#{pane_pid}"],
    ) {
        Ok(t) => parse_panes(&t),
        Err(e) => {
            return finish(
                500,
                target,
                action,
                action,
                false,
                bytes,
                &format!("tmux 会话清点失败: {e}"),
                "",
            );
        }
    };
    let procs = match sh_out("ps", &["-eo", "pid=,ppid=,comm=,args="]) {
        Ok(t) => parse_ps(&t),
        Err(e) => {
            return finish(
                500,
                target,
                action,
                action,
                false,
                bytes,
                &format!("进程清点失败: {e}"),
                "",
            );
        }
    };
    if let Err(d) = check_alive(row, &panes, &procs) {
        return rej(d);
    }
    // ③ 键序与执行（Sleep 步走线程停顿——与 agent-send 的 sleep 同秒数）
    let (steps, effective, note) = match build_keys(req.action, &row.harness, target, &req.text) {
        Ok(x) => x,
        Err(d) => return finish(400, target, action, action, false, bytes, &d, ""),
    };
    for step in &steps {
        let out = match step {
            KeyStep::Cmd(args) => run_cmd(args),
            KeyStep::Sleep(ms) => {
                std::thread::sleep(std::time::Duration::from_millis(*ms));
                Ok(())
            }
        };
        if let Err(e) = out {
            return finish(500, target, action, action, false, bytes, &e, "");
        }
    }
    // ④ 判据③取证（BAR-252：按下后对方窗口真进入处理）
    let post_screen = capture_post_screen(target);
    finish(
        200,
        target,
        action,
        effective.as_str(),
        true,
        bytes,
        note.as_deref().unwrap_or(""),
        &post_screen,
    )
}
