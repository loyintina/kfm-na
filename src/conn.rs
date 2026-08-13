//! conn.rs — ws 连接层（B 档：tokio/tungstenite 胶水，纯逻辑在 session.rs）
//!
//! 职责：把 Session 状态机接到真实网络上。分层铁律：本文件不许长判断逻辑，
//! 遇到「该做个决定了」一律下沉 session.rs（那里有考题盯着）。
//!
//! 尖刺冒烟判卷（C 档）：手机实拍 field-reports.log 出现
//! [ws] connected / opened / output 预览 / exited 四格。
//!
//! 已知留白（尖刺后处理）：
//! - 协议级 ping/pong：tungstenite 读帧时自动排队 pong，但只在下次写/flush
//!   时才真发出去；长时间只读不写可能超 60s 被服务端判半开杀掉
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
