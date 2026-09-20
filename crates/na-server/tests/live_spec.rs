//! crates/na-server/tests/live_spec.rs — 整环考题：真二进制 + 真 PTY + 真 WS
//!
//! B/C 档判卷：不走 mock，直接 spawn na-server 二进制（ephemeral 口），
//! na-protocol 客户端编解码打全套：Open → Output 字节级 → Resize → Exit 码。
//! na 壳的 conn.rs 就是这套消息的消费者——这里绿 = na 切后端零改动可读。

use std::io::Read as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use na_protocol::{ClientMsg, ServerMsg};

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 起真二进制：固定高调口 + idle 自退关死 + report 落临时目录
fn up(port: u16) -> (ServerGuard, tempfile::TempDir) {
    up_env(port, &[])
}

/// 同 up，可附加环境变量（判卷仪器旋钮，如 NA_READ_BUF 拧小读块）
fn up_env(port: u16, envs: &[(&str, &str)]) -> (ServerGuard, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("临时目录");
    let log = tmp.path().join("field-reports.log");
    let child = Command::new(env!("CARGO_BIN_EXE_na-server"))
        .env("NA_BIND", format!("127.0.0.1:{port}"))
        .env("NA_IDLE_EXIT_SECS", "0")
        .env("NA_REPORT_LOG", &log)
        .envs(envs.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn na-server");
    // 等口起来
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "na-server 10s 没起来");
        std::thread::sleep(Duration::from_millis(50));
    }
    (ServerGuard(child), tmp)
}

async fn ws_connect(
    port: u16,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("ws 连接");
    ws
}

async fn send<S>(ws: &mut S, msg: &ClientMsg)
where
    S: SinkExt<tokio_tungstenite::tungstenite::Message> + Unpin,
    S::Error: std::fmt::Debug,
{
    ws.send(na_protocol::encode_client(msg).into())
        .await
        .expect("发帧");
}

/// 收帧直到满足条件或超时（ping 等无关帧跳过）
async fn recv_until(
    ws: &mut (
             impl StreamExt<
        Item = Result<
            tokio_tungstenite::tungstenite::Message,
            tokio_tungstenite::tungstenite::Error,
        >,
    > + Unpin
         ),
    timeout: Duration,
    mut pred: impl FnMut(&ServerMsg) -> bool,
) -> ServerMsg {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(left > Duration::ZERO, "收帧超时");
        let frame = tokio::time::timeout(left, ws.next())
            .await
            .expect("收帧超时")
            .expect("流提前终结")
            .expect("帧错");
        let tokio_tungstenite::tungstenite::Message::Text(t) = frame else {
            continue;
        };
        let m = na_protocol::decode_server(&t).expect("解码服务端帧");
        if pred(&m) {
            return m;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn live_open_output_exit_roundtrip() {
    let (_guard, _tmp) = up(19921);
    let mut ws = ws_connect(19921).await;

    // Open：/bin/sh -c 'echo na-live-ok; exit 3'
    send(
        &mut ws,
        &ClientMsg::Open {
            cwd: None,
            command: Some("echo na-live-ok; exit 3".into()),
            tag: Some("live".into()),
        },
    )
    .await;
    let opened = recv_until(&mut ws, Duration::from_secs(5), |m| {
        matches!(m, ServerMsg::Opened { .. })
    })
    .await;
    let ServerMsg::Opened { session_id, tag } = opened else {
        unreachable!()
    };
    assert_eq!(tag.as_deref(), Some("live"), "tag 必须原样回");

    // Output：字节级含 na-live-ok
    let out = recv_until(
        &mut ws,
        Duration::from_secs(5),
        |m| matches!(m, ServerMsg::Output { data, .. } if data.contains("na-live-ok")),
    )
    .await;
    assert!(matches!(out, ServerMsg::Output { .. }));

    // Exit：码 3 透传
    let exit = recv_until(&mut ws, Duration::from_secs(5), |m| {
        matches!(m, ServerMsg::Exit { .. })
    })
    .await;
    let ServerMsg::Exit {
        session_id: sid2,
        code,
    } = exit
    else {
        unreachable!()
    };
    assert_eq!(sid2, session_id);
    assert_eq!(code, 3, "exit 码必须透传");
}

#[tokio::test(flavor = "current_thread")]
async fn live_interactive_input_echo() {
    let (_guard, _tmp) = up(19922);
    let mut ws = ws_connect(19922).await;

    send(
        &mut ws,
        &ClientMsg::Open {
            cwd: None,
            command: None, // 交互 shell
            tag: None,
        },
    )
    .await;
    let opened = recv_until(&mut ws, Duration::from_secs(5), |m| {
        matches!(m, ServerMsg::Opened { .. })
    })
    .await;
    let ServerMsg::Opened { session_id, .. } = opened else {
        unreachable!()
    };

    // 写一行 echo，读回
    send(
        &mut ws,
        &ClientMsg::Input {
            session_id: session_id.clone(),
            input: "echo na-interactive-$((6*7))\n".into(),
        },
    )
    .await;
    recv_until(
        &mut ws,
        Duration::from_secs(5),
        |m| matches!(m, ServerMsg::Output { data, .. } if data.contains("na-interactive-42")),
    )
    .await;

    // Resize 不炸（行为正确性由 redroid 考场判，这里只钉不崩不踢）
    send(
        &mut ws,
        &ClientMsg::Resize {
            session_id: session_id.clone(),
            cols: 132,
            rows: 50,
        },
    )
    .await;
    send(
        &mut ws,
        &ClientMsg::Input {
            session_id: session_id.clone(),
            input: "echo after-resize\n".into(),
        },
    )
    .await;
    recv_until(
        &mut ws,
        Duration::from_secs(5),
        |m| matches!(m, ServerMsg::Output { data, .. } if data.contains("after-resize")),
    )
    .await;

    // Close → 等死线程补 Exit
    send(&mut ws, &ClientMsg::Close { session_id }).await;
    recv_until(&mut ws, Duration::from_secs(5), |m| {
        matches!(m, ServerMsg::Exit { .. })
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn live_report_and_health() {
    let (_guard, tmp) = up(19923);

    // POST /kfmv4/api/na-report（na 现网打的就是带前缀路径）
    let body = "{\"ts\":1,\"tag\":\"live-spec\"}";
    let mut resp = ureq_post(19923, "/kfmv4/api/na-report", body);
    assert!(resp.starts_with("HTTP/1.1 200"), "report 响应: {resp}");

    // 落盘内容原样一行
    let log = std::fs::read_to_string(tmp.path().join("field-reports.log")).expect("日志落盘");
    assert_eq!(log.trim(), body, "report 必须原样落盘");

    // GET /api/na/health
    resp = ureq_get(19923, "/api/na/health");
    assert!(resp.starts_with("HTTP/1.1 200"), "health 响应: {resp}");
    let json_start = resp.find('{').expect("health 有 JSON 体");
    let v: serde_json::Value = serde_json::from_str(&resp[json_start..]).expect("合法 JSON");
    assert!(v["uptime_s"].as_u64().is_some());
    assert_eq!(v["sessions"], serde_json::json!([]));

    // 404 面
    resp = ureq_get(19923, "/api/ai/chat");
    assert!(resp.starts_with("HTTP/1.1 404"), "未知路径必须 404: {resp}");
}

/// 裸 socket HTTP/1.1 POST（考题不引新依赖）
fn ureq_post(port: u16, path: &str, body: &str) -> String {
    let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连");
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    std::io::Write::write_all(&mut c, req.as_bytes()).expect("写");
    let mut buf = String::new();
    c.read_to_string(&mut buf).expect("读");
    buf
}

fn ureq_get(port: u16, path: &str) -> String {
    let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连");
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    std::io::Write::write_all(&mut c, req.as_bytes()).expect("写");
    let mut buf = String::new();
    c.read_to_string(&mut buf).expect("读");
    buf
}

/// 增量解码整环钉（redroid U+FFFD tofu 病灶）：3750 个「中」= 15000 字节
/// （yes 每行 4 字节：3 字节「中」+ \n，head 恰切在行界）。
/// 读块拧到 7 字节（7 % 3 = 1，每块必劈字）——内核 PTY 分包恰好对齐时
/// 默认 8KB 块抓不到病灶（变异实证），仪器旋钮让劈字成为必然。
/// 命令尾挂 sleep：探针实证子进程退出的瞬间从端关闭会冲刷主端未读
/// 缓冲（Linux PTY 尾巴丢失竞态）——DONE 行会丢，吊住从端让尾巴送达。
/// v1 按块 lossy 解码必红。
#[tokio::test(flavor = "current_thread")]
async fn live_multibyte_across_chunks_no_replacement() {
    let (_guard, _tmp) = up_env(19924, &[("NA_READ_BUF", "7")]);
    let mut ws = ws_connect(19924).await;

    send(
        &mut ws,
        &ClientMsg::Open {
            cwd: None,
            command: Some("yes 中 | head -c 15000; echo LIVE-DONE; sleep 5".into()),
            tag: None,
        },
    )
    .await;
    recv_until(&mut ws, Duration::from_secs(5), |m| {
        matches!(m, ServerMsg::Opened { .. })
    })
    .await;

    // 攒帧直到 DONE。注意判读必须对【累积流】做——7 字节帧会把
    // "LIVE-DONE" 劈进两帧（探针实证：逐帧 contains 永远等不到）。
    // FFFD 是单字符不会跨帧，逐帧查即可；「中」计数也在累积流上算。
    let mut accum = String::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(
            left > Duration::ZERO,
            "收 DONE 超时（累计「中」{}）",
            accum.matches('中').count()
        );
        let frame = tokio::time::timeout(left, ws.next())
            .await
            .expect("收帧超时")
            .expect("流提前终结")
            .expect("帧错");
        let tokio_tungstenite::tungstenite::Message::Text(t) = frame else {
            continue;
        };
        let m = na_protocol::decode_server(&t).expect("解码");
        if let ServerMsg::Output { data, .. } = m {
            assert!(
                !data.contains('\u{FFFD}'),
                "块界劈字产替换符（增量解码没干活）: 帧长 {}",
                data.len()
            );
            accum.push_str(&data);
            if accum.contains("LIVE-DONE") {
                break;
            }
        }
    }
    assert_eq!(
        accum.matches('中').count(),
        3750,
        "「中」必须一个不多一个不少"
    );
}
