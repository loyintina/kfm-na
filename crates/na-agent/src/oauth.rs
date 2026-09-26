//! oauth.rs — kimi-code OAuth 凭证（auth=oauth 方言，BAR-164 工单⑤）。
//!
//! 契约真相源 = 官方 kimi-cli 源码（kimi_cli/auth/oauth.py 本机实录）：
//! - 凭证真身 = $KIMI_CODE_HOME/credentials/<name>.json（KIMI_CODE_HOME
//!   缺省 ~/.kimi-code；provider.json 里 credential_ref 写的 ~/…/oauth
//!   是 0 字节遗留死件，不读它）
//! - 刷新 = POST {oauth_host}/api/oauth/token（form 表单：client_id +
//!   grant_type=refresh_token + refresh_token；oauth_host 缺省
//!   https://auth.kimi.com，env KIMI_CODE_OAUTH_HOST/KIMI_OAUTH_HOST
//!   可覆盖）；响应 {access_token,refresh_token,expires_in,scope,
//!   token_type} → expires_at = now + expires_in
//! - 回写 = 同目录 tmp + fsync + chmod 0600 + rename（官方原子流照抄）
//!
//! 纪律：引用即取——每次调用现读现解析，不缓存长持；**token 本体永不
//! 进日志/报错/wire**（报错只给路径/字段名/状态码；可能回显 token 的
//! 上游错误体经 redact 抹除）。过期/缺失/坏件全走机械报错（明确提示
//! 重跑 kimi /login）——不 panic、不静默回退别家。

use std::path::Path;

/// 过期提前量（官方 REFRESH_INTERVAL 60s 同档：余量 ≤60s 就当过期刷）
pub const EXPIRY_SKEW_SECS: u64 = 60;
/// 官方 client_id（kimi_cli/auth/oauth.py 公开常量，非秘密）
pub const KIMI_CODE_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
/// 官方缺省 oauth host
pub const DEFAULT_OAUTH_HOST: &str = "https://auth.kimi.com";
/// 重新登录提示（机械报错统一口径）
pub const REAUTH_HINT: &str = "凭证过期或失效——重跑 kimi /login 刷新登录态";

/// 凭证（三字段是 na-agent 消费面；scope/token_type 回写时补默认值）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_token: String,
    /// 空串 = 无（过期即 Reauth）
    pub refresh_token: String,
    /// unix 秒（官方文件存 float，截断取整）
    pub expires_at: u64,
}

/// 凭证文件路径（官方约定：<home>/credentials/<name>.json）
pub fn credential_path(kimi_home: &str, name: &str) -> String {
    format!(
        "{}/credentials/{name}.json",
        kimi_home.trim_end_matches('/')
    )
}

/// 解析凭证 JSON（A 档纯函数）。报错文本只给字段名——值（含坏件里
/// 埋的任意内容）一概不回显（泄密红线）
pub fn parse_credentials(json: &str) -> Result<Credentials, String> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| "凭证非合法 JSON（重跑 kimi /login）".to_string())?;
    let access_token = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "凭证缺 access_token（重跑 kimi /login）".to_string())?
        .to_string();
    let refresh_token = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    // expires_at 官方存 float；整数/浮点都认，非数判负
    let expires_at =
        v.get("expires_at")
            .and_then(|x| x.as_f64())
            .filter(|f| *f >= 0.0)
            .ok_or_else(|| "凭证缺 expires_at（重跑 kimi /login）".to_string())? as u64;
    Ok(Credentials {
        access_token,
        refresh_token,
        expires_at,
    })
}

/// 裁决三态（A 档纯函数）：直接用 / 去刷新 / 机械报错重登录
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 未过期（余量 > skew）——access_token 直接用
    Fresh(String),
    /// 已过期（含 skew 内）且有 refresh_token——拿去刷
    Refresh(String),
    /// 已过期且无 refresh_token——机械报错（不 panic 不回退别家）
    Reauth(String),
}

pub fn verdict(c: &Credentials, now_unix: u64) -> Verdict {
    if c.expires_at > now_unix + EXPIRY_SKEW_SECS {
        return Verdict::Fresh(c.access_token.clone());
    }
    if c.refresh_token.is_empty() {
        return Verdict::Reauth(REAUTH_HINT.to_string());
    }
    Verdict::Refresh(c.refresh_token.clone())
}

/// 刷新请求体（form-urlencoded；官方字段三件套）
pub fn build_refresh_body(refresh_token: &str) -> String {
    format!(
        "client_id={KIMI_CODE_CLIENT_ID}&grant_type=refresh_token&refresh_token={refresh_token}"
    )
}

/// oauth host（env KIMI_CODE_OAUTH_HOST > KIMI_OAUTH_HOST > 缺省）
pub fn oauth_host() -> String {
    std::env::var("KIMI_CODE_OAUTH_HOST")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("KIMI_OAUTH_HOST")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_OAUTH_HOST.to_string())
}

/// 抹除（泄密红线兜底）：文本里出现任何 secret 一律替换——上游错误体
/// 可能回显 token，报错前先过这道
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for s in secrets {
        if !s.is_empty() && out.contains(s) {
            out = out.replace(s, "«已抹除»");
        }
    }
    out
}

/// 刷新响应解析（A 档纯函数）→ 回写用凭证文件 JSON。
/// - 200：access_token/expires_in 必需；refresh_token 缺 = 沿用旧的
///   （不轮转端宽容）；expires_at = now + expires_in
/// - 401/403：登录态死了——Reauth 机械报错（官方 OAuthUnauthorized 同义）
/// - 其他非 200：HTTP 错（body 抹除 token 后截断 512）
pub fn parse_refresh_response(
    status: u16,
    body: &str,
    now_unix: u64,
    old: &Credentials,
) -> Result<String, String> {
    let secrets = [old.access_token.as_str(), old.refresh_token.as_str()];
    if status == 401 || status == 403 {
        return Err(REAUTH_HINT.to_string());
    }
    if status != 200 {
        let cap: String = redact(body, &secrets).chars().take(512).collect();
        return Err(format!("刷新失败 HTTP {status}: {cap}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "刷新响应非合法 JSON".to_string())?;
    let access = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "刷新响应缺 access_token".to_string())?;
    let expires_in = v
        .get("expires_in")
        .and_then(|x| x.as_f64())
        .ok_or_else(|| "刷新响应缺 expires_in".to_string())? as u64;
    let refresh = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(&old.refresh_token);
    let scope = v
        .get("scope")
        .and_then(|x| x.as_str())
        .unwrap_or("kimi-code");
    let token_type = v
        .get("token_type")
        .and_then(|x| x.as_str())
        .unwrap_or("Bearer");
    Ok(serde_json::json!({
        "access_token": access,
        "refresh_token": refresh,
        "expires_at": now_unix + expires_in,
        "scope": scope,
        "token_type": token_type,
        "expires_in": expires_in,
    })
    .to_string())
}

/// 原子回写（官方流照抄：同目录 tmp + fsync + chmod 0600 + rename；
/// 中途失败清 tmp 不留尸）
pub fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let dir = path
        .parent()
        .ok_or_else(|| "凭证路径无父目录".to_string())?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("cred"),
        std::process::id()
    ));
    let result = (|| -> Result<(), String> {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("建 tmp 失败: {e}"))?;
        f.write_all(content.as_bytes())
            .and_then(|_| f.sync_all())
            .map_err(|e| format!("写 tmp 失败: {e}"))?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod 0600 失败: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("rename 失败: {e}"))?;
        Ok(())
    })();
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    result
}
