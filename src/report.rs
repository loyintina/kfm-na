//! report.rs — 飞鸽传书：实拍现场回传通道
//!
//! 背景（2026-08-13 实拍闪退）：手机走蜂窝/WiFi NAT，服务器 adb 反连不回去，
//! logcat 拿不到。APK 的 panic 与启动里程碑改走 HTTP POST 直报 kfmv4 服务器
//! （手机既然能下载 APK，就一定能回传），落盘（服务器侧 NA_REPORT_LOG，
//! 缺省 = 进程 cwd 下的 field-reports.log）。
//!
//! 架构（第三版，血泪教训见下）：report() 只入队不触网，专用后台线程冲洗。
//! - v1 fire-and-forget：单条丢失无法区分「没跑到」与「丢了」
//! - v2 主线程同步等应答：白屏被网络 RTT 拉长；connect 无超时可卡分钟级；
//!   服务器 502 也被当「送达」——三条全中过（2026-08-13 实拍）
//! - v3 本版：入队即返回；后台线程冲洗，connect_timeout 2s、只认 HTTP 200、
//!   失败留队 1s 后重试，宁重复不丢
//!
//! 铁律：本通道任何失败都必须吞掉——上报通道自己炸了就是二次事故。

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

/// 服务器地址（2026-09-23 拓扑变更，BAR-139）：na-server 直连 127.0.0.1:9021
/// ——走 na 自持隧道（看门狗双腿维护，Termux 冻不冻都不看脸色）。
/// 旧拓扑 8021 靠 Termux ssh -L 代维，Termux 一冻结报表即断供
/// （2026-09-23 实拍：应用/隧道全活，心跳在 logcat 照打，服务器侧
/// 断更 26 分钟——8021 成单点）。旧拓扑 nginx 80 公网反代已废弃。
/// PATH 保留 /kfmv4 前缀：na-server 前缀别名也认（httpd.rs 双挂载）
pub const SERVER_ADDR: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
    9021,
);
pub const HOST_HEADER: &str = "127.0.0.1:9021";
pub const PATH: &str = "/kfmv4/api/na-report";

static SENDER: Mutex<Option<Sender<String>>> = Mutex::new(None);

/// 设备/实例标识（BAR-134）：field-reports.log 是多设备混流（手机 + redroid
/// 同写一个文件——BAR-126 误判「多实例抢口」的根因），每行末尾挂 [arch/pid]。
/// **BAR-235 加设备维**（2026-10-08）：BAR-134 的 [arch/pid] 只分「设备族 ×
/// 进程」，**两台同为 aarch64 的真机混流后与「一台机两个实例」完全同形**——
/// 2026-10-07 乙案判卷据此把 Neo11（新核）+ neo9（卡萝实验场旧核）误读成
/// 「单机双实例」，白露 12 楼由用户口述才纠正。故形态扩为 **[arch/dev/pid]**：
/// dev = 机型（`ro.product.model`，见下）＋**来源档**，退化链兜底，恒非空。
/// 放 msg 末尾 = 落盘行尾。**口径变更**：旧 `\[arch/pid\]` 精确尾锚 grep 改按
/// `\[arch/[^]]*/pid\]` 或直接按 pid 段匹配（仓内 scripts/ 已清点无该锚）。
///
/// **BAR-273 第二批（MAIN0192 楼185 悬铃裁 A 甲）**：dev 段写成「值@来源档」，
/// 让每条读数的来源自证、机器可判「谁可跨机采信」；**旧行无 @ 档 ⇒ 永久按
/// unknown 处理，不跨机采信**。dev 段是**实例（装置）位标识，不是设备身份**：
/// 它随落点与重装而变，不许当序列号用；序列号不裸存。
static INSTANCE_TAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// 设备位缓存（BAR-235）：整进程只解析一次（文件 IO 不在上报路径上重复做）
static DEV_TAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// 设备位覆盖口（判卷/取证用：`NA_DEVICE_TAG=Neo11` 可钉死机型名，免猜）
const DEV_OVERRIDE_ENV: &str = "NA_DEVICE_TAG";

/// 来源档四档（BAR-273 第二批；楼185 悬铃裁 A 甲「带来源档」＋楼186 白露
/// 「先只留 model／fallback 两档，serial 等测出再加且须附读数」）。采信口径：
/// model＝真读数，可跨机采信；env＝判卷手钉（NA_DEVICE_TAG），只在本轮判卷内
/// 有效——**必须单独成档**，否则手钉值会冒充真读数，正是本件要防的误读；
/// fallback＝持久短 id，同机恒定但**不是设备身份**；unknown＝全链失败，不采信。
/// **serial 暂不采集**：未测出可读路径前不加档，也不裸存。
pub const TIER_ENV: &str = "env";
pub const TIER_MODEL: &str = "model";
pub const TIER_FALLBACK: &str = "fallback";
pub const TIER_UNKNOWN: &str = "unknown";

/// 短 id 落点（BAR-273 第二批，楼193 悬铃更正）：**共享存储的 kfm-na-id/**——
/// 与 kfm-na-keys/ 刻意分开，一目录一用途（楼193 原话）。放私有目录时卸载/重装
/// 即换新值，「同机恒定」必然不成立（BAR-273 判据第二条），且是静默地变——
/// 不报错，只让跨机读数失去可比性。**wipe 演练只许删这一个件**，不碰 usr/hot。
const DEV_ID_PATH: &str = "/storage/emulated/0/kfm-na-id/device-tag";
/// 旧落点（私有目录）：迁移源，迁完即删——两份各走一路比一份没有更坏。
const DEV_ID_PATH_LEGACY: &str = "/data/data/dev.kfm.na/files/device-tag";

/// 机型属性文件候选（Android 9+ 把 ro.product.* 拆进 vendor/odm；
/// `/system/build.prop` 只留一部分——四处都试，先中先用）
const PROP_FILES: [&str; 6] = [
    "/system/build.prop",
    "/vendor/build.prop",
    "/odm/etc/build.prop",
    "/system/etc/prop.default",
    // BAR-273（2026-10-18 清和裁「先修读取」；全库实证：只出现过两枚 8 位随机
    // 退化 id、**无一台读到机型**）：Android 10+ 常把机型拆进 product/system_ext，
    // 旧四处漏掉这两处 ⇒ 永远落到退化第二档。
    "/product/build.prop",
    "/system_ext/build.prop",
];

/// 机型键候选（BAR-273）：Android 10+ 常见 `ro.product.vendor.model` /
/// `ro.product.odm.model` / `ro.product.system.model` 四处分家；旧实现只认
/// 前两个裸键 ⇒ 大量真机读不到机型（全库实证：0 台读到机型）。
const MODEL_KEYS: [&str; 6] = [
    "ro.product.model",
    "ro.product.vendor.model",
    "ro.product.odm.model",
    "ro.product.system.model",
    "ro.product.device",
    "ro.product.vendor.device",
];

/// 从 build.prop 文本里取机型（纯函数，A 档可判）：只认指定键；值去空白、
/// **剜掉行内注释**（`ro.product.model=Neo11 # vivo` → `Neo11`——整行 `#`
/// 注释天然过不了键名比对，真正要防的是行内尾注被当成机型字符吞进标识）；
/// 剜后为空 = 未命中（继续找下一处）。先 model 后 device 的两轮扫描由调用方
/// 决定（本函数只认一个键）
pub fn parse_prop_key(props: &str, key: &str) -> Option<String> {
    for line in props.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.split('#').next().unwrap_or("").trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

/// 机型串消毒（纯函数，A 档可判）：**只留字母数字与 `-`/`_`**，去掉空白与
/// 标点（`Neo 11` → `Neo11`；`iQOO Neo11 (V2332A)` → `iQOO-Neo11-V2332A` 形态
/// 由调用方拼接，不在本函数）；按字符截到 12（够辨识且不撑爆行）；结果空 =
/// 不可用（调用方继续退化链）。**不许**留空格/引号/方括号——它会进
/// 行尾标识，混进 grep 面与 JSON 文本
pub fn sanitize_dev(raw: &str) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for ch in raw.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
            n += 1;
            if n >= 12 {
                break;
            }
        }
    }
    out
}

/// 设备位取源（一次性缓存）：①环境覆盖 → ②机型属性文件 → ③持久短 id → ④unknown。
/// 任何一步 IO 失败只降档，恒返回可打印串（上报通道永不成事故）。
/// **每档都带来源标记**（BAR-273 第二批）：无标记的读数无法判「谁可跨机采信」。
fn device_dev() -> &'static str {
    DEV_TAG.get_or_init(|| {
        if let Ok(v) = std::env::var(DEV_OVERRIDE_ENV) {
            let s = sanitize_dev(&v);
            if !s.is_empty() {
                return with_tier(&s, TIER_ENV);
            }
        }
        for path in PROP_FILES {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            if let Some(v) = MODEL_KEYS.iter().find_map(|k| parse_prop_key(&text, k)) {
                let s = sanitize_dev(&v);
                if !s.is_empty() {
                    return with_tier(&s, TIER_MODEL);
                }
            }
        }
        fallback_dev_id()
    })
}

/// 拼「值@来源档」（纯函数，A 档可判）。档名只含小写字母，与 sanitize_dev 的
/// 白名单（字母数字／`-`／`_`）不冲突；`@` 分隔符本身不参与消毒，所以先消毒
/// 再拼，顺序不许倒——倒着拼会把档名里的字符喂进消毒器。
pub fn with_tier(v: &str, tier: &str) -> String {
    format!("{v}@{tier}")
}

/// 退化链第三档：共享存储持久化短 id（首启生成，两台机必然不同）。
/// 读/写失败都不致命——写不出（只读环境/测试宿主）就退到 unknown 档。
/// 三条硬要求（楼185 悬铃裁 B 甲）：①落点在共享存储 ②旧私有目录的件要迁过来
/// 且迁完删 ③写完**回读校验**，读不回不冒充成功。
fn fallback_dev_id() -> String {
    migrate_legacy_dev_id();
    if let Ok(existing) = std::fs::read_to_string(DEV_ID_PATH) {
        let s = sanitize_dev(&existing);
        if !s.is_empty() {
            return with_tier(&s, TIER_FALLBACK);
        }
    }
    let fresh = fresh_dev_id();
    if let Some(dir) = std::path::Path::new(DEV_ID_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(DEV_ID_PATH, &fresh).is_err() {
        return with_tier("unknown", TIER_UNKNOWN);
    }
    // 写后回读：共享存储可能被 scoped storage 挡住，写入「成功」但读不回来是
    // 一类真实故障（不是理论风险）。读不回 = 这一档不成立，如实降 unknown。
    match std::fs::read_to_string(DEV_ID_PATH) {
        Ok(back) if sanitize_dev(&back) == fresh => with_tier(&fresh, TIER_FALLBACK),
        _ => with_tier("unknown", TIER_UNKNOWN),
    }
}

/// 旧私有目录 → 共享存储的一次性迁移。目标已在位就不覆盖（两份各走一路比
/// 一份没有更坏），迁成功即删旧件；任何一步失败都只是「下轮再试」，不影响
/// 当前进程的上报。
fn migrate_legacy_dev_id() {
    let legacy = std::path::Path::new(DEV_ID_PATH_LEGACY);
    if !legacy.is_file() || std::path::Path::new(DEV_ID_PATH).is_file() {
        return;
    }
    let Ok(text) = std::fs::read_to_string(legacy) else {
        return;
    };
    if sanitize_dev(&text).is_empty() {
        return;
    }
    if let Some(dir) = std::path::Path::new(DEV_ID_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(DEV_ID_PATH, text.trim()).is_ok() {
        let _ = std::fs::remove_file(legacy);
    }
}

/// 首启短 id：时间纳秒 + pid 混洗成 8 位十六进制（无需随机数源，跨机撞率可忽略）
fn fresh_dev_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (u64::from(std::process::id()) << 32);
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    format!("{:08x}", x & 0xFFFF_FFFF)
}

fn instance_tag() -> &'static str {
    INSTANCE_TAG.get_or_init(|| {
        format!(
            "[{}/{}/{}]",
            std::env::consts::ARCH,
            device_dev(),
            std::process::id()
        )
    })
}

/// 给上报消息挂设备/实例标识（判读分道闸，见上）。空消息不领前导空格
pub fn stamp_msg(msg: &str) -> String {
    if msg.is_empty() {
        instance_tag().to_string()
    } else {
        format!("{msg} {}", instance_tag())
    }
}

/// 启动计时锚点（BAR-022：report 双通道的时间戳受冲洗节拍量化，段落归因
/// 靠不住——把「距 android_main 的毫秒数」烘进里程碑消息文本，通道乱序/
/// 量化都影响不了数值本身）。android_main 进门时 set 一次；非 Android
/// 环境（单测）未 set 时恒 0
static BOOT_T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn set_boot_t0() {
    BOOT_T0.set(std::time::Instant::now()).ok();
}

pub fn boot_ms() -> u128 {
    BOOT_T0.get().map(|t| t.elapsed().as_millis()).unwrap_or(0)
}

/// 启动后台冲洗线程（android_main 第一时间调）。幂等。
pub fn start_flusher() {
    let mut guard = SENDER.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return;
    }
    let (tx, rx) = channel::<String>();
    *guard = Some(tx);
    std::thread::spawn(move || {
        let mut backlog: VecDeque<String> = VecDeque::new();
        loop {
            // 先非阻塞排空新到的，再从队首冲洗（FIFO 保序）
            while let Ok(line) = rx.try_recv() {
                backlog.push_back(line);
            }
            // 积压上限：爆了就丢最旧的（丢旧保新，好过全线憋死）
            while backlog.len() > 200 {
                backlog.pop_front();
            }
            if let Some(front) = backlog.front() {
                if try_post(front).is_ok() {
                    backlog.pop_front();
                    continue; // 队里还有就立刻续冲
                }
                // 失败轮转：队首移到队尾——绝不让一条「毒行」堵死全队
                // （2026-08-13 实拍：首帧后全员静默，疑队首持续失败憋死心跳/ws）
                backlog.rotate_left(1);
            }
            // 队空或发送失败：阻塞等下一条，1s 超时回头重试 backlog
            if let Ok(line) = rx.recv_timeout(Duration::from_secs(1)) {
                backlog.push_back(line);
            }
        }
    });
}

/// 上报一行（stage = 阶段名，msg = 详情）。只入队，永不阻塞调用方。
/// 同时落 logcat(2026-08-21 诊断通道事故:HTTP 队列冲洗慢且随进程死
/// 全丢,启动归因两度被断供;logcat 环形缓冲在系统侧,进程死了也能捞)
pub fn report(stage: &str, msg: &str) {
    log::info!("[{stage}] {msg}");
    crate::trace::tap(stage, msg); // 行踪环旁路（过滤在环侧）
    enqueue(line_json(stage, msg));
}

/// 同步直报（仅启动第一格用）：早死进程等不到后台线程第一班车
/// （2026-08-13 实拍：全异步版「进门即死」零行日志）。有界阻塞
/// （connect 2s）直发，失败重试 3 次再入队交后台——当日真机单条丢失率
/// 约 50%（移动网络抖），重试压掉大部分；最坏 3×(2s+3s) 阻塞上限，可接受。
pub fn report_sync(stage: &str, msg: &str) {
    log::info!("[{stage}] {msg}");
    crate::trace::tap(stage, msg);
    let line = line_json(stage, msg);
    // 连发重试无间隔：失败多是 connect 层秒挂，等不等都一样
    for _ in 0..3 {
        if try_post(&line).is_ok() {
            return;
        }
    }
    enqueue(line);
}

/// 单发直报（BAR-022 归因锚点用）：一次尝试，失败入队交后台。
/// 不带重试——重试会放大阻塞，诊断锚点宁可丢不可拖死握手线程
/// （冲洗队列作载体已实踩不可靠：应用一划掉，队列里的行随进程死全丢）。
pub fn report_sync_once(stage: &str, msg: &str) {
    log::info!("[{stage}] {msg}");
    crate::trace::tap(stage, msg);
    let line = line_json(stage, msg);
    if try_post(&line).is_err() {
        enqueue(line);
    }
}

/// 上报行 JSON 装配（单一源：三条上报路径全走这里，设备/实例标识在此挂）
fn line_json(stage: &str, msg: &str) -> String {
    format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        escape_json(stage),
        escape_json(&stamp_msg(msg))
    )
}

fn enqueue(line: String) {
    let guard = SENDER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(line);
    }
    // flusher 未启动（如非 Android 单测）则直接丢——通道永不成事故
}

fn try_post(body: &str) -> std::io::Result<()> {
    let mut s = TcpStream::connect_timeout(&SERVER_ADDR, Duration::from_secs(2))?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    s.set_write_timeout(Some(Duration::from_secs(3)))?;
    let req = format!(
        "POST {PATH} HTTP/1.1\r\nHost: {HOST_HEADER}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    // 写完必须读应答：发完即关会让 nginx 视客户端中止而断掉上游转发；
    // 且只认 200——502/500 一律视为未送达，留队重发（宁重复不丢）
    s.shutdown(Shutdown::Write)?;
    let mut resp = Vec::new();
    s.read_to_end(&mut resp)?;
    if http_status_is_200(&resp) {
        Ok(())
    } else {
        Err(std::io::Error::other("非 200 应答"))
    }
}

/// 从 HTTP 应答提取状态码判 200（纯逻辑，有钉）
pub fn http_status_is_200(resp: &[u8]) -> bool {
    let end = resp
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(resp.len());
    let status_line = String::from_utf8_lossy(&resp[..end]);
    status_line.split_whitespace().nth(1) == Some("200")
}

/// JSON 字符串转义（引号/反斜杠/控制字符）
pub fn escape_json(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 转义引号反斜杠() {
        assert_eq!(escape_json("a\"b\\c"), "a\\\"b\\\\c");
    }

    #[test]
    fn 转义控制字符() {
        assert_eq!(escape_json("x\ny\rz\t"), "x\\ny\\rz\\t");
        assert_eq!(escape_json("a\u{0007}b"), "a\\u0007b");
    }

    #[test]
    fn 普通字符原样通过() {
        assert_eq!(escape_json("紫屏 #8B5CF6 🚀"), "紫屏 #8B5CF6 🚀");
    }

    #[test]
    fn 拼接结果是合法json() {
        let body = format!(
            "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
            escape_json("pa\"nic"),
            escape_json("at src\\main.rs:1\nboom")
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["stage"], "pa\"nic");
        assert_eq!(v["msg"], "at src\\main.rs:1\nboom");
    }

    #[test]
    fn 状态码判定() {
        assert!(http_status_is_200(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"
        ));
        assert!(http_status_is_200(b"HTTP/1.0 200\n\n"));
        assert!(!http_status_is_200(
            b"HTTP/1.1 502 Bad Gateway\r\n\r\n<html>"
        ));
        assert!(!http_status_is_200(
            b"HTTP/1.1 500 Internal Server Error\r\n\r\n"
        ));
        assert!(!http_status_is_200(b""));
        assert!(!http_status_is_200(b"garbage"));
    }
}
