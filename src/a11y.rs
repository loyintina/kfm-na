//! a11y.rs — 无障碍服务 native 委托（BAR-246，MAIN0149 用户拍板「让 na
//! 拥有自己的无障碍权限」：无线调试掉线时 na 自己伸手把开关点开，
//! 不必等人——adb 自愈只能扫端口，开关被关时无口可扫，结构性地救不了）。
//!
//! 形态（承影 NA0175 楼2 口径）：**稳定 Java 薄壳 + native 委托**——
//! android/java/dev/kfm/na/KfmA11yService.java 类名一次定死（授权按
//! 包名+类名记账，类名不变 = 热更碰不到授权面，判据⑧结构性恒过），
//! 全部逻辑在本模块（.so 侧，热更可达）。首装必须整包（manifest 声明
//! ＋服务配置 xml 是系统组件，.so 热更变不出来），此后迭代走热更。
//!
//! 命令面（闸门通道 a11y-req，一行一命令、可多行顺序执行）：
//!   status                      服务 up/down＋root 可用性
//!   dump                        当前窗口节点树（含 WebView 尽力——待验项，
//!                               读不全的替代面 = adb 腿，别让整件卡在这）
//!   dump-log                    动作日志尾部经 offer_result 上传
//!   click-text <文本>           按 text/desc 命中节点，点其可点祖先
//!   click-id <完整资源 id>      按 viewIdResourceName 命中
//!   click-point <x> <y>         dispatchGesture 坐标点（minSdk 24 可达）
//!   scroll-fwd / scroll-back    第一个可滚节点前滚/回滚
//!   set-text <文本>             焦点/首个可编辑节点 ACTION_SET_TEXT
//!   global back|home|recents|notifications|quick-settings
//! 「自己点开无线调试」这类宏不硬编码——机型路径各异，服务器侧用原语
//! 编排序列（卡萝配套脚本持有），app 只供原语。
//!
//! 动作日志（白露 MAIN0149 楼1 补款④「有日志可查」落实物）：
//!   <dir>/a11y-log.jsonl  一行一动作 {"ts":..,"op":..,"arg":..,"ok":..,
//!                         "err":..,"dur_ms":..}；512KB 封顶半切轮转；
//!   <dir>/a11y-status     覆盖写一行状态快照（up＋最近动作摘要）；
//!   <dir>/a11y-res        本批逐行结果（并 offer_result 上传）。
//!
//! 分层：解析/日志/轮转 = A 档纯逻辑（tests/a11y_spec.rs 钉+变异）；
//! Java 服务类/manifest/JNI 胶水 = B 档（判卷 = 真机行为，卡萝七条+⑧）。

use std::path::Path;

/// 通道触发文件名（gate_poller 与 na-server gateq 白名单同票互指）
pub const REQ_FILE: &str = "a11y-req";
/// 动作审计日志（jsonl，封顶半切）
pub const LOG_FILE: &str = "a11y-log.jsonl";
/// 状态快照（覆盖写一行）
pub const STATUS_FILE: &str = "a11y-status";
/// 本批结果（offer_result 上传名 = REQ_FILE）
pub const RES_FILE: &str = "a11y-res";
/// 日志封顶（字节）；超帽保留后半（半切轮转——保最新，弃最旧）
const LOG_CAP: usize = 512 * 1024;

/// 全局动作。sel() 的序号 = Java 侧 switch 的选择子，由 Java 映射到
/// AccessibilityService.GLOBAL_ACTION_* 命名常量——常数值不出 .so
/// （写错数值的静默错面从结构上消失，javac 核常量名）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalAction {
    Back,
    Home,
    Recents,
    Notifications,
    QuickSettings,
}

impl GlobalAction {
    /// Java 侧 switch 选择子（0..=4，与 KfmA11yService.a11yGlobal 的
    /// case 序同表——改动两边同票）
    pub fn sel(self) -> i32 {
        match self {
            GlobalAction::Back => 0,
            GlobalAction::Home => 1,
            GlobalAction::Recents => 2,
            GlobalAction::Notifications => 3,
            GlobalAction::QuickSettings => 4,
        }
    }
}

/// 解析产物（A 档钉面）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A11yCmd {
    Status,
    Dump,
    DumpLog,
    ClickText(String),
    ClickId(String),
    ClickPoint(i32, i32),
    ScrollFwd,
    ScrollBack,
    SetText(String),
    Global(GlobalAction),
}

impl A11yCmd {
    /// 审计日志的 op 字段
    pub fn op(&self) -> &'static str {
        match self {
            A11yCmd::Status => "status",
            A11yCmd::Dump => "dump",
            A11yCmd::DumpLog => "dump-log",
            A11yCmd::ClickText(_) => "click-text",
            A11yCmd::ClickId(_) => "click-id",
            A11yCmd::ClickPoint(..) => "click-point",
            A11yCmd::ScrollFwd => "scroll-fwd",
            A11yCmd::ScrollBack => "scroll-back",
            A11yCmd::SetText(_) => "set-text",
            A11yCmd::Global(_) => "global",
        }
    }

    /// 审计日志的 arg 字段
    pub fn arg(&self) -> String {
        match self {
            A11yCmd::ClickText(t) | A11yCmd::ClickId(t) | A11yCmd::SetText(t) => t.clone(),
            A11yCmd::ClickPoint(x, y) => format!("{x},{y}"),
            A11yCmd::Global(g) => format!("{g:?}"),
            _ => String::new(),
        }
    }
}

/// 解析一行命令（trim 后首 token = 操作名，其余 = 参数）。
/// 空行/注释行（# 起头）由批处理层跳过，此处视为错误输入。
/// Err 文本进审计日志——拒因必须人话可读。
pub fn parse_a11y_line(line: &str) -> Result<A11yCmd, String> {
    let l = line.trim();
    if l.is_empty() {
        return Err("空行".into());
    }
    let (op, rest) = match l.find(char::is_whitespace) {
        Some(i) => (&l[..i], l[i..].trim()),
        None => (l, ""),
    };
    let need_arg = |what: &str| -> Result<String, String> {
        if rest.is_empty() {
            Err(format!("{op} 缺参数（{what}）"))
        } else {
            Ok(rest.to_string())
        }
    };
    let no_arg = |cmd: A11yCmd| -> Result<A11yCmd, String> {
        if rest.is_empty() {
            Ok(cmd)
        } else {
            Err(format!("{op} 不带参数（多给了：{rest}）"))
        }
    };
    match op {
        "status" => no_arg(A11yCmd::Status),
        "dump" => no_arg(A11yCmd::Dump),
        "dump-log" => no_arg(A11yCmd::DumpLog),
        "scroll-fwd" => no_arg(A11yCmd::ScrollFwd),
        "scroll-back" => no_arg(A11yCmd::ScrollBack),
        "click-text" => need_arg("目标文本").map(A11yCmd::ClickText),
        "click-id" => need_arg("完整资源 id").map(A11yCmd::ClickId),
        "set-text" => need_arg("要输入的文本").map(A11yCmd::SetText),
        "click-point" => {
            let mut it = rest.split_whitespace();
            let (x, y, extra) = (it.next(), it.next(), it.next());
            match (x, y, extra) {
                (Some(x), Some(y), None) => {
                    let x: i32 = x
                        .parse()
                        .map_err(|_| format!("click-point x 非整数：{x}"))?;
                    let y: i32 = y
                        .parse()
                        .map_err(|_| format!("click-point y 非整数：{y}"))?;
                    Ok(A11yCmd::ClickPoint(x, y))
                }
                _ => Err("click-point 要正好两个整数参数（x y）".into()),
            }
        }
        "global" => match rest {
            "back" => Ok(A11yCmd::Global(GlobalAction::Back)),
            "home" => Ok(A11yCmd::Global(GlobalAction::Home)),
            "recents" => Ok(A11yCmd::Global(GlobalAction::Recents)),
            "notifications" => Ok(A11yCmd::Global(GlobalAction::Notifications)),
            "quick-settings" => Ok(A11yCmd::Global(GlobalAction::QuickSettings)),
            _ => Err(format!(
                "global 未知动作：{rest}（可选 back/home/recents/notifications/quick-settings）"
            )),
        },
        _ => Err(format!("未知命令：{op}")),
    }
}

/// json 字符串转义（审计行单源——字段值全走它）
fn json_esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 审计日志行（字段序固定：ts/op/arg/ok/err/dur_ms——白露补款④的
/// 「查得到」靠格式稳定兑现）
pub fn log_line(ts_ms: u64, op: &str, arg: &str, ok: bool, err: &str, dur_ms: u64) -> String {
    format!(
        "{{\"ts\":{ts_ms},\"op\":\"{}\",\"arg\":\"{}\",\"ok\":{},\"err\":\"{}\",\"dur_ms\":{dur_ms}}}",
        json_esc(op),
        json_esc(arg),
        ok,
        json_esc(err),
    )
}

/// 追加审计日志；超帽半切（保留后半——最新优先）。IO 失败只报账不炸。
pub fn append_log(dir: &str, line: &str) {
    let p = Path::new(dir).join(LOG_FILE);
    if let Ok(meta) = std::fs::metadata(&p)
        && meta.len() as usize > LOG_CAP
        && let Ok(body) = std::fs::read(&p)
    {
        // 半切：从后半里找第一个换行对齐到行首（不产出半行）
        let half = body.len() / 2;
        let start = body[half..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(half, |i| half + i + 1);
        if std::fs::write(&p, &body[start..]).is_err() {
            crate::report::report("a11y", "审计日志半切重写失败（继续追加）");
        }
    }
    use std::io::Write as _;
    let r = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(e) = r {
        crate::report::report("a11y", &format!("审计日志写入失败: {e}"));
    }
}

/// 覆盖写状态快照（一行；up 位 + 最近动作摘要）
pub fn write_status(dir: &str, line: &str) {
    let p = Path::new(dir).join(STATUS_FILE);
    if let Err(e) = std::fs::write(&p, format!("{line}\n")) {
        crate::report::report("a11y", &format!("状态快照写入失败: {e}"));
    }
}

/// 读触发文件（原子消费：读走即删，同 web-req 口径）
pub fn take_req(dir: &str) -> Option<String> {
    let p = Path::new(dir).join(REQ_FILE);
    let s = std::fs::read_to_string(&p).ok()?;
    let _ = std::fs::remove_file(&p);
    if s.trim().is_empty() { None } else { Some(s) }
}

/// 服务在线位（nativeA11yUp 维护——系统 bind/unbind 的真账）
static SERVICE_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// UI/报表读数：服务当前在线（本进程口径）
pub fn service_up() -> bool {
    SERVICE_UP.load(std::sync::atomic::Ordering::Relaxed)
}

/// 值守循环消费（gate watcher 每 300ms 一行调用）：一批 = 逐行解析执行，
/// 每行进审计日志，批尾落 a11y-res 并 offer_result 上传。任何一步失败
/// 只落账不中断本批后续行（坏行不毒整批）。
pub fn a11y_check(dir: &str) {
    let Some(body) = take_req(dir) else { return };
    let mut results = String::new();
    let mut last = String::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let t0 = std::time::Instant::now();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let (op, arg, res) = match parse_a11y_line(line) {
            Ok(A11yCmd::DumpLog) => {
                // 日志上传：读尾部 64KB 进本批结果（审计原件留在机上不动）
                let p = Path::new(dir).join(LOG_FILE);
                match std::fs::read(&p) {
                    Ok(b) => {
                        let n = b.len().min(64 * 1024);
                        let t = String::from_utf8_lossy(&b[b.len() - n..]).into_owned();
                        (
                            "dump-log",
                            String::new(),
                            Ok(format!("日志尾部 {} 字符", t.len())),
                        )
                    }
                    Err(e) => ("dump-log", String::new(), Err(format!("读日志失败: {e}"))),
                }
            }
            Ok(cmd) => {
                let (op, arg) = (cmd.op(), cmd.arg());
                (op, arg, exec(&cmd))
            }
            Err(e) => ("parse", line.to_string(), Err(e)),
        };
        let dur = t0.elapsed().as_millis() as u64;
        let (ok, err, out) = match &res {
            Ok(s) => (true, String::new(), s.clone()),
            Err(e) => (false, e.clone(), format!("ERR {e}")),
        };
        append_log(dir, &log_line(ts, op, &arg, ok, &err, dur));
        crate::report::report(
            "a11y",
            &format!(
                "{op} {} → {}",
                if arg.is_empty() { "" } else { &arg },
                out.chars().take(120).collect::<String>()
            ),
        );
        results.push_str(&format!("{line} => {out}\n"));
        last = format!("{op} {}", if ok { "ok" } else { "ERR" });
    }
    write_status(
        dir,
        &format!("up={} last={}", if service_up() { 1 } else { 0 }, last),
    );
    let res_path = Path::new(dir).join(RES_FILE);
    if std::fs::write(&res_path, &results).is_ok() {
        crate::gate_poller::offer_result(REQ_FILE, &res_path);
    }
}

// ---- JNI 委托面（安卓真身；host 全落空转——考题在 host 跑通解析/日志/批处理）----

#[cfg(target_os = "android")]
mod imp {
    use super::A11yCmd;
    use jni::objects::{Global, JClass, JValue};
    use std::sync::Mutex;

    /// 服务登记包：nativeA11yUp（Java 线程上下文，类装载无坑——app 类在
    /// 纯 native 线程 FindClass 不到，jclass 全局引用只能在这一刻存）
    pub struct A11yJni {
        pub vm: jni::JavaVM,
        pub cls: Global<JClass<'static>>,
    }
    static JNI_REG: Mutex<Option<A11yJni>> = Mutex::new(None);

    /// 服务 bind/unbind 回调登记（KfmA11yService.onServiceConnected /
    /// onUnbind 调）。up=false 时保留登记包（类引用不失效），只翻状态位。
    pub fn register(vm: jni::JavaVM, cls: Global<JClass<'static>>, up: bool) {
        *JNI_REG.lock().unwrap() = Some(A11yJni { vm, cls });
        super::SERVICE_UP.store(up, std::sync::atomic::Ordering::Relaxed);
        crate::report::report(
            "a11y",
            if up {
                "无障碍服务上线（已绑定）"
            } else {
                "无障碍服务下线（解绑）"
            },
        );
    }

    /// 命令执行（值守线程上下文）：持锁做完整次 JNI——register 只在
    /// Java 线程跑、调用毫秒级，无争用问题；锁保 Global 引用在执行期
    /// 不被换走
    pub fn exec(cmd: &A11yCmd) -> Result<String, String> {
        let reg = JNI_REG.lock().unwrap();
        let Some(j) = reg.as_ref() else {
            return Err("服务未登记（无障碍未启用或本进程未见绑定）".into());
        };
        if !super::service_up() {
            return Err("服务当前不在线（被系统停用，待自恢复或重授权）".into());
        }
        j.vm.attach_current_thread(|env| -> jni::errors::Result<String> {
            let r = match cmd {
                A11yCmd::Status => env.call_static_method(
                    &j.cls,
                    jni::jni_str!("a11yStatus"),
                    jni::jni_sig!(() -> java.lang.String),
                    &[],
                ),
                A11yCmd::Dump => env.call_static_method(
                    &j.cls,
                    jni::jni_str!("a11yDumpTree"),
                    jni::jni_sig!(() -> java.lang.String),
                    &[],
                ),
                A11yCmd::ClickText(t) => {
                    let a = env.new_string(t)?;
                    env.call_static_method(
                        &j.cls,
                        jni::jni_str!("a11yClickText"),
                        jni::jni_sig!((java.lang.String) -> java.lang.String),
                        &[JValue::Object(&a)],
                    )
                }
                A11yCmd::ClickId(t) => {
                    let a = env.new_string(t)?;
                    env.call_static_method(
                        &j.cls,
                        jni::jni_str!("a11yClickId"),
                        jni::jni_sig!((java.lang.String) -> java.lang.String),
                        &[JValue::Object(&a)],
                    )
                }
                A11yCmd::ClickPoint(x, y) => env.call_static_method(
                    &j.cls,
                    jni::jni_str!("a11yClickPoint"),
                    jni::jni_sig!((int, int) -> java.lang.String),
                    &[JValue::Int(*x), JValue::Int(*y)],
                ),
                A11yCmd::ScrollFwd => env.call_static_method(
                    &j.cls,
                    jni::jni_str!("a11yScroll"),
                    jni::jni_sig!((boolean) -> java.lang.String),
                    &[JValue::Bool(true)],
                ),
                A11yCmd::ScrollBack => env.call_static_method(
                    &j.cls,
                    jni::jni_str!("a11yScroll"),
                    jni::jni_sig!((boolean) -> java.lang.String),
                    &[JValue::Bool(false)],
                ),
                A11yCmd::SetText(t) => {
                    let a = env.new_string(t)?;
                    env.call_static_method(
                        &j.cls,
                        jni::jni_str!("a11ySetText"),
                        jni::jni_sig!((java.lang.String) -> java.lang.String),
                        &[JValue::Object(&a)],
                    )
                }
                A11yCmd::Global(g) => env.call_static_method(
                    &j.cls,
                    jni::jni_str!("a11yGlobal"),
                    jni::jni_sig!((int) -> java.lang.String),
                    &[JValue::Int(g.sel())],
                ),
                A11yCmd::DumpLog => unreachable!("dump-log 在批处理层已消化"),
            };
            let out = r?.l()?;
            if out.is_null() {
                return Ok("ERR Java 返回 null".into());
            }
            let js = unsafe { jni::objects::JString::from_raw(env, out.as_raw() as _) };
            js.try_to_string(env)
        })
        .map_err(|e| format!("JNI: {e}"))
    }
}

#[cfg(target_os = "android")]
fn exec(cmd: &A11yCmd) -> Result<String, String> {
    imp::exec(cmd)
}

#[cfg(not(target_os = "android"))]
fn exec(_cmd: &A11yCmd) -> Result<String, String> {
    Err("host 无无障碍服务（仅安卓可用）".into())
}

/// dev.kfm.na.KfmA11yService.nativeA11yUp —— 服务绑定/解绑回调。
/// 纪律同 ime_bridge：JNI 回调线程绝不 panic 跨 FFI 边界。
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmA11yService_nativeA11yUp(
    mut env: jni::EnvUnowned,
    class: jni::objects::JClass,
    up: jni::sys::jboolean,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let vm = env.get_java_vm()?;
        let cls = env.new_global_ref(&class)?;
        imp::register(vm, cls, up);
        Ok(())
    })
    .resolve_with::<jni::errors::LogContextErrorAndDefault, _>(|| "in nativeA11yUp".to_string());
}

// ---- UI 面（系统管理页状态行＋跳授权页；调用点全在 android_app.rs）----

/// 系统管理页「无障碍服务」行的值文本（组件池登记 symbol = 本函数名）。
/// 授权读数 = Settings.Secure enabled_accessibility_services 是否含本包
/// （读取无需权限）；在线读数 = 本进程服务绑定位。
#[cfg(target_os = "android")]
pub fn a11y_ui_status_text(app: Option<&winit::platform::android::activity::AndroidApp>) -> String {
    let authed = app.and_then(read_authed).unwrap_or(false);
    match (authed, service_up()) {
        (true, true) => "已启用·在线".into(),
        (true, false) => "已启用·离线（被停待自恢复）".into(),
        (false, _) => "未启用（点按去授权）".into(),
    }
}

#[cfg(not(target_os = "android"))]
pub fn a11y_ui_status_text() -> String {
    "未知（host）".into()
}

#[cfg(target_os = "android")]
fn read_authed(app: &winit::platform::android::activity::AndroidApp) -> Option<bool> {
    use jni::objects::{JObject, JString, JValue};
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
    vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
        let act = unsafe { JObject::from_raw(env, app.activity_as_ptr() as *mut _) };
        let resolver = env
            .call_method(
                &act,
                jni::jni_str!("getContentResolver"),
                jni::jni_sig!(() -> android.content.ContentResolver),
                &[],
            )?
            .l()?;
        let key = env.new_string("enabled_accessibility_services")?;
        let val = env
            .call_static_method(
                jni::jni_str!("android/provider/Settings$Secure"),
                jni::jni_str!("getString"),
                jni::jni_sig!(
                    (android.content.ContentResolver, java.lang.String) -> java.lang.String
                ),
                &[JValue::Object(&resolver), JValue::Object(&key)],
            )?
            .l()?;
        if val.is_null() {
            return Ok(false);
        }
        let js = unsafe { JString::from_raw(env, val.as_raw() as _) };
        Ok(js.try_to_string(env)?.contains("dev.kfm.na"))
    })
    .ok()
}

/// 跳系统无障碍设置页（用户原话「右上角系统管理设置页加入口」的动作半）：
/// rust-direct Intent，与 install 引导腿同款——不依赖 Java 皮新方法。
#[cfg(target_os = "android")]
pub fn open_settings_page(app: &winit::platform::android::activity::AndroidApp) {
    use jni::objects::{JObject, JValue};
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
    let r = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let act = unsafe { JObject::from_raw(env, app.activity_as_ptr() as *mut _) };
        let action = env.new_string("android.settings.ACCESSIBILITY_SETTINGS")?;
        let intent = env.new_object(
            jni::jni_str!("android/content/Intent"),
            jni::jni_sig!((java.lang.String) -> void),
            &[JValue::Object(&action)],
        )?;
        // FLAG_ACTIVITY_NEW_TASK
        env.call_method(
            &intent,
            jni::jni_str!("addFlags"),
            jni::jni_sig!((int) -> void),
            &[JValue::Int(0x1000_0000)],
        )?;
        env.call_method(
            &act,
            jni::jni_str!("startActivity"),
            jni::jni_sig!((android.content.Intent) -> void),
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    });
    match r {
        Ok(()) => crate::report::report("a11y", "已跳系统无障碍设置页"),
        Err(e) => crate::report::report("a11y", &format!("跳无障碍设置页失败: {e}")),
    }
}
