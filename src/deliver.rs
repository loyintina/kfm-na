//! deliver.rs — 输入栏投递路由（BAR-252，MAIN0168 定位重定向第一步，追踪信
//! NA0181；设计 = 楼1，裁决 = 白露楼2 四点全裁）。
//!
//! 发送口唯一分叉（D9 同源不动——触摸发送钮/IME Enter/闸门注入 submit
//! 都进同一条发送闭包，分叉在闭包里）：
//! - 页＝终页 ∧ 面板栈顶为空 ∧ 活跃腿附着名 S 在 ∧ 投递面已装配 →
//!   **窗路**：spawn 线程经共享腿 HTTP POST /api/deliver（na-server 核验
//!   投递一处成案），瞬时返回契约同 AI 脑；
//! - 否则 → **原 AI 路由**。裸 shell 归 AI 须不静默（白露楼2④）：栏内
//!   一句瞬态小字解释分叉；AI 全屏页/面板开着归 AI 无提示（人没在窗上）。
//!
//! 回执（白露楼2①②）：
//! - 投成 = 画布回显即凭 ＋ 栏内瞬态小字「→ 窗·发送」（降级透出
//!   「排队（已降级为发送）」——action_effective 驱动）；
//! - 拒投 = 终端横幅一行（切换横幅同款机制）写**服务端 detail 原文**
//!   （原因＋出路，客户端不再造文案——单源纪律）＋草稿回填栏不吞字；
//! - 不走 toast（无 Looper 线程 NPE 已有定论）。
//!
//! 词表与 crates/na-server/src/deliver.rs 同源两份（action 三形），改要
//! 同票。纯函数面（decide/request_json/parse_resp/hint_for）钉在
//! tests/deliver_spec.rs。

use crate::ai_presence::{Page, Panel};

/// 投递动作（与服务端 deliver.rs Action 同词）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Send,
    Interrupt,
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

    /// 回执小字用的中文名
    pub fn zh(self) -> &'static str {
        match self {
            Action::Send => "发送",
            Action::Interrupt => "打断",
            Action::Queue => "排队",
        }
    }
}

/// 发送路由判定结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// 窗路：POST /api/deliver 投到附着会话（值 = 会话名）
    Window(String),
    /// 原 AI 路由；Some = 栏内瞬态提示（分叉须可解释，白露楼2④）
    Ai(Option<String>),
}

/// 路由判定（A 档纯函数，钉在 deliver_spec）：
/// 终页裸奔 ∧ 附着名在 ∧ 投递面已装配 → 窗路；其余 AI 旧路。
/// page/top 来自 ai_presence 快照；attached 来自 SessionRouter::active_attached
/// （附着了 tmux 会话 = 会话名，裸 shell = None——BAR-238 同源读数）。
/// 附着名在哪个腿不问：本地腿 ssh attach 与远程腿 ws attach 看的都是
/// 服务器 tmux 窗，投递到窗就是投递到人正在看的地方。
pub fn decide(page: Page, top: Option<Panel>, attached: Option<&str>, configured: bool) -> Route {
    // AI 全屏页输入栏本归 AI；面板开着 = 人没在看窗——都无提示归 AI
    if page != Page::Terminal || top.is_some() {
        return Route::Ai(None);
    }
    // 后端非 na-server / 投递面未装配 = 旧行为零变化（不打扰）
    if !configured {
        return Route::Ai(None);
    }
    match attached {
        Some(name) => Route::Window(name.to_string()),
        // 裸 shell 无从核验（NA0181 楼1：保守归 AI），但须解释分叉
        None => Route::Ai(Some("当前窗是裸 shell——本句发往 AI 页".to_string())),
    }
}

/// 投递面配置（servers.json 条目；gate_poller 同一份）。na-server 后端
/// 装配时登记一次（android_app 起闸门轮询处）
static CTX: std::sync::Mutex<Option<crate::settings::ServerEntry>> = std::sync::Mutex::new(None);

/// 登记投递面配置（幂等——设置重载重复调覆盖）
pub fn configure(srv: &crate::settings::ServerEntry) {
    *CTX.lock().unwrap() = Some(srv.clone());
}

/// 投递面已装配？
pub fn configured() -> bool {
    CTX.lock().unwrap().is_some()
}

/// 现状路由（发送闭包与 bar-inject 同源的读数口）：ai_presence 快照 +
/// 路由附着名 + 投递面装配位。状态核未登记（装配未完成）= AI 旧路不打扰
pub fn route_now() -> Route {
    let Some(ai) = crate::gate::ai_presence_handle() else {
        return Route::Ai(None);
    };
    let snap = ai.snap(crate::report::boot_ms() as u64);
    let attached = crate::gate::gate_router_handle()
        .and_then(|r| r.lock().unwrap().active_attached().map(|s| s.to_string()));
    decide(snap.page, snap.top, attached.as_deref(), configured())
}

/// 请求体（A 档纯函数）：JSON 四键 v/target/action/text
pub fn request_json(target: &str, action: Action, text: &str) -> String {
    serde_json::json!({
        "v": 1,
        "target": target,
        "action": action.as_str(),
        "text": text,
    })
    .to_string()
}

/// 服务端回执（五键里的三面：ok/action_effective/detail 驱动回执形态）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resp {
    pub ok: bool,
    pub action_effective: String,
    pub detail: String,
}

/// 解析回执（A 档纯函数）：坏 JSON/缺键 = None（按失败臂走横幅）
pub fn parse_resp(body: &str) -> Option<Resp> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    Some(Resp {
        ok: v.get("ok")?.as_bool()?,
        action_effective: v.get("action_effective")?.as_str()?.to_string(),
        detail: v.get("detail")?.as_str()?.to_string(),
    })
}

/// 投成小字（A 档纯函数）：「→ 窗·发送」；请求动作与实际动作不同 =
/// 服务端降级（kimi 无排队键），透出「排队（已降级为发送）」——白露
/// 楼2①「回执要能说，否则用户以为排上了」
pub fn hint_for(target: &str, action: Action, effective: &str) -> String {
    match Action::parse(effective) {
        Some(eff) if eff == action => format!("→ {target}·{}", action.zh()),
        Some(eff) => format!("→ {target}·{}（已降级为{}）", action.zh(), eff.zh()),
        None => format!("→ {target}·{}", action.zh()),
    }
}

/// 阻塞版投递（须在自备线程调——瞬时返回契约由调用方的 spawn 保证）：
/// 共享腿槽取当前数据腿连接，QUIC 桥直开流打 POST /api/deliver
/// （与闸门轮询同一载体，BAR-233 根治形制）。腿不在/超时/坏回执 = Err
pub fn deliver_now(target: &str, action: Action, text: &str) -> Result<Resp, String> {
    let srv = CTX.lock().unwrap().clone().ok_or("投递面未装配")?;
    let psk = crate::tunnel::parse_pin(&srv.quic.psk).ok_or("QUIC psk 缺件")?;
    let body = request_json(target, action, text);
    let raw = crate::gate_poller::http_roundtrip(
        &crate::tunnel::data_conn_slot(),
        &psk,
        srv.gate_port,
        &srv.quic.device_id,
        "POST",
        "/api/deliver",
        body.as_bytes(),
        &srv.ssh.host,
    )?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    parse_resp(&text).ok_or_else(|| format!("回执坏形: {}", &text[..text.len().min(120)]))
}

/// 投递线程（发送闭包/值守线程调它即走）。回执：成 = 栏内瞬态小字；
/// 拒/败 = 终端横幅（拒投写服务端 detail 原文——原因＋出路）＋草稿回填
/// 不吞字；两端皆 report 留痕。res_name Some = 结果另落
/// DUMP_DIR/<name>（bar-inject 判卷件——值守线程不阻塞等往返，
/// 判卷读文件拿结局）
pub fn deliver_spawn(target: String, action: Action, text: String, res_name: Option<&'static str>) {
    std::thread::spawn(move || {
        let out = deliver_now(&target, action, &text);
        let line = match &out {
            Ok(r) if r.ok => {
                if let Some(bar) = crate::gate::input_bar_handle() {
                    bar.set_hint(&hint_for(&target, action, &r.action_effective));
                }
                format!(
                    "投成 target={target} action={} effective={}",
                    action.as_str(),
                    r.action_effective
                )
            }
            Ok(r) => {
                banner(&format!("投递被拒——{}", r.detail));
                refill(&text);
                format!(
                    "拒投 target={target} action={} detail={}",
                    action.as_str(),
                    r.detail
                )
            }
            Err(e) => {
                banner(&format!("投递失败——{e}"));
                refill(&text);
                format!("失败 target={target} action={} err={e}", action.as_str())
            }
        };
        crate::report::report("deliver", &line);
        if let Some(name) = res_name {
            std::fs::write(
                std::path::PathBuf::from(crate::gate::DUMP_DIR).join(name),
                format!("{line}\n"),
            )
            .ok();
        }
    });
}

/// 拒投/失败横幅（切换横幅同款机制：喂进终端网格一行——本地显示，
/// 不进远端 pane）。黄底字区分于切换横幅的青
fn banner(msg: &str) {
    if let Some(t) = crate::gate::dump_term_handle() {
        let line = format!("\r\n\x1b[33m[kfm-na: {msg}]\x1b[0m\r\n");
        t.lock().unwrap().feed(line.as_bytes());
    }
}

/// 草稿回填栏不吞字（白露楼2② 不可丢弃）
fn refill(text: &str) {
    if !text.is_empty()
        && let Some(bar) = crate::gate::input_bar_handle()
    {
        bar.insert_text(text);
    }
}
