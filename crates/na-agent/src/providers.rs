//! providers.rs — /root/.kfm/provider.json（schema v2）读取与裁决。
//!
//! auth=api_key 的 OpenAI 兼容源（bigmodel-coding / deepseek）+ auth=oauth
//! 的 kimi-code（BAR-164 工单⑤落地：凭证引用即取，路径按官方约定
//! $KIMI_CODE_HOME/credentials/<name>.json——provider.json 里
//! credential_ref 写的 ~/…/oauth 是 0 字节遗留死件，不读它）。
//! 未知 auth 机械拒绝——不许静默回退别家（provider 选择的语义与
//! src/providers.rs「无静默回退」同款）。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// api_key 静态燃料（每次调用同一个 Bearer）
    ApiKey(String),
    /// oauth 引用即取（每次调用现读凭证文件；过期走 oauth.rs 刷新流）
    OAuth { credential_path: String },
}

/// k3-256k 是 always-thinking 模型（BAR-164 用户探明）：max_tokens 太小
/// 会被 reasoning_tokens 吃光导致 content 空——缺省给足，provider.json
/// 显式 "max_tokens" 可覆盖
pub const KIMI_CODE_DEFAULT_MAX_TOKENS: u32 = 16384;

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub auth: Auth,
    /// None = 请求体不出 max_tokens 键（api_key 老路行为逐字节不变）
    pub max_tokens: Option<u32>,
}

/// 从 provider.json 文本裁决出可用 provider（A 档纯函数——
/// 文件读取在调用方，key 不落日志不落仓）。
pub fn resolve(json_text: &str, name: &str) -> Result<Provider, String> {
    let v: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("provider.json 非合法 JSON: {e}"))?;
    let p = v
        .get("providers")
        .and_then(|ps| ps.get(name))
        .ok_or_else(|| format!("provider 不存在: {name}（无静默回退）"))?;
    let auth = p.get("auth").and_then(|a| a.as_str()).unwrap_or("");
    let base_url = p
        .get("base_url")
        .and_then(|b| b.as_str())
        .ok_or_else(|| format!("provider {name} 缺 base_url"))?
        .to_string();
    let max_tokens = p
        .get("max_tokens")
        .and_then(|m| m.as_u64())
        .map(|m| m as u32);
    match auth {
        "api_key" => {
            let api_key = p
                .get("api_key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| format!("provider {name} 缺 api_key"))?
                .to_string();
            Ok(Provider {
                name: name.to_string(),
                base_url,
                auth: Auth::ApiKey(api_key),
                max_tokens,
            })
        }
        "oauth" => {
            // 凭证路径按官方约定（credential_ref 字段是遗留死件不读）：
            // $KIMI_CODE_HOME/credentials/<name>.json，KIMI_CODE_HOME
            // 缺省 $HOME/.kimi-code
            let home = std::env::var("KIMI_CODE_HOME")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| format!("{h}/.kimi-code"))
                })
                .ok_or_else(|| "KIMI_CODE_HOME/HOME 均无——定位不了凭证目录".to_string())?;
            Ok(Provider {
                name: name.to_string(),
                base_url,
                auth: Auth::OAuth {
                    credential_path: crate::oauth::credential_path(&home, name),
                },
                // thinking 模型缺省给足（显式键覆盖）
                max_tokens: max_tokens.or(Some(KIMI_CODE_DEFAULT_MAX_TOKENS)),
            })
        }
        _ => Err(format!(
            "provider {name} auth={auth} 不支持——v2 可用：api_key / oauth；\
             无静默回退别家"
        )),
    }
}
