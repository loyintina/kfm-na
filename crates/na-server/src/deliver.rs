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
//! ③ 键序——send＝字面文本＋Enter（agent-send 同款）；interrupt＝Escape；
//!    queue＝tianshu 发 M-Enter、kimi 降级 send（action_effective 透出，
//!    回执须能说「排队（已降级为发送）」——白露楼2①）；
//! ④ 留痕——na-server 日志一行（时刻/目标/动作/结果/字节数）＋响应 JSON
//!    （ok/target/action/action_effective/detail）回客户端做回执。
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

/// 键序计划（clippy type_complexity 要求抽别名，termview.rs:2019 同律）：
/// argv 向量序列 + 实际生效动作（降级透出）+ 降级备注
pub type KeyPlan = (Vec<Vec<String>>, Action, Option<String>);

/// ③ 键序（A 档纯函数）：argv 向量序列（execute 逐个 spawn tmux）。
/// `-t =<名>` 等号 = 精确匹配（tmux 目标语法默认前缀匹配，二字名互做
/// 前缀会投错窗）；`-l --` = 字面文本不解析键名。queue 在 kimi 上降级
/// send（note 透出降级，响应 action_effective=send）
pub fn build_keys(
    action: Action,
    harness: &Harness,
    target: &str,
    text: &str,
) -> Result<KeyPlan, String> {
    let t = format!("={target}");
    let send_plan = || {
        vec![
            vec![
                "send-keys".to_string(),
                "-t".to_string(),
                t.clone(),
                "-l".to_string(),
                "--".to_string(),
                text.to_string(),
            ],
            vec![
                "send-keys".to_string(),
                "-t".to_string(),
                t.clone(),
                "Enter".to_string(),
            ],
        ]
    };
    match action {
        Action::Send => {
            if text.is_empty() {
                return Err("空文本不投递（interrupt 请用 action=interrupt）".into());
            }
            Ok((send_plan(), Action::Send, None))
        }
        Action::Interrupt => Ok((
            vec![vec![
                "send-keys".to_string(),
                "-t".to_string(),
                t,
                "Escape".to_string(),
            ]],
            Action::Interrupt,
            None,
        )),
        Action::Queue => {
            if text.is_empty() {
                return Err("空文本不投递（排队也要先有形）".into());
            }
            match harness {
                Harness::Tianshu => Ok((
                    vec![
                        vec![
                            "send-keys".to_string(),
                            "-t".to_string(),
                            t.clone(),
                            "-l".to_string(),
                            "--".to_string(),
                            text.to_string(),
                        ],
                        vec![
                            "send-keys".to_string(),
                            "-t".to_string(),
                            t,
                            "M-Enter".to_string(),
                        ],
                    ],
                    Action::Queue,
                    None,
                )),
                // kimi 无排队键——降级发送，回执必须说得出口（白露楼2①）
                _ => Ok((
                    send_plan(),
                    Action::Send,
                    Some("kimi 无排队键——已降级为发送".to_string()),
                )),
            }
        }
    }
}

/// ④ 回执体（A 档纯函数）：五键全在，detail 空串不省略（客户端凭键认
/// 形状——与 sys_json「键永远在」同律）
pub fn resp_json(ok: bool, target: &str, action: &str, effective: &str, detail: &str) -> String {
    serde_json::json!({
        "ok": ok,
        "target": target,
        "action": action,
        "action_effective": effective,
        "detail": detail,
    })
    .to_string()
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

/// 薄 IO 胶（B 档；纯逻辑全在上面）：解析 → 登记表 → 活态 → 键序 →
/// 执行 → 留痕 → 回执。返回 (HTTP 状态码, 响应体)：
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
                  detail: &str| {
        eprintln!(
            "[na-server] {}",
            log_line(now, target, action, effective, ok, bytes, detail)
        );
        (status, resp_json(ok, target, action, effective, detail))
    };
    let req = match parse_req(body) {
        Ok(r) => r,
        Err(d) => return finish(400, "", "", "", false, body.len(), &d),
    };
    let (target, action, bytes) = (req.target.as_str(), req.action.as_str(), req.text.len());
    let rej = |detail: String| finish(200, target, action, action, false, bytes, &detail);
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
            );
        }
    };
    if let Err(d) = check_alive(row, &panes, &procs) {
        return rej(d);
    }
    // ③ 键序与执行
    let (cmds, effective, note) = match build_keys(req.action, &row.harness, target, &req.text) {
        Ok(x) => x,
        Err(d) => return finish(400, target, action, action, false, bytes, &d),
    };
    for cmd in &cmds {
        if let Err(e) = run_cmd(cmd) {
            return finish(500, target, action, action, false, bytes, &e);
        }
    }
    finish(
        200,
        target,
        action,
        effective.as_str(),
        true,
        bytes,
        note.as_deref().unwrap_or(""),
    )
}
