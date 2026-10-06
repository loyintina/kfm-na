//! gate_poller.rs — 闸门触发腿 QUIC 化的 na 侧（BAR-233，乙案 v1，
//! 追踪信 NA0163；白露 MAIN0125 发令）。
//!
//! 下行：长轮询 na-server `GET /api/gate/pending?wait=25`（经 QUIC 数据
//! 腿本地口 127.0.0.1:{local_port}），取到的触发原子落盘 DUMP_DIR
//! （`.new` 写全 → rename，照抄 gate-lib 语义）→ 既有 300ms 值守线程
//! 照常消费——**闸门与数据面同生共死**，sshd（9022 桥）降级兜底。
//! 上行：gate 各通道写完结果文件调 [`offer_result`]，由本模块上传
//! 线程 POST `/api/gate/result/<name>` 原样字节（脚本 GET 一次性取走，
//! 替代 ssh cat/scp 拉取）。
//!
//! 失败静默重试（下一长轮询周期即自愈），**状态转换才报表**——防
//! 「数据面不可达」30s 一行刷屏。判卷（MAIN0125 §四）：八通道红绿
//! 各一例＋「断 sshd 全灭」场景闸门照通（pkill sshd 后八通道全活）。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

/// 轮询等待（秒）——2026-10-07 判卷回炉改 2（白露 NA0163 7 楼裁①）：
/// 25s 长轮询在 QUIC 桥上撞空闲掐线（现场实证「响应无头体分隔」反复），
/// 短轮询掐线窗口消失、闸门 2s 延迟无感、失败重试成本恒定
pub const POLL_WAIT_SECS: u8 = 2;
/// 上传重试次数（超过即丢＋报表一行）
const UPLOAD_RETRIES: u32 = 3;

/// 八通道白名单（与 crates/na-server/src/gateq.rs CHANNELS 同源两份，
/// 改要同票——注释互指）
const CHANNELS: [&str; 9] = [
    "shot-req",
    "shot-gles-req",
    "text-req",
    "keys-in",
    "ping-req",
    "restart-req",
    "trace-req",
    "stats-req",
    "orb-inject",
];

fn channel_ok(ch: &str) -> bool {
    CHANNELS.contains(&ch)
}

static STARTED: OnceLock<()> = OnceLock::new();

/// 上传通道（offer_result 投件 → 上传线程消费）。未 start 时
/// offer_result 为 no-op（STARTED 未设）。
static UPLOAD_TX: OnceLock<mpsc::Sender<(String, Vec<u8>)>> = OnceLock::new();

pub fn start(local_port: u16) {
    start_with(local_port, PathBuf::from(crate::gate::DUMP_DIR));
}

/// 测试注入口：dump_dir 可指定（start 幂等；start_with 供测试直用）
pub fn start_with(local_port: u16, dump_dir: PathBuf) {
    if STARTED.set(()).is_err() {
        crate::report::report("gatepoll", "轮询已在跑，忽略重复启动");
        return;
    }
    let (tx, rx) = mpsc::channel::<(String, Vec<u8>)>();
    let _ = UPLOAD_TX.set(tx);
    crate::report::report(
        "gatepoll",
        &format!("闸门 QUIC 轮询启动（BAR-233）：127.0.0.1:{local_port} → DUMP_DIR"),
    );
    std::thread::spawn(move || poll_loop(local_port, dump_dir));
    std::thread::spawn(move || upload_loop(local_port, rx));
}

fn poll_loop(local_port: u16, dump_dir: PathBuf) {
    let mut down_reported = false;
    loop {
        match fetch_pending(local_port) {
            Ok(body) => {
                if down_reported {
                    crate::report::report("gatepoll", "数据面恢复，轮询续跑");
                    down_reported = false;
                }
                for (ch, payload) in parse_pending(&body) {
                    if !channel_ok(&ch) {
                        // 服务器侧已白名单——这里再闸一道（纵深防御）
                        crate::report::report("gatepoll", &format!("拒非白名单通道: {ch}"));
                        continue;
                    }
                    if let Err(e) = write_atomic(&dump_dir, &ch, &payload) {
                        crate::report::report("gatepoll", &format!("落盘 {ch} 失败: {e}"));
                    }
                }
            }
            Err(e) => {
                if !down_reported {
                    crate::report::report(
                        "gatepoll",
                        &format!("数据面不可达（{e}），轮询挂起自 retry"),
                    );
                    down_reported = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn upload_loop(local_port: u16, rx: mpsc::Receiver<(String, Vec<u8>)>) {
    while let Ok((name, bytes)) = rx.recv() {
        let mut ok = false;
        for _ in 0..UPLOAD_RETRIES {
            if post_result(local_port, &name, &bytes).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if !ok {
            crate::report::report(
                "gatepoll",
                &format!("结果 {name} 上传丢弃（{} 次重试毕）", UPLOAD_RETRIES),
            );
        }
    }
}

/// 结果回传钩子：gate 通道写完结果文件后调用。未启动（无 poller）=
/// no-op，调用方零负担。
pub fn offer_result(name: &str, path: &Path) {
    if let Some(tx) = UPLOAD_TX.get()
        && let Ok(bytes) = std::fs::read(path)
    {
        if bytes.len() > 32 * 1024 * 1024 {
            crate::report::report("gatepoll", &format!("结果 {name} 超 32MB 不上传"));
            return;
        }
        let _ = tx.send((name.to_string(), bytes));
    }
}

/// 手写最小 HTTP GET（零依赖：std TcpStream）。返回响应体字节。
fn http_call(
    local_port: u16,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let mut s = TcpStream::connect(format!("127.0.0.1:{local_port}"))
        .map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(POLL_WAIT_SECS as u64 + 10)))
        .map_err(|e| format!("timeout set: {e}"))?;
    let body = body.unwrap_or(&[]);
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{local_port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    if !body.is_empty() {
        s.write_all(body).map_err(|e| format!("write body: {e}"))?;
    }
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| format!("read: {e}"))?;
    // 分头体（Connection: close → 读尽即体）
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("响应无头体分隔")?;
    Ok(raw[split + 4..].to_vec())
}

fn fetch_pending(local_port: u16) -> Result<Vec<u8>, String> {
    http_call(
        local_port,
        "GET",
        &format!("/api/gate/pending?wait={POLL_WAIT_SECS}"),
        None,
    )
}

fn post_result(local_port: u16, name: &str, bytes: &[u8]) -> Result<(), String> {
    let resp = http_call(
        local_port,
        "POST",
        &format!("/api/gate/result/{name}"),
        Some(bytes),
    )?;
    if resp.starts_with(b"{\"ok\":true}") {
        Ok(())
    } else {
        Err(format!(
            "非 ok 响应: {}",
            String::from_utf8_lossy(&resp[..resp.len().min(64)])
        ))
    }
}

/// pending 体解析（A 档纯函数，钉在 gate_poller_spec）：首行计数，随后
/// `channel\thex` 行。坏行跳过（hex 非偶数长/非法字符 = 跳，不整批炸）。
pub fn parse_pending(body: &[u8]) -> Vec<(String, Vec<u8>)> {
    let text = String::from_utf8_lossy(body);
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let Some((ch, hex)) = line.split_once('\t') else {
            continue;
        };
        match hex_decode(hex) {
            Some(bytes) => out.push((ch.to_string(), bytes)),
            None => continue,
        }
    }
    out
}

/// hex 小写解码（A 档纯函数）：非偶长/非法字符 = None
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// 原子落盘（A 档可测：写 `.new` 全量 → rename——照抄 gate-lib 的
/// `.new → mv` 语义，写一半崩溃不留半文件在正式名）
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    if !channel_ok(name) {
        return Err(format!("非白名单通道: {name}"));
    }
    let tmp = dir.join(format!("{name}.new"));
    let target = dir.join(name);
    std::fs::write(&tmp, bytes).map_err(|e| format!("写 .new 失败: {e}"))?;
    std::fs::rename(&tmp, &target).map_err(|e| format!("rename 失败: {e}"))?;
    Ok(())
}
