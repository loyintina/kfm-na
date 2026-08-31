//! direct_api_brain.rs — 本地直连脑（期 0② 主力，D11：本地脑是地基）。
//!
//! 契约真相源：docs/active/ai-presence.md §四B。
//! na 本地配 key 直连 provider（Kimi/智谱），rustls 纯 Rust TLS（ring 后端，
//! Android 交叉编译安全）+ http1.rs 手写 HTTP/1.1。
//! 翻译职责复刻 kfmv4 chat.ts：上游 OpenAI SSE → 四A 九事件（brain.rs 出管）。
//!
//! 分层：本模块碰 socket/TLS/线程，纯逻辑（解析/翻译/fuse）全在
//! brain.rs / http1.rs / providers.rs——本文件只是它们的装配工。

use crate::brain::{
    ChatEvent, OpenAiTranslator, SseParser, build_chat_request, error_event_from_http,
};
use crate::brain_ep::{BrainEndpoint, ChatStartReq, RunHandle};
use crate::http1::{BufIo, Request, serialize_request};
use crate::providers::{Provider, merge_env, parse_dotenv, parse_providers, resolve_key};
use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

/// 读超时：取消检查的心跳间隔（阻塞读不会永久睡死，每这么长醒一次看取消旗）
const READ_TICK: Duration = Duration::from_millis(500);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct DirectApiBrain {
    providers: Vec<Provider>,
    env: HashMap<String, String>,
    next_id: std::sync::atomic::AtomicU64,
}

impl DirectApiBrain {
    pub fn new(providers: Vec<Provider>, dotenv: HashMap<String, String>) -> Self {
        Self {
            providers,
            env: merge_env(&dotenv),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 从 kfmv4 风格配置文件装配（providers.json + .env）。
    pub fn from_files(providers_json: &str, dotenv_text: &str) -> Result<Self, String> {
        Ok(Self::new(
            parse_providers(providers_json)?,
            parse_dotenv(dotenv_text),
        ))
    }

    /// 立即失败流：配置层错误（provider 不存在/key 缺失）走 kfmv4 语义——
    /// 200 立即返 runId + SSE error 事件（人话 content），不例外不 panic。
    fn fail_stream(&self, content: String) -> (RunHandle, Receiver<ChatEvent>) {
        let (tx, rx) = channel();
        let handle = RunHandle::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let state = Arc::clone(&handle.state);
        thread::spawn(move || {
            let _ = tx.send(ChatEvent::Error { content });
            state.done.store(true, Ordering::Release);
        });
        (handle, rx)
    }
}

impl BrainEndpoint for DirectApiBrain {
    fn start(&self, req: ChatStartReq) -> (RunHandle, Receiver<ChatEvent>) {
        let Some(provider) = Provider::find(&self.providers, &req.provider) else {
            return self.fail_stream(format!(
                "provider 不存在: {}（providers.json 按 id/name 匹配，无静默回退）",
                req.provider
            ));
        };
        let key = match resolve_key(&provider.api_key_raw, &self.env) {
            Ok(k) => k,
            Err(e) => return self.fail_stream(format!("provider {}: {e}", provider.id)),
        };
        let handle = RunHandle::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = channel();
        let state = Arc::clone(&handle.state);
        let base_url = provider.base_url.clone();
        thread::spawn(move || {
            run_http(&base_url, &key, &req, &state, &tx);
            state.done.store(true, Ordering::Release);
        });
        (handle, rx)
    }

    fn cancel(&self, run: &RunHandle) -> bool {
        if run.is_done() {
            return false;
        }
        run.state.cancelled.store(true, Ordering::Release);
        true
    }

    fn attach(&self, _run: &RunHandle, _from: u64) -> Option<Receiver<ChatEvent>> {
        // 无缓冲直连：断线即重来（期 0 接受；server-brain 期 3 才有 5min 缓冲）
        None
    }
}

/// https://host[:port]/base/path → (host, port, base_path)
fn parse_https_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("只支持 https: {url}"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| format!("坏端口: {authority}"))?,
        ),
        None => (authority.to_string(), 443),
    };
    Ok((host, port, path.to_string()))
}

fn cancelled(state: &crate::brain_ep::RunState, tx: &Sender<ChatEvent>) -> bool {
    if state.cancelled.load(Ordering::Acquire) {
        let _ = tx.send(ChatEvent::Error {
            content: "已取消".to_string(),
        });
        true
    } else {
        false
    }
}

/// 一轮 HTTP 对话的全流程（在专用线程里跑）。任何失败都落成 Error 事件入流，
/// 不 panic——UI 面只见四A 事件。
fn run_http(
    base_url: &str,
    key: &str,
    req: &ChatStartReq,
    state: &crate::brain_ep::RunState,
    tx: &Sender<ChatEvent>,
) {
    if let Err(e) = run_http_inner(base_url, key, req, state, tx)
        && !cancelled(state, tx)
    {
        let _ = tx.send(ChatEvent::Error {
            content: format!("网络错误: {e}"),
        });
    }
}

fn run_http_inner(
    base_url: &str,
    key: &str,
    req: &ChatStartReq,
    state: &crate::brain_ep::RunState,
    tx: &Sender<ChatEvent>,
) -> Result<(), String> {
    let (host, port, base_path) = parse_https_url(base_url)?;
    // TCP + TLS
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS 解析失败 {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("DNS 无结果: {host}"))?;
    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("连接失败 {host}:{port}: {e}"))?;
    tcp.set_read_timeout(Some(READ_TICK))
        .map_err(|e| e.to_string())?;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(host.clone())
        .map_err(|e| format!("坏主机名 {host}: {e}"))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| format!("TLS 初始化失败: {e}"))?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);

    // 请求
    let path = format!("{}/chat/completions", base_path.trim_end_matches('/'));
    let body = build_chat_request(&req.model, &req.messages);
    let http_req = Request {
        method: "POST".into(),
        path,
        headers: vec![
            ("Host".into(), host.clone()),
            ("Authorization".into(), format!("Bearer {key}")),
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "text/event-stream".into()),
            ("Connection".into(), "close".into()),
        ],
        body: body.into_bytes(),
    };
    tls.write_all(&serialize_request(&http_req))
        .map_err(|e| format!("写请求失败: {e}"))?;

    // 取消钩子：每次读超时醒来（READ_TICK 心跳）检查取消旗，
    // 中了就以 Interrupted 中断读取——统一走下方中断处理
    let mut cancel_hook = || {
        if state.cancelled.load(Ordering::Acquire) {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled",
            ))
        } else {
            Ok(())
        }
    };
    /// Interrupted = 钩子判取消（区别于真 IO 错误）
    fn was_interrupted(e: &std::io::Error) -> bool {
        e.kind() == std::io::ErrorKind::Interrupted
    }

    // 响应头
    let mut io = BufIo::new(&mut tls);
    let head = match io.read_head_hook(&mut cancel_hook) {
        Ok(h) => h,
        Err(e) if was_interrupted(&e) => {
            cancelled(state, tx);
            return Ok(());
        }
        Err(e) => return Err(format!("读响应头失败: {e}")),
    };
    if head.status != 200 {
        let mut br = io.body_reader(head.body_kind());
        let mut body = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match br.read_body_hook(&mut buf, &mut cancel_hook) {
                Ok(0) => break,
                Ok(n) => {
                    body.extend_from_slice(&buf[..n]);
                    if body.len() >= 64 * 1024 {
                        break; // 错误体截断上限
                    }
                }
                Err(e) if was_interrupted(&e) => {
                    cancelled(state, tx);
                    return Ok(());
                }
                Err(e) => return Err(format!("读错误体失败: {e}")),
            }
        }
        let _ = tx.send(error_event_from_http(
            head.status,
            &String::from_utf8_lossy(&body),
        ));
        return Ok(());
    }

    // SSE 流：读一块喂一块，事件即产即推
    let mut br = io.body_reader(head.body_kind());
    let mut parser = SseParser::new();
    let mut translator = OpenAiTranslator::new();
    let mut buf = [0u8; 8192];
    loop {
        match br.read_body_hook(&mut buf, &mut cancel_hook) {
            Ok(0) => break, // body 终结
            Ok(n) => {
                parser.feed(&buf[..n]);
                for frame in parser.drain_frames() {
                    for ev in translator.translate_payload(&frame) {
                        let is_done = matches!(ev, ChatEvent::Done);
                        if tx.send(ev).is_err() {
                            return Ok(()); // 接收端走了
                        }
                        if is_done {
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) if was_interrupted(&e) => {
                cancelled(state, tx);
                return Ok(());
            }
            Err(e) => return Err(format!("读流中断: {e}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod url_spec {
    // 内联小考题：URL 解析（判卷成本倒挂不出独立文件）
    use super::parse_https_url;

    #[test]
    fn parse_https_urls() {
        assert_eq!(
            parse_https_url("https://api.kimi.com/coding/v1").unwrap(),
            ("api.kimi.com".to_string(), 443, "/coding/v1".to_string())
        );
        assert_eq!(
            parse_https_url("https://example.com:8443/").unwrap(),
            ("example.com".to_string(), 8443, "/".to_string())
        );
        assert_eq!(
            parse_https_url("https://example.com").unwrap(),
            ("example.com".to_string(), 443, "/".to_string())
        );
        assert!(parse_https_url("http://example.com").is_err());
    }
}
