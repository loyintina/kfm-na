//! main.rs — na-server 入口：监听 / 分流 / idle 自退
//!
//! 配置全走环境变量（主体拉起制：拉起命令由 na 给出，没有配置文件）：
//! - NA_BIND            监听地址（缺省 127.0.0.1:9021，只准回环）
//! - NA_REPORT_LOG      na-report 落盘路径（缺省 = cwd 下 field-reports.log）
//! - NA_IDLE_EXIT_SECS  无连接无会话持续 N 秒自退（缺省 1800，0 = 永不）
//! - NA_QUIC_BIND       QUIC 腿监听（可选，不设=不开；设计 docs/active/quic隧道.md）
//! - NA_QUIC_CERT       QUIC 证书路径前缀（缺省 $HOME/.kfm/certs/quic；
//!   三件 {前缀}.der/.key.der/.psk 作为一个身份：齐则载，缺则拒启——
//!   首跑生成须显式授权 NA_QUIC_GEN_KEYS=1（BAR-200 fail-loud：静默重生
//!   = 所有 pin 旧指纹的客户端永久失配）；开 QUIC 腿即强制 HMAC 挑战，设计 §四）
//! - NA_QUIC_GEN_KEYS   置 1 = 授权首跑生成 QUIC 身份三件（缺省不许）
//! - NA_QUIC_REV_BIND   M4 反连 QUIC 监听（可选，UDP 62694；9022 从 sshd
//!   绑口变本机 TCP 监听器+反向开流，撞口/僵尸/释放三件套消失）
//! - NA_QUIC_REV_TCP    反连本机桥前（缺省 127.0.0.1:9022，只准回环）
//! - NA_QUIC_REV_TARGET 手机侧回联口（缺省 8024 = na sshd）
//! - NA_FS_ROOTS        文件树数据面允许根（冒号分隔；缺省 = `/root`
//!   （服务端全库根，2026-09-27 用户裁决——不依赖 HOME）；设成空串 = 零根全 404）。
//!   语义在 na-protocol::fsapi::roots（每请求现读，运行时可改）
//!
//! 分流：peek 请求头不消费——见 Upgrade: websocket 交 wsterm（tokio-tungstenite
//! 从头自读），否则按平面 HTTP 处理（httpd）。

use na_protocol::fsapi;
use na_server::httpd;
use na_server::state::Registry;
use na_server::wsterm;

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn bind_addr() -> String {
    std::env::var("NA_BIND").unwrap_or_else(|_| "127.0.0.1:9021".into())
}

fn idle_exit_secs() -> u64 {
    std::env::var("NA_IDLE_EXIT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
}

/// 只绑回环的硬闸（安全语义 = 8021 同款：入口只有 SSH 隧道）
fn assert_loopback(addr: &str) {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    assert!(
        host == "127.0.0.1" || host == "localhost" || host == "::1",
        "NA_BIND 只准回环地址，收到: {addr}"
    );
}

/// QUIC 身份三件的前缀（env NA_QUIC_CERT > `$HOME/.kfm/certs/quic`）。
/// 2026-09-30 边界审计：原缺省写死作者机器路径——机器侧把证书放到该位置
/// （本机 `~/.kfm/certs` 软链到资产目录）或显式设 env
fn quic_cert_prefix() -> String {
    std::env::var("NA_QUIC_CERT").unwrap_or_else(|_| format!("{}/.kfm/certs/quic", home()))
}

/// $HOME（缺省 `/root`——systemd 服务不设 HOME 时的 uid 0 习惯位；
/// 空串按未设算：空 HOME 会拼出 `/.kfm/...` 这种坏路径）
fn home() -> String {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "/root".into())
}

/// QUIC 腿（可选，设计 docs/active/quic隧道.md §二桥接模型）：QUIC 入流
/// 读流头（2 字节端口 + 32 字节认证标签）→ 回联本机 TCP（9021 自己）——
/// 协议层零改动，QUIC 只是载体。绑公网必须显式写 0.0.0.0（默认回环不变）；
/// 开腿即强制客户端证（HMAC 挑战，设计 §四——公网口的前置条件）。
fn spawn_quic_leg() {
    let Ok(bind) = std::env::var("NA_QUIC_BIND") else {
        return;
    };
    {
        let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or(&bind);
        assert!(
            host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "0.0.0.0",
            "NA_QUIC_BIND 只准回环或显式 0.0.0.0，收到: {bind}"
        );
    }
    let prefix = quic_cert_prefix();
    let (certs, key, psk) = quic_identity(&prefix);
    eprintln!(
        "[na-server] QUIC 听 {bind}（证书指纹 {} / 客户端证已开）",
        hex(&na_quic::cert_fingerprint(&certs[0]))
    );
    let addr: std::net::SocketAddr = bind.parse().expect("NA_QUIC_BIND 解析");
    tokio::spawn(async move {
        if let Err(e) =
            na_quic::run_server(addr, na_quic::server_config(certs, key), Some(psk)).await
        {
            eprintln!("[na-server] QUIC 腿退出: {e}");
        }
    });
}

/// M4 反连腿（设计 §二、§八 M4：9022 从「sshd 绑口」变「本机 TCP 监听器
/// + 沿 QUIC 反向开流」）。三个环境变量，缺省不开——
/// - NA_QUIC_REV_BIND：QUIC 反连监听（UDP 62694，公网口前置 = 客户端证，
///   与正连腿同一份证书/psk——同一服务器身份、同一把钥匙）
/// - NA_QUIC_REV_TCP：本机 TCP 桥前（缺省 127.0.0.1:9022——今天 sshd
///   的位置；只在认领注册连接存活期内绑定，连接死即让位给 ssh 伴生兜底）
/// - NA_QUIC_REV_TARGET：手机侧回联口（缺省 8024 = na sshd）
fn spawn_rev_quic_leg() {
    let Ok(bind) = std::env::var("NA_QUIC_REV_BIND") else {
        return;
    };
    {
        let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or(&bind);
        assert!(
            host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "0.0.0.0",
            "NA_QUIC_REV_BIND 只准回环或显式 0.0.0.0，收到: {bind}"
        );
    }
    let tcp_bind: std::net::SocketAddr = std::env::var("NA_QUIC_REV_TCP")
        .unwrap_or_else(|_| "127.0.0.1:9022".into())
        .parse()
        .expect("NA_QUIC_REV_TCP 解析");
    {
        let host = tcp_bind.ip().to_string();
        assert!(
            host == "127.0.0.1" || host == "::1",
            "NA_QUIC_REV_TCP 只准回环（9022 语义不变），收到: {tcp_bind}"
        );
    }
    let target: u16 = std::env::var("NA_QUIC_REV_TARGET")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8024);
    let prefix = quic_cert_prefix();
    let (certs, key, psk) = quic_identity(&prefix);
    eprintln!(
        "[na-server] QUIC 反连听 {bind}（TCP 桥前 {tcp_bind} → 手机 {target}，证书指纹 {}）",
        hex(&na_quic::cert_fingerprint(&certs[0]))
    );
    let addr: std::net::SocketAddr = bind.parse().expect("NA_QUIC_REV_BIND 解析");
    tokio::spawn(async move {
        if let Err(e) = na_quic::run_rev_server(
            addr,
            na_quic::server_config_rev(certs, key),
            Some(psk),
            tcp_bind,
            target,
        )
        .await
        {
            eprintln!("[na-server] QUIC 反连腿退出: {e}");
        }
    });
}

/// QUIC 身份加载（BAR-200 fail-loud）：证书/私钥/psk 三件作为一个身份裁决——
/// 三件齐 → 载；全缺且显式授权（NA_QUIC_GEN_KEYS=1）→ 首跑生成落盘；
/// 全缺未授权 / 半缺 → panic 拒启大声喊话（静默换新 = 所有 pin 旧指纹的
/// 客户端永久失配，2026-09-30 事故）。裁决纯函数在 na_server::cert。
fn quic_identity(
    prefix: &str,
) -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
    [u8; 32],
) {
    use na_server::cert::{IdentityVerdict, identity_verdict};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let cert_path = format!("{prefix}.der");
    let key_path = format!("{prefix}.key.der");
    let psk_path = format!("{prefix}.psk");
    let cert_b = std::fs::read(&cert_path).ok();
    let key_b = std::fs::read(&key_path).ok();
    // psk 长度非法按「不在」计（旧码遇坏长度也会重生——坏件 = 事故形态）
    let psk_b = std::fs::read(&psk_path).ok().filter(|b| b.len() == 32);
    let gen_allowed = std::env::var("NA_QUIC_GEN_KEYS").as_deref() == Ok("1");
    match identity_verdict(
        cert_b.is_some(),
        key_b.is_some(),
        psk_b.is_some(),
        gen_allowed,
    ) {
        IdentityVerdict::Load => (
            vec![CertificateDer::from(cert_b.expect("判 Load 必在"))],
            PrivateKeyDer::Pkcs8(key_b.expect("判 Load 必在").into()),
            psk_b.expect("判 Load 必在").try_into().expect("32 字节"),
        ),
        IdentityVerdict::Gen => {
            let (certs, key) = na_quic::gen_self_signed("kfm-na");
            let mut psk = [0u8; 32];
            use std::io::Read as _;
            std::fs::File::open("/dev/urandom")
                .expect("urandom")
                .read_exact(&mut psk)
                .expect("读随机源");
            if let Some(dir) = std::path::Path::new(&cert_path).parent() {
                std::fs::create_dir_all(dir).expect("证书目录");
            }
            std::fs::write(&cert_path, certs[0].as_ref()).expect("证书落盘");
            let der = match &key {
                PrivateKeyDer::Pkcs8(k) => k.secret_pkcs8_der().to_vec(),
                _ => panic!("gen_self_signed 必出 PKCS8"),
            };
            std::fs::write(&key_path, der).expect("私钥落盘");
            std::fs::write(&psk_path, psk).expect("密钥落盘");
            // 0600：私钥级权限（unix 限定；host 侧考题跑在 Linux 上）
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(&psk_path, std::fs::Permissions::from_mode(0o600));
            }
            eprintln!(
                "[na-server] QUIC 身份首跑生成（NA_QUIC_GEN_KEYS=1 授权）：指纹 {} / 客户端证 psk（hex，抄进手机 servers.json 的 quic.psk）: {}",
                hex(&na_quic::cert_fingerprint(&certs[0])),
                hex(&psk)
            );
            (certs, key, psk)
        }
        IdentityVerdict::FailMissing => panic!(
            "QUIC 身份三件全缺（{cert_path} 系），拒绝静默重生（BAR-200）——\
             静默换新 = 所有 pin 旧指纹的客户端永久失配（2026-09-30 事故）。\
             修复二选一：①从归档恢复原件（备份或归档目录）②首跑/重置显式授权：\
             NA_QUIC_GEN_KEYS=1 重启本服务"
        ),
        IdentityVerdict::FailPartial => panic!(
            "QUIC 身份半缺（{prefix} 系：证书{} 私钥{} psk{}），拒绝静默补齐（BAR-200）——\
             半缺 = 事故形态（搬家漏链/误删单件），静默补齐会用新对覆盖幸存件，pin 照废。\
             修复：从归档恢复齐三件；确要重头来 = 删净三件后 NA_QUIC_GEN_KEYS=1",
            if cert_b.is_some() { "在" } else { "缺" },
            if key_b.is_some() { "在" } else { "缺" },
            if psk_b.is_some() { "在" } else { "缺" },
        ),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let addr = bind_addr();
    assert_loopback(&addr);
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("绑 {addr} 失败: {e}"));
    eprintln!("[na-server] 听 {addr}（idle 自退 {}s）", idle_exit_secs());

    let registry = Arc::new(Registry::new());

    spawn_quic_leg();
    spawn_rev_quic_leg();

    // idle 自退：无连接无会话持续超时 → 退出（下次 na 连接重新拉起）
    {
        let reg = Arc::clone(&registry);
        tokio::spawn(async move {
            let limit = idle_exit_secs();
            if limit == 0 {
                return; // 0 = 永不自退（考题/调试用）
            }
            let mut clock = tokio::time::interval(Duration::from_secs(60));
            loop {
                clock.tick().await;
                if reg.uptime_s() >= limit && reg.conn_count() == 0 && reg.session_count() == 0 {
                    eprintln!("[na-server] idle {limit}s 无连接无会话，自退");
                    std::process::exit(0);
                }
            }
        });
    }

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                // BAR-171：fd 枯竭期 accept 立即失败——无退避的 continue
                // 是空转刷屏（99108 行/29GB 实录，2026-09-28 晨）。200ms
                // 定间隔退避 + 同款限流；accept 恢复即正常服务。
                na_server::logcap::throttled("accept", &format!("[na-server] accept 失败: {e}"));
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let reg = Arc::clone(&registry);
        tokio::spawn(async move {
            if let Err(e) = dispatch(stream, reg).await {
                // key 用错误种（稳定串），同款风暴归并；peer 端口进 line
                na_server::logcap::throttled(&e, &format!("[na-server] 连接 {peer} 处理失败: {e}"));
            }
        });
    }
}

/// 连接分流：peek 头区 → WS 升级 or 平面 HTTP
async fn dispatch(stream: TcpStream, registry: Arc<Registry>) -> Result<(), String> {
    let head = peek_head(&stream).await?;
    if head
        .lines()
        .any(|l| l.eq_ignore_ascii_case("upgrade: websocket"))
    {
        wsterm::handle(stream, registry).await;
        return Ok(());
    }
    http_handle(stream, &head, registry).await
}

/// peek 到 \r\n\r\n 为止（不消费；上限 16KB / 10s）
async fn peek_head(stream: &TcpStream) -> Result<String, String> {
    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let n = tokio::time::timeout_at(deadline, stream.peek(&mut buf))
            .await
            .map_err(|_| "peek 头区超时".to_string())?
            .map_err(|e| format!("peek 失败: {e}"))?;
        if n == 0 {
            return Err("对端在头区前断开".into());
        }
        if let Some(pos) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            return String::from_utf8(buf[..pos].to_vec()).map_err(|_| "头区非 UTF-8".to_string());
        }
        if n >= 16384 {
            return Err("头区超 16KB".into());
        }
        if n == buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
        // 头未到齐，让出等更多字节
        tokio::task::yield_now().await;
    }
}

/// /agent 反代上游（na-agentd 本地口；9041 端口对账 2026-09-26 见
/// na-agentd main.rs 注）。env 可改（考题/多实例）
fn agent_upstream_addr() -> String {
    std::env::var("NA_AGENT_UPSTREAM").unwrap_or_else(|_| "127.0.0.1:9041".into())
}

/// /agent 反代（BAR-163 工单⑥ A）：请求原样转发（剥 /agent 前缀的上游
/// 路径 + 原 body），上游 Connection: close 响应**整段字节**回传中继——
/// 状态行/头/体零重组，agentd 的契约就是线上的契约。连接失败/转发
/// 失败 = 502 诚实报错（httpd::agent_error_body）。读超时 3700s——
/// /send 是同步跑到 stop 的慢口，反代不许比 CLI 先放弃
async fn agent_proxy(method: &str, upstream: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let addr = agent_upstream_addr();
    let mut up = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr.as_str()))
        .await
        .map_err(|_| format!("连 {addr} 超时"))
        .and_then(|r| r.map_err(|e| format!("连 {addr} 失败: {e}")))?;
    let req = format!(
        "{method} {upstream} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    up.write_all(req.as_bytes())
        .await
        .map_err(|e| format!("写上游失败: {e}"))?;
    up.write_all(body)
        .await
        .map_err(|e| format!("写上游失败: {e}"))?;
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(3700), up.read_to_end(&mut raw))
        .await
        .map_err(|_| "读上游超时（3700s 封顶）".to_string())?
        .map_err(|e| format!("读上游失败: {e}"))?;
    if raw.is_empty() {
        return Err("上游空响应".into());
    }
    Ok(raw)
}

/// fs 面执行闸：na-server 是 `current_thread` 运行时——全部连接（含 WS 终端
/// 流）共用那一条线程，handler 里直接同步 `std::fs` 会把所有连接一起冻住。
/// 落 `spawn_blocking` 的阻塞池（运行时外挂线程，current_thread 同样有）、
/// await 回收结果。fsapi 全是同步纯函数，搬进闭包零成本。
async fn fs_blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("fs 任务未完成: {e}"))
}

/// 平面 HTTP：读体 → 路由 → 响应
async fn http_handle(
    mut stream: TcpStream,
    head: &str,
    registry: Arc<Registry>,
) -> Result<(), String> {
    let (method, path, content_length) = httpd::parse_head(head)?;
    let head_len = head.len() + 4; // + \r\n\r\n
    let mut raw = vec![0u8; head_len + content_length];
    stream
        .read_exact(&mut raw)
        .await
        .map_err(|e| format!("读请求失败: {e}"))?;
    let body = String::from_utf8_lossy(&raw[head_len..]).into_owned();

    let resp = match httpd::route(&method, &path) {
        httpd::Route::Agent { upstream } => {
            match agent_proxy(&method, &upstream, body.as_bytes()).await {
                Ok(raw) => raw,
                Err(e) => httpd::respond(502, "Bad Gateway", &httpd::agent_error_body(&e)),
            }
        }
        httpd::Route::Report => match httpd::append_report(&body) {
            Ok(()) => httpd::respond(200, "OK", "{\"ok\":true}"),
            Err(e) => httpd::respond(500, "Internal Server Error", &httpd::error_body(&e)),
        },
        httpd::Route::Health => httpd::respond(200, "OK", &registry.health_json()),
        httpd::Route::Sys => {
            // collect 永不失败——单路采不到归该路 null 显形
            httpd::respond(200, "OK", &httpd::sys_json(&na_sys::collect("/")))
        }
        httpd::Route::FsList { dir } => match fs_blocking(move || fsapi::list_json(&dir)).await {
            Ok(Ok(body)) => httpd::respond(200, "OK", &body),
            Ok(Err(e)) => httpd::fs_error_response(&e),
            Err(e) => httpd::respond(500, "Internal Server Error", &httpd::error_body(&e)),
        },
        httpd::Route::FsWalk { ext, after, limit } => {
            match fs_blocking(move || fsapi::walk_json(&ext, after.as_deref(), limit)).await {
                Ok(Ok(body)) => httpd::respond(200, "OK", &body),
                Ok(Err(e)) => httpd::fs_error_response(&e),
                Err(e) => httpd::respond(500, "Internal Server Error", &httpd::error_body(&e)),
            }
        }
        httpd::Route::FsRead {
            path,
            max,
            offset,
            has_offset,
        } => {
            // 显式带 offset 键 = BAR-170 分块读（首块 offset=0 也是新契约）；
            // 不带 = 旧契约（read_json 原样，kfmv4/查看器不受影响）
            match fs_blocking(move || {
                if has_offset {
                    fsapi::read_range_json(&path, offset, max)
                } else {
                    fsapi::read_json(&path, max)
                }
            })
            .await
            {
                Ok(Ok(body)) => httpd::respond(200, "OK", &body),
                Ok(Err(e)) => httpd::fs_error_response(&e),
                Err(e) => httpd::respond(500, "Internal Server Error", &httpd::error_body(&e)),
            }
        }
        httpd::Route::NotFound => {
            httpd::respond(404, "Not Found", "{\"ok\":false,\"error\":\"not found\"}")
        }
    };
    stream
        .write_all(&resp)
        .await
        .map_err(|e| format!("写响应失败: {e}"))?;
    Ok(())
}
