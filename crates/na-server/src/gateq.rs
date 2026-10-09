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

/// 下行队列：(入队时戳, 通道, payload)
static QUEUE: Mutex<VecDeque<(u64, String, Vec<u8>)>> = Mutex::new(VecDeque::new());
/// 上行结果表（同名单次覆盖，GET 取走即删——一次性语义照抄闸门
/// 「摘下即消费」，脚本侧 rm -f 清理在 HTTP 路不再需要）
static RESULTS: Mutex<Option<HashMap<String, Vec<u8>>>> = Mutex::new(None);

pub fn push(channel: &str, payload: Vec<u8>) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    QUEUE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push_back((ts, channel.to_string(), payload));
}

pub fn drain() -> Vec<(u64, String, Vec<u8>)> {
    let mut q = QUEUE.lock().unwrap_or_else(|p| p.into_inner());
    q.drain(..).collect()
}

pub fn queue_empty() -> bool {
    QUEUE.lock().unwrap_or_else(|p| p.into_inner()).is_empty()
}

pub fn put_result(name: &str, body: Vec<u8>) {
    RESULTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(name.to_string(), body);
}

pub fn take_result(name: &str) -> Option<Vec<u8>> {
    RESULTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_mut()
        .and_then(|m| m.remove(name))
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
