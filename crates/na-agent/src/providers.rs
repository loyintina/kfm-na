//! providers.rs — /root/.kfm/provider.json（schema v2）读取与裁决。
//!
//! v1 只放行 auth=api_key 的 OpenAI 兼容源（bigmodel-coding / deepseek）。
//! kimi-code（oauth）挂账：机械拒绝并指向工单⑤——不许静默回退别家
//! （provider 选择的语义与 src/providers.rs「无静默回退」同款）。

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
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
    if auth != "api_key" {
        // 挂账工单⑤：kimi-code oauth 通道（managed credential_ref）v1 不做
        return Err(format!(
            "provider {name} auth={auth} v1 不支持——oauth 方言挂账工单⑤；\
             v1 可用：bigmodel-coding / deepseek"
        ));
    }
    let base_url = p
        .get("base_url")
        .and_then(|b| b.as_str())
        .ok_or_else(|| format!("provider {name} 缺 base_url"))?
        .to_string();
    let api_key = p
        .get("api_key")
        .and_then(|k| k.as_str())
        .ok_or_else(|| format!("provider {name} 缺 api_key"))?
        .to_string();
    Ok(Provider {
        name: name.to_string(),
        base_url,
        api_key,
    })
}
