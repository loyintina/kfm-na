//! oauth_spec.rs — kimi-code OAuth 方言考题（A 档，BAR-164 工单⑤）。
//!
//! 契约真相源：官方 kimi-cli 源码（本机 site-packages/kimi_cli/auth/
//! oauth.py）+ 用户探明事实（凭证真身 $KIMI_CODE_HOME/credentials/
//! <name>.json，短命 token 15 分钟，k3-256k always-thinking 要足量
//! max_tokens）。纪律：先验证红，答案生成到绿，绿后变异抽检。
//! 本文件是考题，生成器不许改。

use na_agent::dialect::{build_request, parse_response};
use na_agent::oauth::{self, Verdict};
use na_agent::providers;

/// canary 假 token（不是真凭证——真凭证永不进仓）
const CANARY_ACCESS: &str = "CANARY_ACCESS_TOKEN_0123456789";
const CANARY_REFRESH: &str = "CANARY_REFRESH_TOKEN_0123456789";

fn cred_json(access: &str, refresh: &str, expires_at: u64) -> String {
    format!(
        r#"{{"access_token":"{access}","refresh_token":"{refresh}","expires_at":{expires_at},"scope":"kimi-code","token_type":"Bearer","expires_in":900}}"#
    )
}

// ---- 凭证解析：形状与机械报错 ----

#[test]
fn spec_bar164_凭证_解析形状与坏件() {
    let c = oauth::parse_credentials(&cred_json(CANARY_ACCESS, CANARY_REFRESH, 1_800_000_000))
        .expect("合法凭证");
    assert_eq!(c.access_token, CANARY_ACCESS);
    assert_eq!(c.refresh_token, CANARY_REFRESH);
    assert_eq!(c.expires_at, 1_800_000_000);
    // expires_at 是 float（官方存 time.time()）也要吃
    let c = oauth::parse_credentials(
        &cred_json(CANARY_ACCESS, CANARY_REFRESH, 0)
            .replace("\"expires_at\":0", "\"expires_at\":1800000000.5"),
    )
    .expect("float expires_at 宽容");
    assert_eq!(c.expires_at, 1_800_000_000);
    // 坏件全判负：非 JSON / 缺 access_token / 缺 expires_at / access_token 空
    assert!(oauth::parse_credentials("不是 json").is_err());
    assert!(oauth::parse_credentials(r#"{"refresh_token":"x","expires_at":1}"#).is_err());
    assert!(oauth::parse_credentials(r#"{"access_token":"x","refresh_token":"y"}"#).is_err());
    assert!(
        oauth::parse_credentials(&cred_json("", CANARY_REFRESH, 1)).is_err(),
        "空 access_token 判负"
    );
}

#[test]
fn spec_bar164_凭证_报错文本永不泄token() {
    // 坏 JSON 里埋 canary——报错只许说「非合法 JSON」，不许回显内容
    let err = oauth::parse_credentials(&format!("{{坏 json {CANARY_ACCESS}")).unwrap_err();
    assert!(!err.contains(CANARY_ACCESS), "报错泄 token: {err}");
    let err = oauth::parse_credentials(
        &cred_json(CANARY_ACCESS, CANARY_REFRESH, 0)
            .replace("\"expires_at\":0", "\"expires_at\":\"不是数字\""),
    )
    .unwrap_err();
    assert!(!err.contains(CANARY_ACCESS), "报错泄 access: {err}");
    assert!(!err.contains(CANARY_REFRESH), "报错泄 refresh: {err}");
}

#[test]
fn spec_bar164_凭证_路径形状() {
    assert_eq!(
        oauth::credential_path("/root/.kimi-code", "kimi-code"),
        "/root/.kimi-code/credentials/kimi-code.json"
    );
}

// ---- 过期裁决：60s skew + 三态 ----

#[test]
fn spec_bar164_裁决_三态与skew() {
    let now = 1_800_000_000u64;
    // 未过期（余量 > 60s）→ Fresh 直接用
    let c = oauth::parse_credentials(&cred_json(CANARY_ACCESS, CANARY_REFRESH, now + 900)).unwrap();
    match oauth::verdict(&c, now) {
        Verdict::Fresh(t) => assert_eq!(t, CANARY_ACCESS),
        _ => panic!("未过期必须 Fresh"),
    }
    // 余量恰 60s = 已过期（skew 提前判）→ 有 refresh → Refresh
    let c = oauth::parse_credentials(&cred_json(CANARY_ACCESS, CANARY_REFRESH, now + 60)).unwrap();
    match oauth::verdict(&c, now) {
        Verdict::Refresh(rt) => assert_eq!(rt, CANARY_REFRESH),
        _ => panic!("skew 内必须 Refresh"),
    }
    // 已过期且无 refresh_token → Reauth 机械报错（不 panic 不回退别家）
    let c = oauth::parse_credentials(&cred_json(CANARY_ACCESS, "", now - 10)).unwrap();
    match oauth::verdict(&c, now) {
        Verdict::Reauth(msg) => {
            assert!(msg.contains("kimi"), "提示重跑 login: {msg}");
            assert!(!msg.contains(CANARY_ACCESS), "报错泄 token: {msg}");
        }
        _ => panic!("过期无 refresh 必须 Reauth"),
    }
}

// ---- 刷新：请求体 / 响应解析 / 回写 ----

#[test]
fn spec_bar164_刷新_请求体形状() {
    let body = oauth::build_refresh_body(CANARY_REFRESH);
    assert!(body.contains("grant_type=refresh_token"), "{body}");
    assert!(body.contains("client_id="), "{body}");
    assert!(
        body.contains(&format!("refresh_token={CANARY_REFRESH}")),
        "刷新请求体带 refresh_token（这是正主）: {body}"
    );
}

#[test]
fn spec_bar164_刷新_响应解析与expires_at换算() {
    let now = 1_800_000_000u64;
    let old = oauth::parse_credentials(&cred_json(CANARY_ACCESS, CANARY_REFRESH, now - 5)).unwrap();
    // 200：新凭证文件 JSON——access/refresh 换新，expires_at = now + expires_in
    let resp = r#"{"access_token":"NEW_ACCESS","refresh_token":"NEW_REFRESH","expires_in":900,"scope":"kimi-code","token_type":"Bearer"}"#;
    let file_json = oauth::parse_refresh_response(200, resp, now, &old).expect("200 必成");
    let new = oauth::parse_credentials(&file_json).expect("回写件必须能回读");
    assert_eq!(new.access_token, "NEW_ACCESS");
    assert_eq!(new.refresh_token, "NEW_REFRESH");
    assert_eq!(new.expires_at, now + 900, "expires_at = now + expires_in");
    // 响应缺 refresh_token = 沿用旧的（不轮转端宽容）
    let resp = r#"{"access_token":"NEW_ACCESS2","expires_in":900,"scope":"kimi-code","token_type":"Bearer"}"#;
    let file_json = oauth::parse_refresh_response(200, resp, now, &old).expect("缺 refresh 宽容");
    let new = oauth::parse_credentials(&file_json).unwrap();
    assert_eq!(new.refresh_token, CANARY_REFRESH, "沿用旧 refresh_token");
    // 401/403 → Reauth 机械报错（提示重跑 login，不 retry 不 panic）
    let err =
        oauth::parse_refresh_response(401, r#"{"error_description":"invalid_token"}"#, now, &old)
            .unwrap_err();
    assert!(err.contains("kimi"), "401 提示重跑 login: {err}");
    let err = oauth::parse_refresh_response(403, "{}", now, &old).unwrap_err();
    assert!(err.contains("kimi"), "403 同 401: {err}");
    // 500 → 普通 HTTP 错（调用方可retry，但不是 Reauth）
    let err = oauth::parse_refresh_response(500, "server error", now, &old).unwrap_err();
    assert!(err.contains("500"), "{err}");
}

#[test]
fn spec_bar164_刷新_报错体抹除token() {
    let now = 1_800_000_000u64;
    let old = oauth::parse_credentials(&cred_json(CANARY_ACCESS, CANARY_REFRESH, now - 5)).unwrap();
    // 上游错误体回显了 refresh_token（事故形态）——报错文本必须抹除
    let body = format!(r#"{{"error_description":"bad token {CANARY_REFRESH} hmm"}}"#);
    let err = oauth::parse_refresh_response(400, &body, now, &old).unwrap_err();
    assert!(!err.contains(CANARY_REFRESH), "报错泄 refresh: {err}");
    assert!(!err.contains(CANARY_ACCESS), "报错泄 access: {err}");
    assert!(err.contains("400"), "状态码留住: {err}");
}

#[test]
fn spec_bar164_回写_原子与0600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("bar164-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kimi-code.json");
    // 覆盖已有文件（旧件先放）
    std::fs::write(&path, "旧件").unwrap();
    let content = cred_json("A", "R", 1);
    oauth::write_atomic(&path, &content).expect("原子写");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "凭证必须 0600: {mode:o}");
    // 无 .tmp 残留
    let residue: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(residue.is_empty(), "tmp 残留: {residue:?}");
    std::fs::remove_dir_all(&dir).ok();
}

// ---- providers 裁决：oauth 路径落地 ----

fn provider_json(auth: &str, extra: &str) -> String {
    format!(
        r#"{{"providers":{{"kimi-code":{{"base_url":"https://api.kimi.com/coding/v1","auth":"{auth}"{extra}}}}}}}"#
    )
}

#[test]
fn spec_bar164_providers_oauth落地() {
    let p = providers::resolve(&provider_json("oauth", ""), "kimi-code").expect("oauth 放行");
    match &p.auth {
        providers::Auth::OAuth { credential_path } => {
            assert!(
                credential_path.ends_with("credentials/kimi-code.json"),
                "凭证路径按官方约定: {credential_path}"
            );
        }
        _ => panic!("oauth 必须落 OAuth 变体"),
    }
    assert_eq!(p.base_url, "https://api.kimi.com/coding/v1");
    // k3-256k always-thinking：缺省 max_tokens 给足（≥4096）
    assert!(
        p.max_tokens.unwrap_or(0) >= 4096,
        "thinking 模型 max_tokens 必须给足: {:?}",
        p.max_tokens
    );
    // 显式 max_tokens 覆盖缺省
    let p =
        providers::resolve(&provider_json("oauth", ",\"max_tokens\":8192"), "kimi-code").unwrap();
    assert_eq!(p.max_tokens, Some(8192));
    // api_key 路径原样（无 max_tokens 键 = None，行为不变）
    let json = r#"{"providers":{"glm":{"base_url":"https://x","auth":"api_key","api_key":"k"}}}"#;
    let p = providers::resolve(json, "glm").expect("api_key 放行");
    assert!(matches!(p.auth, providers::Auth::ApiKey(_)));
    assert_eq!(p.max_tokens, None);
    // 未知 auth 仍机械拒绝（不静默回退）
    let err = providers::resolve(&provider_json("weird", ""), "kimi-code").unwrap_err();
    assert!(err.contains("weird"), "{err}");
}

// ---- 方言：thinking 模型响应形状 + max_tokens 入体 ----

#[test]
fn spec_bar164_方言_thinking响应解析() {
    // k3-256k 真身形状：content 空串 + reasoning_content 在 + usage 带
    // completion_tokens_details.reasoning_tokens——解析不许断、usage 照记
    let body = r#"{
        "choices":[{"message":{"role":"assistant","content":"",
            "reasoning_content":"想了一大段"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":100,"completion_tokens":800,"total_tokens":900,
            "completion_tokens_details":{"reasoning_tokens":792}}
    }"#;
    let reply = parse_response(body).expect("thinking 形状不许断");
    assert_eq!(reply.content.as_deref(), Some(""));
    assert!(reply.tool_calls.is_empty());
    let u = reply.usage.expect("usage 账必须在");
    assert_eq!(u.completion_tokens, 800);
    // reasoning 与 tool_calls 共存（工具循环不断）
    let body = r#"{
        "choices":[{"message":{"role":"assistant","content":null,
            "reasoning_content":"先想",
            "tool_calls":[{"id":"c1","type":"function",
                "function":{"name":"read_file","arguments":"{\"path\":\"/x\"}"}}]}}],
        "usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}
    }"#;
    let reply = parse_response(body).expect("reasoning+tool_calls 共存不许断");
    assert_eq!(reply.tool_calls.len(), 1);
    assert_eq!(reply.tool_calls[0].function.name, "read_file");
}

#[test]
fn spec_bar164_方言_max_tokens入体() {
    let messages = vec![na_agent::dialect::Message::user("问")];
    let tools = vec![];
    // Some → 入体；None → 键不出现（api_key 老路行为逐字节不变）
    let body = build_request("k3-256k", &messages, &tools, Some(16384));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["max_tokens"], 16384);
    let body = build_request("glm-5.3-flash", &messages, &tools, None);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("max_tokens").is_none(), "None 不出 max_tokens 键");
}
