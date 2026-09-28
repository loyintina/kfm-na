//! wsterm.rs — terminal-pty WS 面（每连接一会话表，B 档胶水）
//!
//! 语义对齐 kfmv4 ws-server.ts + terminal-pty.ts：
//! - Open → spawn，回 Opened；Input → 写；Resize → 改窗；Close → 杀
//! - 会话归连接所有：连接断 = 全部杀（kfmv4 killAll(ws) 同款）
//! - 30s 应用层 ping（payload null，na 客户端 decode_server 已收纳）
//! - 子进程收割：child 归 Arc<Mutex> 共享——等死线程 try_wait 轮询
//!   （短锁不阻塞 kill），Exit 帧经主流发出（kfmv4 kill 也发 exit，对齐）

use std::collections::HashMap;
use std::io::Read as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use na_protocol::{ClientMsg, ServerMsg};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::pty_sess::{self, PtySession};
use crate::state::{Registry, SessionRec};

/// 出站事件（三条来源汇一条道：PTY 读线程 / 等死轮询线程 / ping 钟）
enum Out {
    /// 已增量解码的字符串（读线程持有 carry 续帧拼接，块界不劈字）
    Data(String, String),
    Exit(String, i32),
    Ping,
}

static NEXT_SID: AtomicU64 = AtomicU64::new(1);

fn next_sid() -> String {
    format!("s{}", NEXT_SID.fetch_add(1, Ordering::Relaxed))
}

fn now_epoch_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub async fn handle(stream: TcpStream, registry: Arc<Registry>) {
    registry.conn_open();
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(w) => w,
        Err(e) => {
            crate::logcap::throttled("ws-handshake", &format!("[na-server] ws 握手失败: {e}"));
            registry.conn_close();
            return;
        }
    };
    let (mut sink, mut src) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Out>();
    let mut sessions: HashMap<String, PtySession> = HashMap::new();
    let mut ping_clock = interval(Duration::from_secs(ping_secs()));
    // 心跳租约账（BAR-171 翻案·修复②）：任何入站帧（含协议级 pong）
    // 都盖戳；租约期无回执 = 对端死/冻结——**发送停摆判死的盲区补位**：
    // 安静会话无出站可停摆（缓冲永远填不满），只有「要回执」才逮得住
    let lease = Duration::from_secs(lease_secs());
    let mut last_rx = std::time::Instant::now();

    loop {
        tokio::select! {
            frame = src.next() => {
                let Some(Ok(msg)) = frame else { break }; // 连接断/帧错 → killAll
                last_rx = std::time::Instant::now(); // 入站即回执（pong 也算）
                let tokio_tungstenite::tungstenite::Message::Text(text) = msg else {
                    continue; // 二进制/pong 等不处理
                };
                match na_protocol::decode_client(&text) {
                    Ok(ClientMsg::Open { cwd, command, tag }) => {
                        let sid = next_sid();
                        match pty_sess::spawn(cwd.as_deref(), command.as_deref(), 80, 24) {
                            Ok((sess, mut reader)) => {
                                // 读线程：PTY 主端 → Out::Data（EOF/读错即终）。
                                // 增量 UTF-8 解码（utf8x）：carry 续帧拼接，
                                // 块界劈字不再产 U+FFFD；EOF 时尾巴强制出场
                                {
                                    let tx2 = tx.clone();
                                    let sid2 = sid.clone();
                                    std::thread::spawn(move || {
                                        // 读块大小：默认 8KB；NA_READ_BUF 是判卷仪器
                                        // 旋钮（live 考题拧小强制块界劈字，生产不
                                        // 设）。下限 4 = 容得下任一完整 UTF-8 字符。
                                        let read_sz = std::env::var("NA_READ_BUF")
                                            .ok()
                                            .and_then(|v| v.parse::<usize>().ok())
                                            .map(|v| v.clamp(4, 65536))
                                            .unwrap_or(8192);
                                        let mut buf = vec![0u8; read_sz];
                                        let mut carry: Vec<u8> = Vec::new();
                                        loop {
                                            match reader.read(&mut buf) {
                                                Ok(0) | Err(_) => break,
                                                Ok(n) => {
                                                    let (text, rest) =
                                                        crate::utf8x::utf8_feed(&carry, &buf[..n]);
                                                    carry = rest;
                                                    if text.is_empty() {
                                                        continue; // 半截序列攒着，不发空帧
                                                    }
                                                    if tx2.send(Out::Data(sid2.clone(), text)).is_err() {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        // EOF：尾巴不是完整序列，替换符强制出场不吞字节
                                        if !carry.is_empty() {
                                            let (mut text, _) = crate::utf8x::utf8_feed(&[], &carry);
                                            if text.is_empty() {
                                                text = "\u{FFFD}".into();
                                            }
                                            let _ = tx2.send(Out::Data(sid2, text));
                                        }
                                    });
                                }
                                // 等死线程：try_wait 短锁轮询（不阻塞 kill）
                                {
                                    let child = Arc::clone(&sess.child);
                                    let tx2 = tx.clone();
                                    let sid2 = sid.clone();
                                    std::thread::spawn(move || {
                                        loop {
                                            {
                                                let mut c = child.lock().unwrap_or_else(PoisonError::into_inner);
                                                match c.try_wait() {
                                                    Ok(Some(code)) => {
                                                        let _ = tx2.send(Out::Exit(sid2, code));
                                                        break;
                                                    }
                                                    Ok(None) => {}
                                                    Err(_) => {
                                                        let _ = tx2.send(Out::Exit(sid2, -1));
                                                        break;
                                                    }
                                                }
                                            }
                                            std::thread::sleep(Duration::from_millis(100));
                                        }
                                    });
                                }
                                registry.register(SessionRec {
                                    id: sid.clone(),
                                    cmd: sess.cmd_label.clone(),
                                    cols: 80,
                                    rows: 24,
                                    opened_epoch_s: now_epoch_s(),
                                    last_active_epoch_s: now_epoch_s(),
                                });
                                sessions.insert(sid.clone(), sess);
                                send(&mut sink, &ServerMsg::Opened { session_id: sid, tag }).await;
                            }
                            Err(e) => {
                                send(&mut sink, &ServerMsg::Error { message: format!("spawn 失败: {e}") }).await;
                            }
                        }
                    }
                    Ok(ClientMsg::Input { session_id, input }) => {
                        registry.touch(&session_id); // 真空闲：敲键即活动
                        if let Some(s) = sessions.get_mut(&session_id) {
                            use std::io::Write as _;
                            let _ = s.writer.write_all(input.as_bytes());
                            let _ = s.writer.flush();
                        }
                    }
                    Ok(ClientMsg::Resize { session_id, cols, rows }) => {
                        if let Some(s) = sessions.get(&session_id) {
                            let _ = s.resize(cols, rows);
                        }
                    }
                    Ok(ClientMsg::Close { session_id }) => {
                        kill_one(&mut sessions, &registry, &session_id);
                    }
                    Err(e) => {
                        send(&mut sink, &ServerMsg::Error { message: e.to_string() }).await;
                    }
                }
            }
            Some(out) = rx.recv() => {
                match out {
                    Out::Data(sid, data) => {
                        registry.touch(&sid); // 真空闲：产出即活动
                        if !send(&mut sink, &ServerMsg::Output { session_id: sid, data }).await { break; }
                    }
                    Out::Exit(sid, code) => {
                        registry.unregister(&sid);
                        sessions.remove(&sid);
                        if !send(&mut sink, &ServerMsg::Exit { session_id: sid, code }).await { break; }
                    }
                    Out::Ping => {
                        // 协议级 ws Ping 先发（BAR-171 翻案·修复②）：
                        // tungstenite 客户端自动 pong（实锤见 na 侧
                        // conn.rs 心跳注释）——回执盖 last_rx 的戳；
                        // 应用层 ServerMsg::Ping 照旧（客户端 decode_server
                        // 已收纳，租约不依赖它）
                        if !send_raw(
                            &mut sink,
                            tokio_tungstenite::tungstenite::Message::Ping(
                                tokio_tungstenite::tungstenite::Bytes::new(),
                            ),
                        )
                        .await
                        {
                            break;
                        }
                        if !send(&mut sink, &ServerMsg::Ping).await { break; }
                    }
                }
            }
            _ = ping_clock.tick() => {
                // 租约先于 ping 判：已死的连接不配再拿心跳
                if lease_expired(last_rx.elapsed(), lease) {
                    crate::logcap::throttled(
                        "ws-lease",
                        &format!(
                            "[na-server] ws 心跳租约 {}s 无回执，判死收尸",
                            lease.as_secs()
                        ),
                    );
                    break;
                }
                if tx.send(Out::Ping).is_err() { break; }
            }
        }
    }

    // killAll(ws) 同款：连接终结 = 名下会话全杀
    let ids: Vec<String> = sessions.keys().cloned().collect();
    for id in ids {
        kill_one(&mut sessions, &registry, &id);
    }
    registry.conn_close();
}

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<TcpStream>,
    tokio_tungstenite::tungstenite::Message,
>;

/// 发送停摆判死秒数（BAR-171）：env NA_SEND_STALL_SECS 可拧（考题用），
/// 缺省 45——30s ping 一拍半的余量。对端不排水的唯一合法长因是手机
/// 死/冻结（QUIC 流控窗满、桥停抽、TCP 缓冲塞死）；手机醒来走
/// BAR-140/141 重孵链几秒回活，比养 4h 僵尸便宜得多。
fn send_stall_secs() -> u64 {
    std::env::var("NA_SEND_STALL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(45)
}

/// 心跳间隔秒数（BAR-171 翻案·修复②）：env NA_PING_SECS 可拧（考题用），
/// 缺省 30——与 kfmv4 应用层 ping 同拍
fn ping_secs() -> u64 {
    std::env::var("NA_PING_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// 心跳租约秒数（BAR-171 翻案·修复②）：env NA_LEASE_SECS 可拧（考题用），
/// 缺省 90 = 3 个心跳拍——手机省电冻结超过这个时长，重孵比养着便宜
fn lease_secs() -> u64 {
    std::env::var("NA_LEASE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(90)
}

/// 租约判满（A 档纯函数，钉走 tests/bar171_spec.rs）：距最后一次
/// 入站回执达到租约期 = 对端死/冻结
pub fn lease_expired(elapsed: Duration, limit: Duration) -> bool {
    elapsed >= limit
}

/// 发一帧原始 ws 消息（协议级 Ping 也走停摆判死——同 send 的账；
/// 保持 timeout(stall, fut) 形态：bar171 接线守卫钉认这个写法）
async fn send_raw(sink: &mut WsSink, msg: tokio_tungstenite::tungstenite::Message) -> bool {
    let stall = Duration::from_secs(send_stall_secs());
    let fut = sink.send(msg);
    match tokio::time::timeout(stall, fut).await {
        Ok(r) => r.is_ok(),
        Err(_) => {
            crate::logcap::throttled(
                "ws-send-stall",
                &format!("[na-server] ws 发送停摆 {}s，判死收尸", stall.as_secs()),
            );
            false
        }
    }
}

/// 发一帧；返回是否还活着（false = 对端已断/停摆判死，调用方应 break）
///
/// BAR-171：发送无超时的年代，对端停摆 = send 挂死 = 整个会话循环
/// 陪葬（ping 发不出、close 收不到、killAll 永不触发），僵尸 ws 会话
/// 撑满 QUIC idle 4h 上限——fd/pty/附着壳全套泄漏的传导起点。
async fn send(sink: &mut WsSink, msg: &ServerMsg) -> bool {
    send_raw(sink, na_protocol::encode_server(msg).into()).await
}

/// 杀一条会话：先登记注销，再杀子进程（等死线程会补发 Exit——
/// 但会话已出册，Exit 只走 ws 不再碰 registry，幂等）
fn kill_one(sessions: &mut HashMap<String, PtySession>, registry: &Registry, sid: &str) {
    registry.unregister(sid);
    if let Some(s) = sessions.remove(sid) {
        let _ = s
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kill();
    }
}
