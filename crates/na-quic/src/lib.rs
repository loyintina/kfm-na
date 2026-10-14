//! na-quic — QUIC 隧道核心（设计：docs/active/quic隧道.md）
//!
//! 桥接模型：本机 TCP 监听器 → QUIC bidirectional stream → 对端回联 TCP。
//! 协议层零改动：App 照旧打 127.0.0.1:9021，QUIC 只是载体（角色与今天
//! 的 ssh -L/-R 完全相同），出问题关桥退 ssh，应用层无感。
//!
//! 分层：核心层合规——quinn/rustls/ring 无平台依赖（M1 双端编译实证）。
//!
//! 帧格式：每条 bidirectional stream 前 2 字节 = 目标端口（网络序）。
//! 双向同构——客户端开流带 9021（正连），服务器开流带 8024（反连），
//! 对端按端口回联本机 127.0.0.1:{port}。撞口在类型层面不存在：
//! 多并发 = 多条流，没有端口绑定动作。
//!
//! 协议 v2（BAR-247 改道，2026-10-14 用户终审「SG 零增开新口」——一口多
//! 设备）：v1 的认证锚焊死端口号、认领槽全局唯一，双机同 psk 同口必互踢
//! （11 机 field-reports 定罪）。v2 = 流头魔数 [`V2_MAGIC`] 分流 + 设备 id
//! 入流头与认证锚（[`auth_tag2`]）+ 认领槽按 device_id 分槽（同 id 顶替
//! 防僵尸保留，异 id 各占各槽）。无旗日：v1 客户端零改动照连，归 legacy
//! 槽（空 id）；quinn 未配 ALPN，魔数走流内自描述，与「流头自带口+标签」
//! 同构。

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// idle 上限：4h——省电冻结期连接状态保活（代价 ≈ 一条 CID 表项，设计 §五）
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(4 * 3600);

/// 反连腿死寂判死（M4-5 兜底演练实踩）：反连连接**协议层必须自带
/// 死亡检测**——数据腿僵尸有 e2e 探活收尸（本地口还在可打），反连
/// 腿无本地可观测物，服务器重启/网络静死后全靠 idle 上限。沿用 4h
/// = ssh 兜底永远接不上（2026-09-24 实录：systemd 重启 na-server，
/// 反连腿僵尸举「在线」相，9022 无人绑）。取 60s = keepalive 10s
/// 的 6 倍：健康连接对端 ACK 续命稳活，死寂 1 分钟内定罪；doze 冻结
/// 期计时器同冻，醒来一次性重拉（成本与数据腿 BAR-141 同级）
pub const REV_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 客户端保活：10s 一 ping 保 NAT 映射（设计 §五）
pub const KEEPALIVE: Duration = Duration::from_secs(10);

/// 握手超时（BAR-146）：UDP 黑洞里 connect 会挂到 idle 上限（4h）——
/// 腿线程不死、死信不发、本地口不开，看门狗在 Starting 里空转成天坑。
/// 握手必须速败，把定罪权交回看门狗（跳闸账满即降级 ssh 兜底）
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);

// ---- A 档纯逻辑：流头端口编解码 ----

/// 流头（A 档）：每条流前 2 字节 = 目标端口（网络序大端）
pub fn port_header(port: u16) -> [u8; 2] {
    port.to_be_bytes()
}

pub fn parse_port_header(h: &[u8; 2]) -> u16 {
    u16::from_be_bytes(*h)
}

// ---- 协议 v2（BAR-247 改道「一口多设备」）：魔数分流 + 设备 id ----

/// v2 流头魔数：首 2 字节 == 此值 = v2 头（随后 1 字节 id 长 + id + 2 字节
/// 端口），否则按 v1 的 2 字节端口解。占 0xFF00 保留段——现网业务口
/// （0 注册 / 8024 sshd / 90xx 闸门）全在其下，零撞（钉在 oneport_spec）。
pub const V2_MAGIC: u16 = 0xFF02;

/// 设备 id 字节上限（流头 1 字节长度前缀的硬顶之外再收一道：id 是配置面
/// 人工命名，32 字节绰绰有余，收紧 = 坏配置早炸）
pub const DEVICE_ID_MAX_LEN: usize = 32;

/// 设备 id 合法判（A 档）：1..=32 字节，ASCII 字母数字与 `-_`。空串非法
/// ——空 = v1/legacy 形态，不许混进 v2 通道（钉在 oneport_spec）
pub fn valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= DEVICE_ID_MAX_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// v2 流头编码（A 档）：魔数 + 1 字节 id 长 + id + 2 字节端口（网络序）。
/// 非法 id 拒编码（None）——调用方必须先用 [`valid_device_id`] 过闸
pub fn stream_header_v2(device_id: &str, port: u16) -> Option<Vec<u8>> {
    if !valid_device_id(device_id) {
        return None;
    }
    let mut v = Vec::with_capacity(2 + 1 + device_id.len() + 2);
    v.extend_from_slice(&V2_MAGIC.to_be_bytes());
    v.push(device_id.len() as u8);
    v.extend_from_slice(device_id.as_bytes());
    v.extend_from_slice(&port.to_be_bytes());
    Some(v)
}

/// v2 流头解码（A 档，入参 = 魔数之后的字节）：1 字节 id 长 + id + 2 字节
/// 端口。坏体（截断/零长/超长/非法字符）一律 None——解析层不收尸，坏流
/// 由调用方弃掉
pub fn parse_stream_header_v2(body: &[u8]) -> Option<(&str, u16)> {
    let (&len, rest) = body.split_first()?;
    let len = len as usize;
    if len == 0 || len > DEVICE_ID_MAX_LEN {
        return None;
    }
    let id_b = rest.get(..len)?;
    let id = std::str::from_utf8(id_b)
        .ok()
        .filter(|s| valid_device_id(s))?;
    let port_b = rest.get(len..len + 2)?;
    Some((id, u16::from_be_bytes([port_b[0], port_b[1]])))
}

/// 从流上读 v2 头字段（B 档胶水，魔数已被调用方消费）：读不齐/非法 = 弃流
async fn read_v2_fields(recv: &mut quinn::RecvStream) -> Option<(String, u16)> {
    let mut lb = [0u8; 1];
    recv.read_exact(&mut lb).await.ok()?;
    let len = lb[0] as usize;
    if len == 0 || len > DEVICE_ID_MAX_LEN {
        return None;
    }
    let mut idb = vec![0u8; len];
    recv.read_exact(&mut idb).await.ok()?;
    let id = String::from_utf8(idb).ok().filter(|s| valid_device_id(s))?;
    let mut pb = [0u8; 2];
    recv.read_exact(&mut pb).await.ok()?;
    Some((id, u16::from_be_bytes(pb)))
}

/// 认领槽显示名（日志用）：空 id = v1 legacy 槽
fn slot_name(id: &str) -> &str {
    if id.is_empty() { "(v1-legacy)" } else { id }
}

// ---- 证书 ----

/// 自签证书生成（双端同用：服务器生成一次落盘，客户端钉指纹——设计 §四）
pub fn gen_self_signed(
    name: &str,
) -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let cert = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
    (
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into()),
    )
}

/// 证书指纹（pinning 的比对物）：DER 的 SHA-256
pub fn cert_fingerprint(der: &rustls::pki_types::CertificateDer<'_>) -> [u8; 32] {
    use sha2::Digest as _;
    let mut h = sha2::Sha256::new();
    h.update(der.as_ref());
    h.finalize().into()
}

// ---- 客户端证（设计 §四：预共享密钥 HMAC 挑战——服务器认密钥不认 IP） ----

/// HMAC-SHA256（RFC 2104 手卷——不引 hmac 依赖，vendor 免重生；
/// 判卷 = RFC 4231 标准向量，tests/auth_spec.rs 钉死）
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha2::Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = sha2::Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let ih = inner.finalize();
    let mut outer = sha2::Sha256::new();
    outer.update(opad);
    outer.update(ih);
    outer.finalize().into()
}

/// 认证标签长度（流头 = 2 字节端口 + 32 字节标签）
pub const AUTH_TAG_LEN: usize = 32;

/// 认证标签：HMAC(psk, "na-quic-auth-v1" || 端口头)。标签在 TLS 内部
/// 传输——攻击者看不见（加密）也伪造不了（无钥），静态标签已足够；
/// 绑端口头 = 标签不可跨端口挪用
pub fn auth_tag(psk: &[u8; 32], port: u16) -> [u8; 32] {
    let mut m = [0u8; 17];
    m[..15].copy_from_slice(b"na-quic-auth-v1");
    m[15..].copy_from_slice(&port.to_be_bytes());
    hmac_sha256(psk, &m)
}

/// 认证标签 v2（BAR-247 改道）：HMAC(psk, "na-quic-auth-v2" ‖ id 长 ‖
/// 设备 id ‖ 端口头)。换锚三件套——异版本签名不可互挪用（"v1"≠"v2" 锚）、
/// 跨机不可挪用（绑设备 id）、跨口不可挪用（绑端口照旧）。id 长度前缀入
/// 锚 = 流头线格式与认证消息同源同构，无拼接歧义
pub fn auth_tag2(psk: &[u8; 32], device_id: &str, port: u16) -> [u8; 32] {
    let mut m = Vec::with_capacity(15 + 1 + device_id.len() + 2);
    m.extend_from_slice(b"na-quic-auth-v2");
    m.push(device_id.len() as u8);
    m.extend_from_slice(device_id.as_bytes());
    m.extend_from_slice(&port.to_be_bytes());
    hmac_sha256(psk, &m)
}

/// 常量时间比对（认证面不许早退——时序侧信道零成本就堵上）
pub fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 认证连败封禁裁决（A 档纯函数）：同一来源连败满 trip 次，ban_secs 内
/// 不再给它花一个字节的处理——在线枚举 32 字节密钥本是天文数字，
/// 这道闸防的是资源消耗，不是枚举本身
pub const AUTH_FAIL_TRIP: u32 = 5;
pub const AUTH_BAN_SECS: u64 = 600;

pub fn ban_verdict(fails: u32, elapsed_secs: u64) -> bool {
    fails >= AUTH_FAIL_TRIP && elapsed_secs < AUTH_BAN_SECS
}

/// 服务器 QUIC 配置
pub fn server_config(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> ServerConfig {
    server_config_with_idle(certs, key, IDLE_TIMEOUT)
}

/// 服务器 QUIC 配置（反连腿版，M4-5）：死寂判死 60s——手机静死/掉线
/// 时认领连接按期收尸，9022 及时让位 ssh 伴生兜底（4h = 兜底撞口
/// 活锁：na-server 僵尸占着 9022，ssh -R 绑不上 255 空转）
pub fn server_config_rev(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> ServerConfig {
    server_config_with_idle(certs, key, REV_IDLE_TIMEOUT)
}

fn server_config_with_idle(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
    idle: Duration,
) -> ServerConfig {
    let mut sc = ServerConfig::with_single_cert(certs, key).expect("证书装载");
    Arc::get_mut(&mut sc.transport)
        .expect("transport 独占")
        .max_concurrent_bidi_streams(128u32.into())
        .max_idle_timeout(Some(idle.try_into().expect("idle 上限")));
    sc
}

/// 指纹 pinning 验证器（设计 §四：不走 CA——我们没有域名，CA 是负资产）
#[derive(Debug)]
pub struct PinnedVerifier {
    pub pinned: [u8; 32],
}

impl rustls::client::danger::ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if cert_fingerprint(end_entity) == self.pinned {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "na-quic: 服务器证书指纹与 pinning 不符".into(),
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &rustls::pki_types::CertificateDer<'_>,
        _d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _m: &[u8],
        _c: &rustls::pki_types::CertificateDer<'_>,
        _d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// 客户端 QUIC 配置（指纹 pinning）
pub fn client_config(pinned: [u8; 32]) -> ClientConfig {
    client_config_with_idle(pinned, IDLE_TIMEOUT)
}

/// 客户端 QUIC 配置（反连腿版，M4-5）：死寂判死 60s——反连腿无本地
/// 可观测物（数据腿僵尸有 e2e 探活收尸），服务器重启/网络静死全靠
/// idle 上限定罪；4h = ssh 兜底永远接不上（2026-09-24 兜底演练实踩：
/// systemd 重启 na-server，反连腿僵尸举「在线」相，9022 无人绑）
pub fn client_config_rev(pinned: [u8; 32]) -> ClientConfig {
    client_config_with_idle(pinned, REV_IDLE_TIMEOUT)
}

fn client_config_with_idle(pinned: [u8; 32], idle: Duration) -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier { pinned }))
        .with_no_client_auth();
    let mut cc = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).expect("tls 配置"),
    ));
    cc.transport_config(Arc::new({
        let mut t = quinn::TransportConfig::default();
        t.keep_alive_interval(Some(KEEPALIVE));
        t.max_idle_timeout(Some(idle.try_into().expect("idle 上限")));
        t
    }));
    cc
}

// ---- B 档桥接（TCP↔stream splice，IO 胶水） ----

/// 排水窗（BAR-171）：桥一侧终结后另一侧的排放上限。旧制 `join!` 等
/// 双侧齐终——一侧永久 pend（死手机的 QUIC 流在 idle 4h 内不报错，
/// 流控窗满后写端也 pend）桥任务就陪葬 4h：fd、对端 ws 会话、pty、
/// tmux 附着壳全套僵尸沿桥传导（2026-09-27/28 实录：30 孤儿 attach +
/// 252 fd + 29GB 日志）。改「先终 + 排水窗」：一侧终结另一侧限时
/// 排放（HTTP 半关的响应照常走完，bridge_spec 半关钉守住这条），
/// 窗尽强制收尾——函数返回即析构，tcp/流柄全 drop，两侧都不许再挂。
/// env NA_SPLICE_DRAIN_MS 可拧（考题拧小），生产缺省 15s。
pub const SPLICE_DRAIN_MS_DEFAULT: u64 = 15_000;

fn drain_grace() -> Duration {
    std::env::var("NA_SPLICE_DRAIN_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(SPLICE_DRAIN_MS_DEFAULT))
}

/// TCP ↔ QUIC 流双向拷贝（B 档胶水）：**任一侧先终即进排水窗**，窗尽
/// 或另侧自终即返。排水窗语义见 SPLICE_DRAIN_MS_DEFAULT 注。
async fn splice(
    mut tcp: tokio::net::TcpStream,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
) {
    let (mut tr, mut tw) = tcp.split();
    let up = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match tr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if send.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = send.finish();
    };
    let down = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match recv.read(&mut buf).await {
                Ok(Some(n)) if n > 0 => {
                    if tw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    };
    tokio::pin!(up);
    tokio::pin!(down);
    tokio::select! {
        _ = &mut up => {
            // 上行先终（对端 TCP EOF/错）：下行限时排放残余
            let _ = tokio::time::timeout(drain_grace(), &mut down).await;
        }
        _ = &mut down => {
            // 下行先终（QUIC 流 FIN/错）：上行限时排放残余
            let _ = tokio::time::timeout(drain_grace(), &mut up).await;
        }
    }
    // 收尾 = 析构：pin 住的 future 弃疗即释放借用，tcp/流柄随返回全
    // drop——桥任务的生命周期钉死在「先终 + 排水窗」之内
}

/// 服务器腿：QUIC 监听，每条入站流读流头 → 回联 127.0.0.1:{port} →
/// splice。流头 = 2 字节端口；psk 为 Some 时再读 32 字节认证标签，
/// 验不过直接弃流并按来源 IP 记连败（满 AUTH_FAIL_TRIP 封 10 分钟）。
/// 连接级循环：连接死了 accept 出错返回（看门狗外侧重建）
pub async fn run_server(
    bind: SocketAddr,
    cfg: ServerConfig,
    psk: Option<[u8; 32]>,
) -> std::io::Result<()> {
    use std::collections::HashMap;
    let ep = Endpoint::server(cfg, bind).map_err(|e| std::io::Error::other(e.to_string()))?;
    // 连败账（来源 IP → (次数, 首次失败时刻)）——认证面的资源闸
    let fails = std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::<
        std::net::IpAddr,
        (u32, std::time::Instant),
    >::new()));
    // 认领账（BAR-171 翻案·修复①；BAR-247 改道升多槽）：正连腿按
    // device_id 分槽养活跃连接——同槽新连接第一条验签通过的流落地时，
    // 旧连接当场 close；异槽各占各（一口多设备零互踢，钉在 oneport_spec）。
    // v1 客户端归 legacy 槽（空 id，顶替语义照旧 = 无旗日）。级联收尸链：
    // 旧连接死 → 其 splice 全终 → loopback TCP 全断 → wsterm killAll
    // → 预热壳整组收。**验签通过才认领**——握手无客户端证，扫描器
    // 光握手顶不掉真客户端（设计 §四）。QUIC 迁移不触雷：网络切换
    // 是同一 Connection 不换 Incoming，stable_id 不变不自杀。
    // （反连腿同款语义早已在 run_rev_server：「新注册挤掉旧的」）
    let owner = std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::<String, Connection>::new()));
    while let Some(inc) = ep.accept().await {
        let fails = std::sync::Arc::clone(&fails);
        let owner = std::sync::Arc::clone(&owner);
        tokio::spawn(async move {
            let Ok(conn) = inc.await else { return };
            let ip = conn.remote_address().ip();
            // 敲门账本（用户「担心被扫」的观测面）：谁来了/谁验签栽了/
            // 谁被封，全落 stderr（na-server → /var/log/kfm-na-server.log）
            eprintln!("[na-quic] 连接 {ip}");
            {
                let g = fails.lock().await;
                if let Some(&(n, t0)) = g.get(&ip)
                    && ban_verdict(n, t0.elapsed().as_secs())
                {
                    eprintln!("[na-quic] 封禁中拒连 {ip}（第 {n} 次连败）");
                    conn.close(1u32.into(), b"auth banned");
                    return;
                }
            }
            loop {
                let stream = conn.accept_bi().await;
                let Ok((send, mut recv)) = stream else { break };
                let fails = std::sync::Arc::clone(&fails);
                let owner = std::sync::Arc::clone(&owner);
                let conn = conn.clone();
                tokio::spawn(async move {
                    let mut hdr = [0u8; 2];
                    if recv.read_exact(&mut hdr).await.is_err() {
                        return;
                    }
                    // v1/v2 分流（BAR-247 改道）：魔数 = v2（续读 id+端口），
                    // 否则 hdr 即 v1 的 2 字节端口，归 legacy 槽（空 id）
                    let (slot, port) = if u16::from_be_bytes(hdr) == V2_MAGIC {
                        let Some((id, port)) = read_v2_fields(&mut recv).await else {
                            return; // 坏头弃流（对齐 v1「读不齐即弃」，不入连败账）
                        };
                        (id, port)
                    } else {
                        (String::new(), parse_port_header(&hdr))
                    };
                    if let Some(k) = psk {
                        let mut tag = [0u8; AUTH_TAG_LEN];
                        let expect = if slot.is_empty() {
                            auth_tag(&k, port)
                        } else {
                            auth_tag2(&k, &slot, port)
                        };
                        let ok = recv.read_exact(&mut tag).await.is_ok() && ct_eq(&tag, &expect);
                        if !ok {
                            let mut g = fails.lock().await;
                            let e = g.entry(ip).or_insert((0, std::time::Instant::now()));
                            if e.1.elapsed().as_secs() >= AUTH_BAN_SECS {
                                *e = (0, std::time::Instant::now());
                            }
                            e.0 += 1;
                            eprintln!("[na-quic] 验签失败 {ip}（第 {} 次连败）", e.0);
                            return;
                        }
                    }
                    // 验签通过 = 真客户端——认领本槽正连载具：顶掉同槽旧连接
                    // （幂等：本连接已是槽主则零动作；逐流对账免 claimed 旗）
                    {
                        let mut g = owner.lock().await;
                        let mine = conn.stable_id();
                        if g.get(&slot).map(|c| c.stable_id()) != Some(mine)
                            && let Some(old) = g.insert(slot.clone(), conn.clone())
                        {
                            eprintln!(
                                "[na-quic] 正连顶替：槽 {} 旧连接 {} 收尸（新连接 {mine} 认领，{ip}）",
                                slot_name(&slot),
                                old.stable_id()
                            );
                            old.close(0u32.into(), b"superseded");
                        }
                    }
                    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                    let Ok(tcp) = tokio::net::TcpStream::connect(target).await else {
                        return;
                    };
                    splice(tcp, send, recv).await;
                });
            }
        });
    }
    Ok(())
}

/// 跳闸回切探针（BAR-171 翻案③，tunnel 看门狗用）：只握手——不开
/// 本地口（口可能还在 ssh 手里）、不验签（验签是正腿的事）。成功 =
/// UDP 路通且服务器 QUIC 面活着。握手超时沿用 HANDSHAKE_TIMEOUT
/// （BAR-146：UDP 黑洞里 connect 挂死到 idle 上限，必须速败）。
pub async fn probe(server: SocketAddr, sni: &str, cfg: ClientConfig) -> bool {
    let Ok(mut ep) = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))) else {
        return false;
    };
    ep.set_default_client_config(cfg);
    let connecting = match ep.connect(server, sni) {
        Ok(c) => c,
        Err(_) => return false,
    };
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, connecting).await {
        Ok(Ok(conn)) => {
            conn.close(0u32.into(), b"probe");
            ep.wait_idle().await; // 成功路放干净（close 帧发出去）
            true
        }
        // 失败路直接丢端点：wait_idle 会等失败连接的排水（实测 +3s，
        // 死口探 11s 超线被钉咬）——探针没有体面收尾的义务
        _ => false,
    }
}

/// 客户端腿：连上服务器保持一条 QUIC 连接；本机 TCP 监听器每收一个
/// 连接 → 开一条流（写端口头 = target_port；psk 为 Some 时续写认证
/// 标签——设计 §四客户端证）→ splice。连接死 = 本函数返回（看门狗
/// 外侧重建——死亡检测是事件驱动的，这就是 QUIC 腿比 ssh 腿省掉探活
/// 三件套的原因）。target_port 与 local_bind 解耦：部署惯例双端同口
/// （9021→9021），但桥接模型本身不绑这个约定。
/// conn_out（BAR-233 根治返工·共享腿连接）：Some 时**握手成功后、本地
/// TCP 口 bind 之前**把 Connection clone 递出去——进程内共享消费方
/// （gate_poller）在连接对象上直接 open_bi 开流说话，不落本机 TCP 口；
/// 选 bind 前的时点＝尽早可用（bind 失败/腿死时槽里留着死连接，消费方
/// 试错退避等下一条覆盖，与「腿死不清槽」语义一致）。None = 旧行为零
/// 变化。
pub async fn run_client(
    server: SocketAddr,
    sni: &str,
    local_bind: SocketAddr,
    target_port: u16,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
    conn_out: Option<tokio::sync::mpsc::UnboundedSender<Connection>>,
) -> std::io::Result<()> {
    run_client_inner(
        server,
        sni,
        local_bind,
        target_port,
        cfg,
        psk,
        conn_out,
        None,
    )
    .await
}

/// 客户端腿 v2（BAR-247 改道「一口多设备」）：与 [`run_client`] 同构，
/// 唯流头走 v2（魔数+设备 id+端口）、认证走 [`auth_tag2`]——服务器按
/// device_id 分槽认领，同 psk 多设备同 UDP 口共存零互踢。非法 id
/// fail-loud 拒起（配置面坏件不许带伤跑）
// v1 兼容皮同形签名 + 一个 device_id——参数与腿配置一一对应，拆包
// 反丢可读性（run_client_inner 同款 allow）
#[allow(clippy::too_many_arguments)]
pub async fn run_client_v2(
    server: SocketAddr,
    sni: &str,
    local_bind: SocketAddr,
    target_port: u16,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
    conn_out: Option<tokio::sync::mpsc::UnboundedSender<Connection>>,
    device_id: &str,
) -> std::io::Result<()> {
    if !valid_device_id(device_id) {
        return Err(std::io::Error::other(format!(
            "非法设备 id {device_id:?}（1..=32 字节 ASCII 字母数字与 -_）"
        )));
    }
    run_client_inner(
        server,
        sni,
        local_bind,
        target_port,
        cfg,
        psk,
        conn_out,
        Some(device_id),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_client_inner(
    server: SocketAddr,
    sni: &str,
    local_bind: SocketAddr,
    target_port: u16,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
    conn_out: Option<tokio::sync::mpsc::UnboundedSender<Connection>>,
    device_id: Option<&str>,
) -> std::io::Result<()> {
    let dev: Option<String> = device_id.map(str::to_string);
    let mut ep = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    ep.set_default_client_config(cfg);
    let conn: Connection = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        ep.connect(server, sni)
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    )
    .await
    .map_err(|_| std::io::Error::other("QUIC 握手超时（UDP 黑洞？）"))?
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Some(tx) = conn_out {
        // unbounded：消费方不在也不卡腿；信道随 run_client 返回而关
        let _ = tx.send(conn.clone());
    }
    // BAR-233 追件（白露 NA0163 7 楼裁②）：本机口 bind 失败要带着地址响亮
    // 报错——双实例/分身竞争期谁持口无人知晓（判卷 38 次 refused 的观测
    // 盲区）；裸 `?` 把 AddrInUse 折叠成无差别腿死，定性线索丢失。
    let listener = tokio::net::TcpListener::bind(local_bind)
        .await
        .map_err(|e| {
            std::io::Error::other(format!(
                "本机数据口 bind 失败 {local_bind}: {e}（口被谁占？双实例/残留监听候选——BAR-233）"
            ))
        })?;
    loop {
        let (tcp, _) = listener.accept().await?;
        // BAR-247：死因变体透出——superseded 顶号 / TimedOut / reset 分型
        // 是断连风暴定罪的直接证据（反连腿同款早已带，「closed by peer:
        // replaced」即由此见）；旧制吞变体只剩「连接已死」，万次死亡无定性
        let (mut send, recv) = match conn.open_bi().await {
            Ok(s) => s,
            Err(e) => return Err(std::io::Error::other(format!("QUIC 连接已死: {e}"))),
        };
        let dev = dev.clone();
        tokio::spawn(async move {
            // 流头 + 认证标签：v2（魔数+id+端口+tag2）或 v1（端口+tag）
            let mut head = match &dev {
                Some(id) => {
                    let mut h = stream_header_v2(id, target_port).expect("id 已过闸");
                    if let Some(k) = psk {
                        h.extend_from_slice(&auth_tag2(&k, id, target_port));
                    }
                    h
                }
                None => {
                    let mut h = port_header(target_port).to_vec();
                    if let Some(k) = psk {
                        h.extend_from_slice(&auth_tag(&k, target_port));
                    }
                    h
                }
            };
            head.shrink_to_fit();
            if send.write_all(&head).await.is_err() {
                return;
            }
            splice(tcp, send, recv).await;
        });
    }
}

// ---- M4 反连路（设计 §二：9022 从「sshd 绑口」变「na-server 本机 TCP
// 监听器 + 服务器沿 QUIC 反向开流」——撞口/僵尸/释放三件套整族消失）----
//
// 角色互换：手机是 QUIC 客户端（拨出），服务器是流的**发起方**。
// 注册闸：反连客户端不主动开业务流，服务器无从验客户端证——故连接
// 建立后手机必须先开一条「注册流」（端口头 = REG_PORT + psk 标签），
// 验过才认领这条连接当反连载具；验不过记连败（与正连同一份资源闸）。

/// 注册流端口头（A 档常量）：0 不是合法业务口，天然不会与正连流混
pub const REG_PORT: u16 = 0;

/// 反连桥前配置（BAR-247 改道「一口多设备」）：一个本机 TCP 口 ↔ 一个
/// 设备 id 认领槽。device_id 空 = legacy 槽（v1 旧核注册）；非空 = v2
/// 设备专属桥前（如 9025:neo9，na-server 经 NA_QUIC_REV_FRONTS 配置）。
/// target_port = 设备侧回联口（8024 = na sshd）
pub struct RevFront {
    pub tcp_bind: SocketAddr,
    pub device_id: String,
    pub target_port: u16,
}

/// 反连服务器腿（v1 单 front 兼容皮）：等价于 [`run_rev_server_multi`] 只
/// 挂一个 legacy front（空 id，tcp_bind/target_port 照旧）。旧调用零迁移
pub async fn run_rev_server(
    bind: SocketAddr,
    cfg: ServerConfig,
    psk: Option<[u8; 32]>,
    tcp_bind: SocketAddr,
    target_port: u16,
) -> std::io::Result<()> {
    run_rev_server_multi(
        bind,
        cfg,
        psk,
        vec![RevFront {
            tcp_bind,
            device_id: String::new(),
            target_port,
        }],
    )
    .await
}

/// 反连服务器腿（多 front 形态）：QUIC 监听（UDP 62694 一口）；注册连接
/// 按 device_id 分槽认领（同槽新注册挤掉旧的——旧机掉线残留不会让新机
/// 认领不上；异槽各占各 = 一口多设备零互踢，钉在 oneport_spec）；每个
/// front 在本槽认领存活期内绑自己的本机 TCP 口，入站 TCP → 沿本槽连接
/// 开流（v2 槽写 v2 流头，legacy 槽写 v1 端口头）→ splice。连接死 =
/// 收 TCP 监听器回等认领（桥口让位给 ssh 伴生兜底——口随供应商走，与
/// 今天 sshd 持有它的语义同构）
pub async fn run_rev_server_multi(
    bind: SocketAddr,
    cfg: ServerConfig,
    psk: Option<[u8; 32]>,
    fronts: Vec<RevFront>,
) -> std::io::Result<()> {
    let ep = Endpoint::server(cfg, bind).map_err(|e| std::io::Error::other(e.to_string()))?;
    let fails = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        std::net::IpAddr,
        (u32, std::time::Instant),
    >::new()));
    // BAR-157：accept 驱动独立成任务，握手+验签每条 Incoming 一个任务。
    // 旧制认领循环串行 await Incoming——quinn 的 Incoming 在应用 accept
    // 之前不回第一个包，而客户端弃连（8s 握手超时）后服务器要等满
    // REV_IDLE_TIMEOUT 才收尸 → 一具陈尸把 accept 停摆 60s，期间所有
    // 新握手零回包，弃尸排队永远排不完（62694 pcap 实录：60s cadence
    // 一具陈尸一组 PTO 序列，23 次新鲜尝试零回包）。考题
    // spec_m4_bar157_弃尸风暴_诚实客户端不被憋死。
    //
    // BAR-247 改道：认领面 = watch 活账（device_id → 注册连接），注册任务
    // 直写、front 任务订——单认领循环与 mpsc 随多槽化退场（「只留最新」
    // 语义按槽天然成立：同槽 insert 即顶旧）。
    let (claim_tx, claim_rx) =
        tokio::sync::watch::channel(std::collections::HashMap::<String, Connection>::new());
    let mut tasks = tokio::task::JoinSet::new();
    {
        let fails = std::sync::Arc::clone(&fails);
        let claim_tx = claim_tx.clone();
        tasks.spawn(async move {
            while let Some(inc) = ep.accept().await {
                let fails = std::sync::Arc::clone(&fails);
                let claim_tx = claim_tx.clone();
                tokio::spawn(async move {
                    // 握手等待给硬顶（客户端 8s 弃连点之上）——陈尸自己
                    // 烂在自己的任务里，不拖累 accept 驱动
                    let conn = match tokio::time::timeout(HANDSHAKE_TIMEOUT * 2, async {
                        inc.await
                    })
                    .await
                    {
                        Ok(Ok(c)) => c,
                        _ => return,
                    };
                    let ip = conn.remote_address().ip();
                    eprintln!("[na-quic] 反连连接 {ip}");
                    {
                        let g = fails.lock().await;
                        if let Some(&(n, t0)) = g.get(&ip)
                            && ban_verdict(n, t0.elapsed().as_secs())
                        {
                            eprintln!("[na-quic] 封禁中拒连 {ip}（第 {n} 次连败）");
                            conn.close(1u32.into(), b"auth banned");
                            return;
                        }
                    }
                    // 注册流：第一条流必须是合法注册（v1 = REG_PORT+标签
                    // 归 legacy 槽；v2 = 魔数+id+REG_PORT+tag2 归 id 槽）
                    let Some(id) = read_registration(&conn, &psk).await else {
                        let mut g = fails.lock().await;
                        let e = g.entry(ip).or_insert((0, std::time::Instant::now()));
                        if e.1.elapsed().as_secs() >= AUTH_BAN_SECS {
                            *e = (0, std::time::Instant::now());
                        }
                        e.0 += 1;
                        eprintln!("[na-quic] 反连注册验签失败 {ip}（第 {} 次连败）", e.0);
                        conn.close(1u32.into(), b"bad registration");
                        return;
                    };
                    // 认领：同槽顶旧（旧连接 close → 其 front 服务段收尸
                    // 让位），异槽插新；幂等——同连接重复注册零动作
                    let mut replaced = None;
                    let mine = conn.stable_id();
                    claim_tx.send_if_modified(|m| {
                        if m.get(&id).map(|c| c.stable_id()) == Some(mine) {
                            return false;
                        }
                        replaced = m.insert(id.clone(), conn.clone());
                        true
                    });
                    if let Some(old) = replaced {
                        eprintln!(
                            "[na-quic] 反连顶替：槽 {} 旧连接 {} 收尸（新连接 {mine} 认领，{ip}）",
                            slot_name(&id),
                            old.stable_id()
                        );
                        old.close(0u32.into(), b"replaced");
                    } else {
                        eprintln!(
                            "[na-quic] 反连认领：槽 {} ← 连接 {mine}（{ip}）",
                            slot_name(&id)
                        );
                    }
                });
            }
        });
    }
    // —— front 任务组：每 front 一个生命周期循环（认领在才绑口、死即
    // 让位、复认领复绑），accept 驱动同组。任何组件退场 = 反连面半残，
    // 响亮返回（部署侧进程级看门狗收尸重启），不许带病空转
    for front in fronts {
        eprintln!(
            "[na-quic] 反连 front 就位：槽 {} → 本机 TCP {}",
            slot_name(&front.device_id),
            front.tcp_bind
        );
        tasks.spawn(run_rev_front(front, claim_rx.clone(), claim_tx.clone()));
    }
    // 任何组件先退场 = 反连面半残，响亮返回（部署侧进程级看门狗收尸
    // 重启），不许带病空转。join_next 首返即定论——留在集合里的组件
    // 随本函数返回被 JoinSet drop 一并收走
    if let Some(r) = tasks.join_next().await {
        return match r {
            Ok(()) => Err(std::io::Error::other("QUIC 反连面组件退场（半残拒跑）")),
            Err(e) => Err(std::io::Error::other(format!("QUIC 反连面组件 panic: {e}"))),
        };
    }
    Ok(())
}

/// 注册流裁决（run_rev_server_multi 私有）：第一条流必须是合法注册——
/// v1（REG_PORT + 标签）归 legacy 槽（空 id 返回）；v2（魔数+id+
/// REG_PORT+tag2）归 id 槽。None = 坏注册（调用方记连败 + 关连接）
async fn read_registration(conn: &Connection, psk: &Option<[u8; 32]>) -> Option<String> {
    let (_send, mut recv) = conn.accept_bi().await.ok()?;
    let mut hdr = [0u8; 2];
    recv.read_exact(&mut hdr).await.ok()?;
    if u16::from_be_bytes(hdr) == V2_MAGIC {
        let (id, port) = read_v2_fields(&mut recv).await?;
        if port != REG_PORT {
            return None;
        }
        if let Some(k) = psk {
            let mut tag = [0u8; AUTH_TAG_LEN];
            let ok = recv.read_exact(&mut tag).await.is_ok()
                && ct_eq(&tag, &auth_tag2(k, &id, REG_PORT));
            if !ok {
                return None;
            }
        }
        Some(id)
    } else {
        if parse_port_header(&hdr) != REG_PORT {
            return None;
        }
        if let Some(k) = psk {
            let mut tag = [0u8; AUTH_TAG_LEN];
            let ok = recv.read_exact(&mut tag).await.is_ok() && ct_eq(&tag, &auth_tag(k, REG_PORT));
            if !ok {
                return None;
            }
        }
        Some(String::new())
    }
}

/// 单个桥前的生命周期循环（run_rev_server_multi 私有）：本槽有认领连接
/// 才绑 TCP 口服务；连接死/被顶 → 服务段返回 → 槽里若仍是这条（自然死）
/// 主动摘掉防循环空转，回等下一次认领；被顶的情形 = 槽里已是新连接，
/// 直接拿去续服
async fn run_rev_front(
    front: RevFront,
    mut rx: tokio::sync::watch::Receiver<std::collections::HashMap<String, Connection>>,
    tx: tokio::sync::watch::Sender<std::collections::HashMap<String, Connection>>,
) {
    loop {
        let conn = loop {
            if let Some(c) = rx.borrow().get(&front.device_id) {
                break c.clone();
            }
            if rx.changed().await.is_err() {
                return; // 认领面全灭 = 服务器关张
            }
        };
        serve_rev_tcp(&conn, front.tcp_bind, front.target_port, &front.device_id).await;
        tx.send_if_modified(|m| match m.get(&front.device_id) {
            Some(c) if c.stable_id() == conn.stable_id() => {
                m.remove(&front.device_id);
                true
            }
            _ => false,
        });
    }
}

/// TCP 服务段（run_rev_front 私有）：认领连接存活期内绑 tcp_bind，每个
/// 入站 → 开流写目标端口流头 → splice；连接死返回。device_id 非空 =
/// v2 槽，写 v2 流头（魔数+id+端口）；空 = legacy 槽，写 v1 的 2 字节
/// 端口头（旧核零改动）
async fn serve_rev_tcp(conn: &Connection, tcp_bind: SocketAddr, target_port: u16, device_id: &str) {
    let Ok(listener) = tokio::net::TcpListener::bind(tcp_bind).await else {
        return; // 口被占（ssh 伴生兜底在场？）——等下轮认领再说
    };
    loop {
        tokio::select! {
            a = listener.accept() => {
                let Ok((tcp, _)) = a else { return };
                let Ok((mut send, recv)) = conn.open_bi().await else { return };
                let head = if device_id.is_empty() {
                    port_header(target_port).to_vec()
                } else {
                    stream_header_v2(device_id, target_port).expect("front id 已过闸")
                };
                tokio::spawn(async move {
                    if send.write_all(&head).await.is_err() {
                        return;
                    }
                    splice(tcp, send, recv).await;
                });
            }
            _ = conn.closed() => return,
        }
    }
}

/// 反连客户端腿（手机侧，核内线程）：拨出到服务器 UDP 62694，先发
/// 注册流（REG_PORT + psk 标签），随后 accept_bi 循环——服务器每开
/// 一条流 = 9022 那边来了一个连接，读端口头回联本机 127.0.0.1:{port}
/// （na sshd 8024）→ splice。连接死 = 本函数返回（看门狗外侧重建，
/// 与正连腿同款事件驱动）
pub async fn run_rev_client(
    server: SocketAddr,
    sni: &str,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
) -> std::io::Result<()> {
    run_rev_client_inner(server, sni, cfg, psk, None).await
}

/// 反连客户端腿 v2（BAR-247 改道「一口多设备」）：注册流带设备 id
/// （魔数+id+REG_PORT+tag2），服务器按 id 分槽认领、把本机对应 front
/// 的流量路由进本条连接；业务流按 v2 流头读。非法 id fail-loud 拒起
pub async fn run_rev_client_v2(
    server: SocketAddr,
    sni: &str,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
    device_id: &str,
) -> std::io::Result<()> {
    if !valid_device_id(device_id) {
        return Err(std::io::Error::other(format!(
            "非法设备 id {device_id:?}（1..=32 字节 ASCII 字母数字与 -_）"
        )));
    }
    run_rev_client_inner(server, sni, cfg, psk, Some(device_id)).await
}

async fn run_rev_client_inner(
    server: SocketAddr,
    sni: &str,
    cfg: ClientConfig,
    psk: Option<[u8; 32]>,
    device_id: Option<&str>,
) -> std::io::Result<()> {
    let mut ep = Endpoint::client(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    ep.set_default_client_config(cfg);
    let conn: Connection = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        ep.connect(server, sni)
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    )
    .await
    .map_err(|_| std::io::Error::other("QUIC 反连握手超时（UDP 黑洞？）"))?
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    // 注册流：验客户端证（服务器不验不认领，设计 §四）
    let (mut send, _recv) = conn
        .open_bi()
        .await
        .map_err(|e| std::io::Error::other(format!("注册流开不出: {e}")))?;
    match device_id {
        Some(id) => {
            let mut h = stream_header_v2(id, REG_PORT).expect("id 已过闸");
            if let Some(k) = psk {
                h.extend_from_slice(&auth_tag2(&k, id, REG_PORT));
            }
            send.write_all(&h)
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        None => {
            send.write_all(&port_header(REG_PORT))
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if let Some(k) = psk {
                send.write_all(&auth_tag(&k, REG_PORT))
                    .await
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
        }
    }
    let _ = send.finish();
    let dev: Option<String> = device_id.map(str::to_string);
    loop {
        let (_send2, mut recv) = match conn.accept_bi().await {
            Ok(s) => s,
            Err(e) => return Err(std::io::Error::other(format!("QUIC 反连连接已死: {e}"))),
        };
        let dev = dev.clone();
        tokio::spawn(async move {
            // 读流头：v2 = 魔数+id+端口（id 为服务器回显，无消费价值但字节
            // 必须读走）；v1 = 2 字节端口
            let port = if dev.is_some() {
                let mut m = [0u8; 2];
                if recv.read_exact(&mut m).await.is_err() {
                    return;
                }
                if u16::from_be_bytes(m) != V2_MAGIC {
                    return; // v2 槽连接上服务器只发 v2 头，否则 = 串线
                }
                let Some((_id, port)) = read_v2_fields(&mut recv).await else {
                    return;
                };
                port
            } else {
                let mut hdr = [0u8; 2];
                if recv.read_exact(&mut hdr).await.is_err() {
                    return;
                }
                parse_port_header(&hdr)
            };
            if port == REG_PORT {
                return; // 注册口不是业务口
            }
            let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
            let Ok(tcp) = tokio::net::TcpStream::connect(target).await else {
                return;
            };
            splice(tcp, _send2, recv).await;
        });
    }
}
