//! conn.rs — ws 连接层（B 档：tokio/tungstenite 胶水，纯逻辑在 session.rs）
//!
//! 职责：把 Session 状态机接到真实网络上。分层铁律：本文件不许长判断逻辑，
//! 遇到「该做个决定了」一律下沉 session.rs（那里有考题盯着）。
//!
//! 尖刺冒烟判卷（C 档）：手机实拍 field-reports.log 出现
//! [ws] connected / opened / output 预览 / exited 四格。
//!
//! 已知留白（尖刺后处理）：
//! - echo_roundtrip（一次性冒烟路径）不主动冲 pong：30s 超时内必跑完，无感；
//!   常驻路径 spawn_terminal_session 已在 Ping 时主动 flush（函数文档有实锤）
//! - 明文 ws://：尖刺期直连 80，正式上 wss（nginx 443 证书）

use crate::protocol::{self, ClientMsg};
use crate::session::{Session, SessionEvent};

/// 会话闭环结果（live 测试与冒烟共用一条驱动路径）
#[derive(Debug)]
pub struct EchoRun {
    pub session_id: String,
    /// 按序收集的 output 数据
    pub outputs: Vec<String>,
    pub exit_code: i32,
}

/// 核心驱动：连 url → 开会话(command) → 收 output 直到 exit。
/// 每一步事件回调给 on_event（冒烟=上报，测试=断言收集）。
pub async fn echo_roundtrip(
    url: &str,
    command: &str,
    on_event: &mut dyn FnMut(&str),
) -> Result<EchoRun, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    async fn send_msg<S, E>(ws: &mut S, msg: ClientMsg) -> Result<(), String>
    where
        S: futures_util::Sink<Message, Error = E> + Unpin,
        E: std::fmt::Display,
    {
        let text = protocol::encode_client(&msg);
        SinkExt::send(ws, Message::Text(text.into()))
            .await
            .map_err(|e| format!("发送失败: {e}"))
    }

    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    on_event("connected");

    let mut session = Session::new();
    send_msg(&mut ws, Session::open_msg(Some(command))).await?;

    let mut session_id = None;
    let mut outputs = Vec::new();
    while let Some(frame) = ws.next().await {
        let text = match frame {
            Ok(Message::Text(t)) => t,
            Ok(_) => continue, // 二进制/ping/pong：跳过
            Err(e) => return Err(format!("读帧失败: {e}")),
        };
        let msg = protocol::decode_server(text.as_str()).map_err(|e| format!("解码失败: {e}"))?;
        match session.on_server(msg) {
            Some(SessionEvent::Opened { session_id: id }) => {
                on_event("opened");
                session_id = Some(id);
            }
            Some(SessionEvent::Output { data }) => {
                on_event("output");
                outputs.push(data);
            }
            Some(SessionEvent::Exited { code }) => {
                on_event("exited");
                return Ok(EchoRun {
                    session_id: session_id.ok_or("exit 先于 opened")?,
                    outputs,
                    exit_code: code,
                });
            }
            Some(SessionEvent::Failed { message }) => {
                return Err(format!("会话失败: {message}"));
            }
            None => {}
        }
    }
    Err("服务端先关闭连接".into())
}

/// 冒烟入口（Android 起线程调）：echo 闭环 + 每事件飞鸽传书
/// 各阶段用 report_sync 独立直发（绕开队列——队首阻塞时冒烟不受影响）；
/// 全程 30s 超时兜底，失败有声
pub fn spawn_smoke(url: &'static str, command: &'static str) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("建 tokio runtime 失败");
        rt.block_on(async move {
            crate::report::report_sync("ws", "冒烟线程启动");
            let mut report = |stage: &str| crate::report::report_sync("ws", stage);
            let run = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                echo_roundtrip(url, command, &mut report),
            )
            .await;
            match run {
                Ok(Ok(run)) => crate::report::report_sync(
                    "ws",
                    &format!(
                        "闭环成功: session={} exit={} 输出预览={:.80}",
                        run.session_id,
                        run.exit_code,
                        run.outputs.concat()
                    ),
                ),
                Ok(Err(e)) => crate::report::report_sync("ws", &format!("闭环失败: {e}")),
                Err(_) => crate::report::report_sync("ws", "闭环超时（30s）"),
            }
        });
    });
}

/// 常驻会话的出向命令（应用侧 → ws 线程）
#[derive(Debug)]
pub enum TermCmd {
    /// terminal-input 的按键字节串（Opened 之前先缓存，绑定后按序补发）
    Input(String),
    /// terminal-resize；只留最新值，Opened 时补发一次
    Resize { cols: u32, rows: u32 },
    /// terminal-close
    Close,
}

/// 常驻终端会话驱动（切片：手机启动即进交互 shell）。
///
/// 线程模型与 spawn_smoke 同款：独立线程 + tokio current_thread runtime。
/// 双向：
/// - ws 读循环：Opened/Output/Exited/Failed 经 Session 状态机转成 SessionEvent
///   回调给 inbound（闭包运行在 ws 线程——跨线程交付由调用方自己桥，如 mpsc）
/// - outbound_rx：TermCmd 翻译成出向协议消息；Opened 前 Input 缓存、
///   Resize 只留最新（Session 门禁 None 时不丢不烂）
///
/// 30s 心跳实锤（tungstenite-0.26.2 src/protocol/mod.rs:647 + tokio-tungstenite
/// src/lib.rs:286）：服务端协议级 ping 到达时 tungstenite 自动把 pong 排进
/// additional 写队列，但只在「下一次写/flush」才真正发出——纯读路径不冲。
/// 本驱动在读到 Message::Ping 时主动 flush 一次把队列里的 pong 顶出去，
/// 空闲终端（用户不敲键盘=无写）也不会被 60s 半开判杀。
pub fn spawn_terminal_session(
    url: &'static str,
    command: Option<String>,
    inbound: impl FnMut(SessionEvent) + Send + 'static,
) -> std::sync::mpsc::Sender<TermCmd> {
    spawn_terminal_session_owned(url.to_string(), command, inbound)
}

/// String 版 spawn（工厂层用：配置来自 ConnConfig 而非编译期字面量）
fn spawn_terminal_session_owned(
    url: String,
    command: Option<String>,
    mut inbound: impl FnMut(SessionEvent) + Send + 'static,
) -> std::sync::mpsc::Sender<TermCmd> {
    let (tx, rx) = std::sync::mpsc::channel::<TermCmd>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("建 tokio runtime 失败");
        rt.block_on(async move {
            if let Err(e) = run_terminal_session(&url, command, &mut inbound, rx).await {
                inbound(SessionEvent::Failed { message: e });
            }
        });
    });
    tx
}

// ---- 工厂层（连接 provider 设计页 §2；插件化边界，行为零变化） ----

/// 连接配置：连哪、开什么命令（None = 交互 shell）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnConfig {
    pub url: String,
    pub command: Option<String>,
}

impl Default for ConnConfig {
    /// 默认 = 现状硬编码（android_app 启动即连的回环 8021），行为零变化的锚
    fn default() -> Self {
        ConnConfig {
            url: "ws://127.0.0.1:8021/ws".into(),
            command: None,
        }
    }
}

/// 一次已建立的终端连接：裸通道对，不含任何插件可蒸发状态——
/// 归调用方持有，跨插件生命周期存活（设计页 §7 状态存活，评审裁决 2/3）
pub struct TermHandle {
    /// 应用 → 连接
    pub outbound: std::sync::mpsc::Sender<TermCmd>,
    /// 连接 → 应用（服务内部数据通道，非插件事件——设计页 §6 措辞钉死）
    pub events: std::sync::mpsc::Receiver<SessionEvent>,
}

/// 连接工厂服务（服务键 `dyn TermFactory`，注册表式、独占绑定 v1）。
/// spawn 瞬时返回：只开线程建通道，握手在线程里异步发生。
pub trait TermFactory: Send + Sync {
    /// 插件配置表解析出的默认连接参数（配置变更 = 自我重载换新工厂）
    fn default_config(&self) -> ConnConfig;
    /// 建一条连接；事件桥收进工厂内部，调用方只拿 TermHandle
    fn spawn(&self, config: &ConnConfig) -> TermHandle;
}

/// transport 注入缝（评审裁决 4）：真实路径 = ws 线程，测试 = 假 transport
pub type Spawner = std::sync::Arc<dyn Fn(ConnConfig) -> TermHandle + Send + Sync>;

/// 真实 ws transport：内部建 mpsc 事件桥（原 android_app 手工工序收敛至此），
/// 驱动仍是 spawn_terminal_session 同款的独立线程 + current_thread runtime
pub fn ws_spawner() -> Spawner {
    std::sync::Arc::new(|cfg: ConnConfig| {
        let (event_tx, event_rx) = std::sync::mpsc::channel::<SessionEvent>();
        let outbound = spawn_terminal_session_owned(cfg.url, cfg.command, move |ev| {
            // 主循环死了发送失败：吞掉——ws 线程绝不为上报陪葬
            let _ = event_tx.send(ev);
        });
        TermHandle {
            outbound,
            events: event_rx,
        }
    })
}

/// ws 连接工厂：捕获默认配置 + transport 缝
pub struct WsTermFactory {
    default: ConnConfig,
    spawner: Spawner,
}

impl WsTermFactory {
    pub fn new(default: ConnConfig, spawner: Spawner) -> Self {
        WsTermFactory { default, spawner }
    }
}

impl TermFactory for WsTermFactory {
    fn default_config(&self) -> ConnConfig {
        self.default.clone()
    }
    fn spawn(&self, config: &ConnConfig) -> TermHandle {
        (self.spawner)(config.clone())
    }
}

/// 手工 ws 握手（BAR-022 归因定案版：std 阻塞式——把 tokio/epoll 从握手
/// 路径上整个拿掉，字节确实晚到应用 socket 与 tokio 无关；首连 ~2.1s 是
/// 冷进程一次性唤醒成本，第二条连接即 ~0.2s，故不做预演只提前 spawn）。
/// 校验从简：只认 101 状态行；之后用 from_partially_read 接管残流续帧。
/// 内部四段耗时收进一条 summary，report() 异步上报（不经阻塞，不拖提示符）
async fn ws_handshake(
    url: &str,
) -> Result<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, String> {
    use std::io::{Read, Write};
    use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};

    let (authority, path) = url
        .split("://")
        .nth(1)
        .map(|rest| match rest.split_once('/') {
            Some((h, p)) => (h.to_string(), format!("/{p}")),
            None => (rest.to_string(), "/".into()),
        })
        .ok_or_else(|| format!("URL 无法解析: {url}"))?;

    let t0 = std::time::Instant::now();
    let mut tcp = std::net::TcpStream::connect(&authority)
        .map_err(|e| format!("TCP 连接失败({authority}): {e}"))?;
    let tcp_ms = t0.elapsed().as_millis();
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("设写超时失败: {e}"))?;

    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    tcp.write_all(req.as_bytes())
        .map_err(|e| format!("发送升级请求失败: {e}"))?;
    let write_ms = t0.elapsed().as_millis();

    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    let mut first_ms = None;
    loop {
        let n = tcp
            .read(&mut chunk)
            .map_err(|e| format!("读握手响应失败: {e}"))?;
        if n == 0 {
            return Err("握手响应被提前关闭".into());
        }
        if first_ms.is_none() {
            first_ms = Some(t0.elapsed().as_millis());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 4096 {
            return Err("握手响应头异常肥大".into());
        }
    }
    let head_ms = t0.elapsed().as_millis();
    let head = String::from_utf8_lossy(&buf);
    if !head.starts_with("HTTP/1.1 101") {
        return Err(format!(
            "升级失败，状态行: {}",
            head.lines().next().unwrap_or("(空)")
        ));
    }
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let rest = buf[sep + 4..].to_vec();

    tcp.set_nonblocking(true)
        .map_err(|e| format!("转非阻塞失败: {e}"))?;
    let tcp = tokio::net::TcpStream::from_std(tcp).map_err(|e| format!("转 tokio 流失败: {e}"))?;
    let ws = tokio_tungstenite::WebSocketStream::from_partially_read(
        tcp,
        rest,
        Role::Client,
        Some(WebSocketConfig::default()),
    )
    .await;
    // 异步上报（BAR-022）：sync 直发会卡握手线程 ~0.3s 拖慢提示符；
    // 队列丢失可接受——这段归因数据只是遥测，连接提示行已让等待无感
    crate::report::report(
        "conn",
        &format!(
            "ws 握手 {} (TCP={tcp_ms}ms 写请求={write_ms}ms 首字节+{}ms 头读完+{}ms)",
            head_ms,
            first_ms.map(|v| v - write_ms).unwrap_or(0),
            head_ms - write_ms
        ),
    );
    Ok(ws)
}

/// 常驻驱动核心（异步）：连 url → 开会话 → select 双向泵，直到 Exited/Failed/断线
async fn run_terminal_session(
    url: &str,
    command: Option<String>,
    inbound: &mut impl FnMut(SessionEvent),
    outbound_rx: std::sync::mpsc::Receiver<TermCmd>,
) -> Result<(), String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    // std mpsc → tokio 无界桥（select! 要异步流；转发线程随 rx 断开自然死）
    let (otx, mut orx) = tokio::sync::mpsc::unbounded_channel::<TermCmd>();
    std::thread::spawn(move || {
        while let Ok(cmd) = outbound_rx.recv() {
            if otx.send(cmd).is_err() {
                break; // ws 线程已退，出向无人收——应用侧僵尸发送被吞
            }
        }
    });

    async fn send_msg<S, E>(ws: &mut S, msg: ClientMsg) -> Result<(), String>
    where
        S: futures_util::Sink<Message, Error = E> + Unpin,
        E: std::fmt::Display,
    {
        let text = protocol::encode_client(&msg);
        SinkExt::send(ws, Message::Text(text.into()))
            .await
            .map_err(|e| format!("发送失败: {e}"))
    }

    // BAR-022 定案：std 阻塞式握手直接跑真会话（首连 ~2.1s 是冷进程一次性
    // 唤醒成本，预演只会叠加；连接已提前到基座就绪即刻，与建终端并行）。
    // summary 已由 ws_handshake 内部异步上报；此处保留常驻里程碑锚点
    let t0 = std::time::Instant::now();
    let mut ws = ws_handshake(url).await?;
    crate::report::report(
        "conn",
        &format!(
            "ws 握手完成 +{}ms (升级段 {}ms)",
            crate::report::boot_ms(),
            t0.elapsed().as_millis()
        ),
    );

    let mut session = Session::new();
    send_msg(&mut ws, Session::open_msg(command.as_deref())).await?;

    // Opened 前的出向缓存：Input 全留（按键有序），Resize 只留最新
    let mut pending_input: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut last_resize: Option<(u32, u32)> = None;
    let mut outbound_open = true;

    loop {
        tokio::select! {
            frame = ws.next() => {
                let text = match frame {
                    Some(Ok(Message::Text(t))) => t,
                    // ping：flush 把 tungstenite 自动排的 pong 顶出去（心跳实锤见上）
                    Some(Ok(Message::Ping(_))) => {
                        let _ = SinkExt::flush(&mut ws).await;
                        continue;
                    }
                    Some(Ok(_)) => continue, // 二进制/pong/close：跳过
                    Some(Err(e)) => return Err(format!("读帧失败: {e}")),
                    None => return Err("服务端先关闭连接".into()),
                };
                let msg = protocol::decode_server(text.as_str())
                    .map_err(|e| format!("解码失败: {e}"))?;
                match session.on_server(msg) {
                    Some(SessionEvent::Opened { session_id }) => {
                        inbound(SessionEvent::Opened { session_id });
                        // 补发缓存：先 resize（PTY 尺寸尽早对），再按序灌 input
                        if let Some((cols, rows)) = last_resize.take()
                            && let Some(m) = session.resize_msg(cols, rows)
                        {
                            send_msg(&mut ws, m).await?;
                        }
                        while let Some(input) = pending_input.pop_front() {
                            if let Some(m) = session.input_msg(&input) {
                                send_msg(&mut ws, m).await?;
                            }
                        }
                    }
                    Some(ev @ SessionEvent::Output { .. }) => inbound(ev),
                    Some(ev @ SessionEvent::Exited { .. }) => {
                        inbound(ev);
                        return Ok(());
                    }
                    Some(ev @ SessionEvent::Failed { .. }) => {
                        // 事件已上交，回 Ok 避免 spawn 包装层再报一次 Failed
                        inbound(ev);
                        return Ok(());
                    }
                    None => {}
                }
            }
            cmd = orx.recv(), if outbound_open => {
                match cmd {
                    Some(TermCmd::Input(input)) => {
                        match session.input_msg(&input) {
                            Some(m) => send_msg(&mut ws, m).await?,
                            None => pending_input.push_back(input), // 未 Live：缓存
                        }
                    }
                    Some(TermCmd::Resize { cols, rows }) => {
                        last_resize = Some((cols, rows));
                        if let Some(m) = session.resize_msg(cols, rows) {
                            send_msg(&mut ws, m).await?;
                        }
                    }
                    Some(TermCmd::Close) => {
                        if let Some(m) = session.close_msg() {
                            send_msg(&mut ws, m).await?;
                        }
                        // 不主动断：等服务端 terminal-exit 走正常收尾
                    }
                    // 应用侧弃管（UI 死了）：标记关闭防 select 空转，继续读到会话终
                    None => outbound_open = false,
                }
            }
        }
    }
}
