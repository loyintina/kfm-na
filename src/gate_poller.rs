//! gate_poller.rs — 闸门触发腿（BAR-233 根治案：共享腿连接直开流，
//! 追踪信 NA0163；白露 21 楼批准「poller 改进程内直连：经 na-quic 的
//! QUIC 连接对象直接开流说话，不落本机 TCP」，返工裁定共享腿连接）。
//!
//! 下行：长轮询 na-server `GET /api/gate/pending?wait=2`——**不再借道
//! 本机 TCP 口**（旧路 127.0.0.1:{local_port} 的存在性依赖隧道腿，QUIC
//! 态下该口由腿间接供出、不可靠：判卷实证 Connection refused 后裸
//! connect 挂死＝永静默；master baab276 的 2s 超时+tick 心跳是诊断兼
//! 过渡，本案根治）。每拍从 [`crate::tunnel::DATA_CONN_SLOT`] 取当前
//! 数据腿的 QUIC 连接对象，直接 open_bi 开流说话，取到的触发原子落盘
//! DUMP_DIR（`.new` 写全 → rename，照抄 gate-lib 语义）→ 既有 300ms
//! 值守线程照常消费——闸门不再依赖本机任何 TCP 监听。
//! 上行：gate 各通道写完结果文件调 [`offer_result`]，由本模块上传
//! 线程 POST `/api/gate/result/<name>` 原样字节（脚本 GET 一次性取走，
//! 替代 ssh cat/scp 拉取）。
//!
//! **共享腿连接而非专用连接**（返工裁定）：na-quic run_server 的正连
//! 顶替语义（BAR-171，lib.rs 认领段：新连接第一条验签通过的流落地即收
//! 旧连接）下，poller 的第二条专用连接会与隧道腿连接互踢——腿被踢死
//! 看门狗记账，3 次即跳闸把数据面降级 ssh。共享一条连接则零冲突：QUIC
//! 多流原生复用，poller 的 HTTP 短流与 ws 长流同连接共存，且**腿死随
//! 腿愈**——腿死→槽里留死连接→open_bi 即错→退避；看门狗重拉腿握手
//! 成功→新 Connection 覆盖槽→poller 自愈。已知代价：闸门与数据面同
//! 生共死（QUIC 跳闸降 ssh 期闸门盲，直到回切探测重拉腿）。
//!
//! 失败静默重试（退避 1s 起指数封顶 30s），**状态转换才报表**——防
//! 「不可达」30s 一行刷屏。tick 心跳每 30 拍一行（判据挂 field-reports
//! 断档——「成功静默」设计让僵死与空转不可分，这行是解药）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::settings::ServerEntry;

/// 轮询等待（秒）——2026-10-07 判卷回炉改 2（白露 NA0163 7 楼裁①）：
/// 25s 长轮询在 QUIC 桥上撞空闲掐线（现场实证「响应无头体分隔」反复），
/// 短轮询掐线窗口消失、闸门 2s 延迟无感、失败重试成本恒定
pub const POLL_WAIT_SECS: u8 = 2;
/// 上传重试次数（超过即丢＋报表一行）
const UPLOAD_RETRIES: u32 = 3;
/// 重试退避：1s 起指数、30s 封顶——不爆炸，病态网络下每分钟至少敲一次门
const BACKOFF_START_SECS: u64 = 1;
const BACKOFF_MAX_SECS: u64 = 30;

/// 十通道白名单（与 crates/na-server/src/gateq.rs CHANNELS 同源两份，
/// 改要同票——注释互指；BAR-240 补 switch-req：缺它远程切换被 400
/// 逼成 keys-in 盲发，MAIN0134 实案）
const CHANNELS: [&str; 10] = [
    "shot-req",
    "shot-gles-req",
    "text-req",
    "keys-in",
    "ping-req",
    "restart-req",
    "trace-req",
    "stats-req",
    "orb-inject",
    "switch-req",
];

fn channel_ok(ch: &str) -> bool {
    CHANNELS.contains(&ch)
}

static STARTED: OnceLock<()> = OnceLock::new();

/// 上传通道（offer_result 投件 → 上传线程消费）。未 start 时
/// offer_result 为 no-op（STARTED 未设）。tokio 无界信道：投递端在同步
/// 上下文（gate 通道线程）零阻塞，消费端在上传线程的 async 循环里 await。
static UPLOAD_TX: OnceLock<tokio::sync::mpsc::UnboundedSender<(String, Vec<u8>)>> = OnceLock::new();

pub fn start(srv: ServerEntry) {
    start_with(
        srv,
        crate::tunnel::NA_SERVER_PORT,
        crate::tunnel::data_conn_slot(),
        PathBuf::from(crate::gate::DUMP_DIR),
    );
}

/// 测试注入口：target_port＝QUIC 桥流头里的目标口（生产恒 NA_SERVER_PORT
/// 9021＝na-server；测试注入随机口——开发机 9021 常被真 na-server 占用，
/// 固定绑口不可行）、slot＝腿连接槽（生产用 tunnel::DATA_CONN_SLOT——
/// start 经 data_conn_slot() 取真槽；测试自连 QUIC client 写自己的槽，
/// 见 gate_poller_quic_spec）、dump_dir 可指定（start 幂等；start_with
/// 供测试直用）
pub fn start_with(
    srv: ServerEntry,
    target_port: u16,
    slot: Arc<Mutex<Option<quinn::Connection>>>,
    dump_dir: PathBuf,
) {
    // psk start 时 parse 一次缓存（auth_tag 用——连接归腿建，poller 只
    // 开流；pin 不需要）。缺件不 set STARTED：配置修好、设置重载再调
    // start 还能起来
    let Some(psk) = crate::tunnel::parse_pin(&srv.quic.psk) else {
        crate::report::report("gatepoll", "QUIC psk 缺件（64 位小写 hex），闸门轮询不启动");
        return;
    };
    if STARTED.set(()).is_err() {
        crate::report::report("gatepoll", "轮询已在跑，忽略重复启动");
        return;
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<(String, Vec<u8>)>();
    let _ = UPLOAD_TX.set(tx);
    let host = srv.ssh.host.clone();
    crate::report::report(
        "gatepoll",
        &format!(
            "闸门 QUIC 轮询启动（BAR-233 根治：共享腿连接直开流）：腿连接槽 → 桥目标口 {target_port} → DUMP_DIR"
        ),
    );
    let host_up = host.clone();
    let slot_up = slot.clone();
    std::thread::spawn(move || poll_loop(psk, target_port, host, slot, dump_dir));
    std::thread::spawn(move || upload_loop(psk, target_port, host_up, slot_up, rx));
}

/// HTTP/1.1 请求字节构造（A 档纯函数，钉在 gate_poller_spec）：
/// method/path/Host/Content-Length/Connection: close——与旧 TCP 路同构
pub fn build_request(method: &str, path: &str, body_len: usize, host: &str) -> Vec<u8> {
    format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n")
        .into_bytes()
}

/// 单次 HTTP 往返（QUIC 版的旧 http_call）：腿连接对象上开一条 bi 流 →
/// **帧序必须是 端口头(port_header)＋认证标签(auth_tag) 先于 HTTP 字节**
/// （服务器桥逐流消费这 34 字节：路由到 127.0.0.1:{target_port}＋验
/// 客户端证，psk 不符整流弃掉）→ 写请求 → 半关（FIN＝请求写完，服务器
/// splice 见 EOF 转发断言）→ 读流至 EOF（Connection: close → 读尽即体）
/// → 返回体字节（剥 \r\n\r\n 头，同旧逻辑）。整体限时（旧路 read_timeout
/// 同量级）——不挂死防线：连接死/读写卡死都在限时内返回，调用方退避。
async fn quic_http_roundtrip(
    conn: &quinn::Connection,
    psk: &[u8; 32],
    target_port: u16,
    method: &str,
    path: &str,
    body: &[u8],
    host: &str,
) -> Result<Vec<u8>, String> {
    tokio::time::timeout(Duration::from_secs(POLL_WAIT_SECS as u64 + 10), async {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| format!("open_bi: {e}"))?;
        send.write_all(&na_quic::port_header(target_port))
            .await
            .map_err(|e| format!("写端口头: {e}"))?;
        send.write_all(&na_quic::auth_tag(psk, target_port))
            .await
            .map_err(|e| format!("写认证标签: {e}"))?;
        send.write_all(&build_request(method, path, body.len(), host))
            .await
            .map_err(|e| format!("写请求: {e}"))?;
        if !body.is_empty() {
            send.write_all(body)
                .await
                .map_err(|e| format!("写体: {e}"))?;
        }
        send.finish().map_err(|e| format!("半关: {e}"))?;
        let raw = recv
            .read_to_end(64 * 1024 * 1024)
            .await
            .map_err(|e| format!("读响应: {e}"))?;
        // 分头体（Connection: close → 读尽即体）
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("响应无头体分隔")?;
        Ok(raw[split + 4..].to_vec())
    })
    .await
    .map_err(|_| "往返超时".to_string())?
}

async fn fetch_pending(
    conn: &quinn::Connection,
    psk: &[u8; 32],
    target_port: u16,
    host: &str,
) -> Result<Vec<u8>, String> {
    quic_http_roundtrip(
        conn,
        psk,
        target_port,
        "GET",
        &format!("/api/gate/pending?wait={POLL_WAIT_SECS}"),
        &[],
        host,
    )
    .await
}

/// 轮询线程：std::thread 外壳 + 线程内单件 current_thread runtime 把整个
/// 循环搬进 async（open_bi/读写全 async，退避睡 tokio sleep——学
/// tunnel.rs 腿线程的做法，一次建 runtime 常驻到底）。连接管理＝每拍
/// 从槽取当前腿连接：None＝腿还没握上手（等腿握手）；open_bi/读写错＝
/// 连接已死（腿死不清槽——见 tunnel::DATA_CONN_SLOT 注），丢弃本拍退避
/// 重试，腿重连后新连接覆盖槽即自愈。
fn poll_loop(
    psk: [u8; 32],
    target_port: u16,
    host: String,
    slot: Arc<Mutex<Option<quinn::Connection>>>,
    dump_dir: PathBuf,
) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        crate::report::report("gatepoll", "tokio runtime 起不来，轮询线程退出");
        return;
    };
    rt.block_on(async move {
        let mut down_reported = false;
        let mut tick: u32 = 0;
        let mut backoff = BACKOFF_START_SECS;
        loop {
            tick = tick.wrapping_add(1);
            // tick 心跳（白露 NA0163 19 楼批）：每 30 拍一行——「成功静默」设计
            // 让僵死与空转不可分，这行是解药（判据挂 field-reports 断档）
            if tick.is_multiple_of(30) {
                crate::report::report("gatepoll", &format!("tick {tick}"));
            }
            let conn = slot.lock().unwrap().clone();
            let Some(c) = conn else {
                // 槽空＝隧道腿还没握上手（首起/腿死重拉中）——不弃不炸，
                // 退避短睡等腿握手覆盖槽
                if !down_reported {
                    crate::report::report("gatepoll", "腿连接槽空（等腿握手），退避重试");
                    down_reported = true;
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(BACKOFF_MAX_SECS);
                continue;
            };
            match fetch_pending(&c, &psk, target_port, &host).await {
                Ok(body) => {
                    if down_reported {
                        crate::report::report("gatepoll", "腿连接恢复，轮询续跑");
                        down_reported = false;
                    }
                    backoff = BACKOFF_START_SECS;
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
                    // 连接断（open_bi/读写错/超时）＝腿死槽里留尸——退避
                    // 重试，腿重连覆盖槽后自愈
                    if !down_reported {
                        crate::report::report(
                            "gatepoll",
                            &format!("腿连接不可达（{e}），退避等腿重连"),
                        );
                        down_reported = true;
                    }
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(BACKOFF_MAX_SECS);
                }
            }
        }
    });
}

/// 上传线程：与轮询线程同构（线程内单件 runtime + async 循环）；同一
/// 槽取连接（无常驻第二连接——共享腿连接设计），重试沿用 3 次×500ms
/// 后丢弃
fn upload_loop(
    psk: [u8; 32],
    target_port: u16,
    host: String,
    slot: Arc<Mutex<Option<quinn::Connection>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<(String, Vec<u8>)>,
) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        crate::report::report("gatepoll", "tokio runtime 起不来，上传线程退出");
        return;
    };
    rt.block_on(async move {
        while let Some((name, bytes)) = rx.recv().await {
            let mut ok = false;
            for _ in 0..UPLOAD_RETRIES {
                let conn = slot.lock().unwrap().clone();
                let Some(c) = conn else {
                    break; // 槽空：腿不在，重试也白搭——直接丢弃报表
                };
                let path = format!("/api/gate/result/{name}");
                match quic_http_roundtrip(&c, &psk, target_port, "POST", &path, &bytes, &host).await
                {
                    Ok(resp) if resp.starts_with(b"{\"ok\":true}") => {
                        ok = true;
                        break;
                    }
                    _ => {}
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if !ok {
                crate::report::report(
                    "gatepoll",
                    &format!("结果 {name} 上传丢弃（{} 次重试毕）", UPLOAD_RETRIES),
                );
            }
        }
    });
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
