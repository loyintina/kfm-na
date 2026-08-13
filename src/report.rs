//! report.rs — 飞鸽传书：实拍现场回传通道
//!
//! 背景（2026-08-13 实拍闪退）：手机走蜂窝/WiFi NAT，服务器 adb 反连不回去，
//! logcat 拿不到。APK 的 panic 与启动里程碑改走 HTTP POST 直报 kfmv4 服务器
//! （手机既然能下载 APK，就一定能回传），落盘 /root/kfm-na/field-reports.log。
//!
//! 铁律：本通道任何失败都必须吞掉——上报通道自己炸了就是二次事故。

use std::io::Write;
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

/// 服务器地址（nginx 80 反代 → kfmv4 8021，/kfmv4 代字前缀）
const HOST: &str = "8.145.46.182";
const PORT: u16 = 80;
const PATH: &str = "/kfmv4/api/na-report";

/// 未送达队列：单条发送失败不丢，压进队列，下一次 report 捎带重发
/// （2026-08-13 实拍：fire-and-forget 单条丢失导致无法区分「没跑到」与「丢了」）
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// best-effort 上报一行（stage = 阶段名，msg = 详情）；任何失败只入队，不炸
pub fn report(stage: &str, msg: &str) {
    let line = format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        escape_json(stage),
        escape_json(msg)
    );
    let mut q = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    q.push(line);
    // 依次清队列：一条失败就停（多半网络不通），留待下次捎带
    while let Some(first) = q.first() {
        if try_post(first).is_ok() {
            q.remove(0);
        } else {
            break;
        }
    }
}

fn try_post(body: &str) -> std::io::Result<()> {
    let mut s = TcpStream::connect((HOST, PORT))?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    s.set_write_timeout(Some(Duration::from_secs(3)))?;
    let req = format!(
        "POST {PATH} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    Ok(())
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
}
