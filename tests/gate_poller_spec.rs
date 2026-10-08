//! gate_poller_spec.rs — BAR-233 乙案 v1 na 侧考题。
//!
//! 判卷维度：hex 编解码往返（含空/二进制）／pending 体解析（坏行跳过
//! 不整批炸）／write_atomic 原子性与白名单纵深闸／端到端冒烟（假
//! HTTP 服务器回 pending 体 → 触发文件出现在注入的 dump_dir）。
//! 「断 sshd 全灭」场景判卷在设备侧（pkill sshd 后八通道全活——
//! MAIN0125 §四），本卷钉住进程内环。

use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn spec_bar233_hex往返() {
    let f = kfm_na::gate_poller::hex_decode;
    // 空与往返
    assert_eq!(f(""), Some(Vec::new()));
    assert_eq!(f("00ff10"), Some(vec![0x00, 0xff, 0x10]));
    assert_eq!(f("616c"), Some(b"al".to_vec()));
    // 坏输入：奇长/大写/非法字符
    assert_eq!(f("0"), None);
    assert_eq!(f("0F"), None, "只认小写");
    assert_eq!(f("zz"), None);
}

#[test]
fn spec_bar233_pending解析_坏行跳过() {
    let body = b"3\nping-req\t616c697665\nrestart-req\t\nBAD-NO-TAB\nkeys-in\tzzbad\n";
    let items = kfm_na::gate_poller::parse_pending(body);
    // 坏行（无 TAB、非法 hex）跳过，好行保留——不整批炸
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, "ping-req");
    assert_eq!(items[0].1, b"alive");
    assert_eq!(items[1].0, "restart-req");
    assert!(items[1].1.is_empty(), "空 payload");
}

#[test]
fn spec_bar233_write_atomic_原子与白名单() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // 白名单纵深闸：非白名单名拒（服务器侧已闸，这里第二道）
    assert!(kfm_na::gate_poller::write_atomic(dir, "evil-req", b"x").is_err());
    // 含换行/NUL 的 payload 逐字节落地；无 .new 残留
    let payload: &[u8] = b"line1\nline2\x00tail";
    kfm_na::gate_poller::write_atomic(dir, "keys-in", payload).unwrap();
    assert_eq!(std::fs::read(dir.join("keys-in")).unwrap(), payload);
    assert!(!dir.join("keys-in.new").exists(), "rename 后无 .new 残留");
    // 覆写语义：第二次写完整替换（不留半文件）
    kfm_na::gate_poller::write_atomic(dir, "keys-in", b"second").unwrap();
    assert_eq!(std::fs::read(dir.join("keys-in")).unwrap(), b"second");
}

/// 端到端冒烟：假服务器回一份 pending 体 → start_with 轮询 → 触发
/// 文件落进注入的 dump_dir（进程内全环：fetch → parse → 原子落盘）。
/// offer_result 未启动时 no-op 也在本卷顺带咬（直接调用不 panic）。
#[test]
fn spec_bar233_端到端冒烟_假服务器落盘() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    std::fs::create_dir_all(&dump).unwrap();
    let lis = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = lis.local_addr().unwrap().port();
    // 假服务器：第一轮回 pending（含两条），后续轮询空（避免忙循环
    // 打满——Connection: close 一回一连接）
    std::thread::spawn(move || {
        for i in 0..40 {
            let (mut s, _) = match lis.accept() {
                Ok(v) => v,
                Err(_) => return,
            };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let body: &str = if i == 0 {
                "2\nping-req\t\nrestart-req\t68656c6c6f\n"
            } else {
                "0\n"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    kfm_na::gate_poller::start_with(port, dump.clone());
    // offer_result 在（伪）启动态：不 panic 即可（上传会连假口失败重试
    // 3 次后丢——异步线程，不挡本卷）
    std::thread::sleep(std::time::Duration::from_millis(500));
    kfm_na::gate_poller::offer_result("never-file", std::path::Path::new("/nonexistent"));
    // 等第一轮落盘
    let mut got = false;
    for _ in 0..40 {
        if dump.join("ping-req").exists() && dump.join("restart-req").exists() {
            got = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(got, "假服务器 pending 应已落成触发文件");
    assert_eq!(std::fs::read(dump.join("restart-req")).unwrap(), b"hello");
}

#[test]
fn spec_bar233_轮询节拍短窗() {
    // BAR-233 追件：POLL_WAIT_SECS 须为 2（25s 长轮询在 QUIC 桥撞空闲
    // 掐线——现场「响应无头体分隔」实证；变异：改回 25 → 本钉红）
    assert_eq!(
        kfm_na::gate_poller::POLL_WAIT_SECS,
        2,
        "短轮询节拍被改回长窗"
    );
}

#[test]
fn spec_bar233_tick心跳与超时常量() {
    // BAR-233 追件三（白露 NA0163 19 楼批）：僵死双治的守卫常量——
    // tick 每 30 拍（变异：摘心跳块 → 本钉红不了行为，钉常量存在性
    // 由源码守卫咬，这里钉节拍不被改坏）；connect 超时 2s 同源。
    assert_eq!(kfm_na::gate_poller::POLL_WAIT_SECS, 2);
    // tick 节拍 30 写死在 poll_loop——源码守卫（test-bar-new 同族）在
    // scripts/check/ 不便，这里以「常量面 + 源码 grep」双咬：
    let src = std::fs::read_to_string("src/gate_poller.rs").unwrap_or_default();
    assert!(
        src.contains("is_multiple_of(30)"),
        "tick 心跳块被摘（每 30 拍一行是僵死可见性的唯一解药）"
    );
    assert!(
        src.contains("connect_timeout"),
        "connect 超时被退回裸 connect（挂死主嫌疑回潮）"
    );
}
