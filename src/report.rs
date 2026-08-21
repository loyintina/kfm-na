//! report.rs — 飞鸽传书：实拍现场回传通道
//!
//! 背景（2026-08-13 实拍闪退）：手机走蜂窝/WiFi NAT，服务器 adb 反连不回去，
//! logcat 拿不到。APK 的 panic 与启动里程碑改走 HTTP POST 直报 kfmv4 服务器
//! （手机既然能下载 APK，就一定能回传），落盘 /root/kfm-na/field-reports.log。
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

/// 服务器地址（2026-08-14 拓扑变更）：kfmv4 直连 127.0.0.1:8021（无 nginx、
/// 无公网）——手机侧 SSH 隧道把两端 8021 对接（Termux ssh -L），APK 打
/// 本机回环即达。旧拓扑 nginx 80 反代 8.145.46.182 已废弃。
/// PATH 保留 /kfmv4 前缀：服务端 /api 与 /kfmv4/api 双挂载，都能到
const SERVER_ADDR: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
    8021,
);
const HOST_HEADER: &str = "127.0.0.1:8021";
const PATH: &str = "/kfmv4/api/na-report";

static SENDER: Mutex<Option<Sender<String>>> = Mutex::new(None);

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
    enqueue(format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        escape_json(stage),
        escape_json(msg)
    ));
}

/// 同步直报（仅启动第一格用）：早死进程等不到后台线程第一班车
/// （2026-08-13 实拍：全异步版「进门即死」零行日志）。有界阻塞
/// （connect 2s）直发，失败重试 3 次再入队交后台——当日真机单条丢失率
/// 约 50%（移动网络抖），重试压掉大部分；最坏 3×(2s+3s) 阻塞上限，可接受。
pub fn report_sync(stage: &str, msg: &str) {
    log::info!("[{stage}] {msg}");
    let line = format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        escape_json(stage),
        escape_json(msg)
    );
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
    let line = format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        escape_json(stage),
        escape_json(msg)
    );
    if try_post(&line).is_err() {
        enqueue(line);
    }
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
