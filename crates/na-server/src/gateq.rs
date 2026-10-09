//! gateq.rs — 闸门触发腿 QUIC 化的服务器侧队列（BAR-233，乙案 v1，
//! 追踪信 NA0163；白露 MAIN0125 发令）。
//!
//! 八通道触发与结果改走数据面（9021 HTTP）：脚本 POST /api/gate/<req>
//! 入队 → na 核内 gate_poller 长轮询 /api/gate/pending 取走、原子落盘
//! DUMP_DIR → 既有 300ms 值守线程照常消费；结果反向 POST /api/gate/result/
//! <name> 入表 → 脚本 GET 一次性取走。sshd（9022 桥）降级为兜底路——
//! BAR-229 的「prefix 死闸门陪葬」循环依赖就此拆除（闸门与数据面同生
//! 共死，QUIC 在闸门就在）。
//!
//! 选轮询不选推送：零新协议（纯 HTTP），与数据面共用同一条 QUIC 腿；
//! 长轮询（?wait=N）把触发延迟压到即时。
//!
//! **多设备（NA0163 楼25 承影需求、楼26 裁 B 案「每机一条闸门腿」）**：
//! 队列/结果表按**到达监听口**分槽——端口即设备命名空间。QUIC 桥是字节
//! 透传 splice，HTTP 层看不到会话，设备维只能落口：脚本打哪个口＝货进
//! 哪队；手机 poller 的桥 port_header 指哪个口＝取哪队。设备↔口显式
//! 配置绑定（9021＝主生产机，NA_GATE_LEGS 各腿＝判卷机，9023＝9 机），
//! 不靠注册顺序（防双机翻转变串机）。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// 十通道触发文件名白名单（A 档纯函数判据，钉在 gate_http_spec）——shot
/// 家族两个文件名（CPU/GLES）、orb 的注入文件名，共十名。加通道 =
/// 此表加一行（na 侧 gate_poller 同源两份，注释互指）。
/// BAR-240（MAIN0134）：switch-req 补入——缺它时远程切换正路被 400 拒，
/// 调用方被逼成 keys-in 盲发活跃会话（承影救机命令误投白露会话实案）。
pub const CHANNELS: [&str; 10] = [
    "shot-req",
    "shot-gles-req",
    "text-req",
    "keys-in",
    "ping-req",
    "restart-req",
    "trace-req",
    "stats-req",
    "orb-inject",
    "switch-req",
];

pub fn channel_ok(ch: &str) -> bool {
    CHANNELS.contains(&ch)
}

/// 单口队列条目：(入队时戳, 通道, payload)
type QueueItems = VecDeque<(u64, String, Vec<u8>)>;
/// 单口结果表：名 → 字节
type ResultMap = HashMap<String, Vec<u8>>;

/// 下行队列：**按到达监听口分槽**（键＝口；多设备命名空间，见模块头）。
/// LazyLock：HashMap::new 非常量（随机态种子）
static QUEUES: std::sync::LazyLock<Mutex<HashMap<u16, QueueItems>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
/// 上行结果表（同名单次覆盖，GET 取走即删——一次性语义照抄闸门
/// 「摘下即消费」，脚本侧 rm -f 清理在 HTTP 路不再需要），同按口分槽
static RESULTS: std::sync::LazyLock<Mutex<HashMap<u16, ResultMap>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn push(leg: u16, channel: &str, payload: Vec<u8>) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    QUEUES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(leg)
        .or_default()
        .push_back((ts, channel.to_string(), payload));
}

pub fn drain(leg: u16) -> Vec<(u64, String, Vec<u8>)> {
    let mut qs = QUEUES.lock().unwrap_or_else(|p| p.into_inner());
    match qs.get_mut(&leg) {
        Some(q) => q.drain(..).collect(),
        None => Vec::new(),
    }
}

pub fn queue_empty(leg: u16) -> bool {
    QUEUES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&leg)
        .is_none_or(VecDeque::is_empty)
}

pub fn put_result(leg: u16, name: &str, body: Vec<u8>) {
    RESULTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(leg)
        .or_default()
        .insert(name.to_string(), body);
}

pub fn take_result(leg: u16, name: &str) -> Option<Vec<u8>> {
    RESULTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_mut(&leg)
        .and_then(|m| m.remove(name))
}

/// NA_GATE_LEGS 解析（A 档纯函数，钉在 gate_http_spec）：逗号分隔的额外
/// 闸门腿口（缺省/空 = 无腿）。fail-loud 拒：非数字、0、与主口重复、
/// 腿间互重——半残/歧义配置 = 串机温床，宁可拒启不许错路由（BAR-200 同族）。
pub fn parse_gate_legs(env: Option<&str>, main_port: u16) -> Result<Vec<u16>, String> {
    let Some(s) = env else {
        return Ok(Vec::new());
    };
    let mut out: Vec<u16> = Vec::new();
    for part in s.split(',').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        let port: u16 = part
            .parse()
            .map_err(|_| format!("NA_GATE_LEGS 含非端口项: {part}"))?;
        if port == 0 {
            return Err("NA_GATE_LEGS 含 0 口".to_string());
        }
        if port == main_port {
            return Err(format!("NA_GATE_LEGS 与主口 {main_port} 重复: {port}"));
        }
        if out.contains(&port) {
            return Err(format!("NA_GATE_LEGS 腿间重复: {port}"));
        }
        out.push(port);
    }
    Ok(out)
}

/// pending 响应体（text/plain，A 档纯函数，钉在 httpd_spec 黄金样例）：
/// 首行十进制计数 N，随后 N 行 `channel\tpayload_hex`（hex 小写；空
/// payload = 空 hex 段）。选此格式：na 侧零 JSON 依赖、零二进制歧义。
pub fn pending_body(items: &[(u64, String, Vec<u8>)]) -> String {
    let mut out = format!("{}\n", items.len());
    for (_, ch, payload) in items {
        let hex: String = payload.iter().map(|b| format!("{b:02x}")).collect();
        out.push_str(&format!("{ch}\t{hex}\n"));
    }
    out
}
