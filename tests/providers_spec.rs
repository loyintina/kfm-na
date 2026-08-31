//! tests/providers_spec.rs — A 档考题：providers.json/.env 代字 fuse（src/providers.rs）
//!
//! 契约真相源：docs/active/ai-presence.md §四B「配置复刻」——
//! resolveKey：先 process env 后 .env，缺失 → error，绝不裸发代字。
//! 纪律：先验证红，答案生成到绿，绿后变异抽检。本文件是考题，生成器不许改。

use kfm_na::providers::{Provider, merge_env, parse_dotenv, parse_providers, resolve_key};
use std::collections::HashMap;

const SAMPLE: &str = r#"[
  {
    "id": "Kimi",
    "name": "Kimi",
    "baseUrl": "https://api.kimi.com/coding/v1",
    "apiKey": "${KFM_PROVIDER_KIMI}",
    "models": ["kimi-for-coding-highspeed", "k3-256k"]
  },
  {
    "id": "智谱",
    "name": "智谱",
    "baseUrl": "https://open.bigmodel.cn/api/coding/paas/v4",
    "apiKey": "${KFM_PROVIDER_ZHIPU}",
    "models": ["glm-5.3-flash"]
  },
  {
    "id": "free",
    "name": "free",
    "baseUrl": "https://example.com/v1",
    "apiKey": "",
    "models": []
  }
]"#;

// ========== providers.json 解析 ==========

#[test]
fn parse_providers_basic_fields() {
    let ps = parse_providers(SAMPLE).expect("解析失败");
    assert_eq!(ps.len(), 3);
    let kimi = &ps[0];
    assert_eq!(kimi.id, "Kimi");
    assert_eq!(kimi.base_url, "https://api.kimi.com/coding/v1");
    assert_eq!(kimi.api_key_raw, "${KFM_PROVIDER_KIMI}");
    assert_eq!(kimi.models, vec!["kimi-for-coding-highspeed", "k3-256k"]);
}

#[test]
fn find_provider_by_id_or_name() {
    let ps = parse_providers(SAMPLE).unwrap();
    assert!(Provider::find(&ps, "Kimi").is_some(), "按 id 找");
    assert!(Provider::find(&ps, "智谱").is_some(), "按中文 id/name 找");
    assert!(Provider::find(&ps, "不存在").is_none());
}

// ========== .env 解析 ==========

#[test]
fn dotenv_parse_comments_quotes_whitespace() {
    let text = "# 注释\nKFM_PROVIDER_KIMI=sk-kimi-123\n\
KFM_PROVIDER_ZHIPU = \"sk-zhipu-456\"\nexport KFM_EXPORT=v1\nEMPTY=\n";
    let map = parse_dotenv(text);
    assert_eq!(
        map.get("KFM_PROVIDER_KIMI").map(String::as_str),
        Some("sk-kimi-123")
    );
    assert_eq!(
        map.get("KFM_PROVIDER_ZHIPU").map(String::as_str),
        Some("sk-zhipu-456")
    );
    assert_eq!(map.get("KFM_EXPORT").map(String::as_str), Some("v1"));
    assert_eq!(map.get("EMPTY").map(String::as_str), Some(""));
}

// ========== 代字 fuse ==========

fn env_with(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn fuse_resolves_from_merged_env() {
    let env = env_with(&[("KFM_PROVIDER_KIMI", "sk-real-key")]);
    assert_eq!(
        resolve_key("${KFM_PROVIDER_KIMI}", &env).unwrap(),
        "sk-real-key"
    );
}

#[test]
fn fuse_missing_var_is_error_never_sends_placeholder() {
    let env = env_with(&[]);
    let err = resolve_key("${KFM_PROVIDER_NOPE}", &env).unwrap_err();
    assert!(
        err.contains("KFM_PROVIDER_NOPE"),
        "错误必须点名缺失变量: {err}"
    );
}

#[test]
fn fuse_literal_key_passthrough() {
    let env = env_with(&[]);
    assert_eq!(resolve_key("sk-literal", &env).unwrap(), "sk-literal");
}

#[test]
fn fuse_empty_key_is_error() {
    let env = env_with(&[]);
    assert!(resolve_key("", &env).is_err(), "空 key = 未配置，必须报错");
}

#[test]
fn merge_env_process_env_wins_over_dotenv() {
    // process env 优先于 .env（kfmv4 resolveKey 语义）
    unsafe { std::env::set_var("NA_TEST_FUSE_PRIORITY", "from-process") };
    let dotenv = env_with(&[("NA_TEST_FUSE_PRIORITY", "from-dotenv")]);
    let merged = merge_env(&dotenv);
    assert_eq!(
        merged.get("NA_TEST_FUSE_PRIORITY").map(String::as_str),
        Some("from-process")
    );
    unsafe { std::env::remove_var("NA_TEST_FUSE_PRIORITY") };
}
