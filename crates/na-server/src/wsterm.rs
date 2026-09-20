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
            eprintln!("[na-server] ws 握手失败: {e}");
            registry.conn_close();
            return;
        }
    };
    let (mut sink, mut src) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Out>();
    let mut sessions: HashMap<String, PtySession> = HashMap::new();
    let mut ping_clock = interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            frame = src.next() => {
                let Some(Ok(msg)) = frame else { break }; // 连接断/帧错 → killAll
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
                        if !send(&mut sink, &ServerMsg::Ping).await { break; }
                    }
                }
            }
            _ = ping_clock.tick() => {
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

/// 发一帧；返回是否还活着（false = 对端已断，调用方应 break）
async fn send(sink: &mut WsSink, msg: &ServerMsg) -> bool {
    sink.send(na_protocol::encode_server(msg).into())
        .await
        .is_ok()
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
