//! providers.rs — providers.json / .env 代字 fuse（kfmv4 配置语义复刻）。
//!
//! 契约真相源：docs/active/ai-presence.md §四B「配置复刻」——
//! resolveKey：先 process env 后 .env，缺失 → error 事件，绝不裸发代字。
//! 纯逻辑零 IO：文件读取在调用方（direct_api_brain 装配时）。

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// 原始 apiKey 字段：可能是 ${VAR} 代字，也可能是字面量，也可能为空
    pub api_key_raw: String,
    pub models: Vec<String>,
}

impl Provider {
    /// 按 id 或 name 匹配（kfmv4 chat.ts 同款：双字段都试，无静默回退）。
    pub fn find<'a>(providers: &'a [Provider], key: &str) -> Option<&'a Provider> {
        providers.iter().find(|p| p.id == key || p.name == key)
    }
}

/// 解析 kfmv4 风格 providers.json（数组，条目字段宽容缺省）。
pub fn parse_providers(json: &str) -> Result<Vec<Provider>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("providers.json 不是合法 JSON: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "providers.json 顶层必须是数组".to_string())?;
    let mut out = Vec::new();
    for item in arr {
        let s = |k: &str| {
            item.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let models = item
            .get("models")
            .and_then(|m| m.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(Provider {
            id: s("id"),
            name: s("name"),
            base_url: s("baseUrl"),
            api_key_raw: s("apiKey"),
            models,
        });
    }
    Ok(out)
}

/// 解析 .env：KEY=value 行，支持 # 注释、export 前缀、引号包裹、= 两侧空白。
pub fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        map.insert(k.trim().to_string(), v.to_string());
    }
    map
}

/// 合成查 key 环境：process env 优先，.env 兜底（kfmv4 resolveKey 顺序）。
pub fn merge_env(dotenv: &HashMap<String, String>) -> HashMap<String, String> {
    let mut merged = dotenv.clone();
    for (k, v) in std::env::vars() {
        merged.insert(k, v); // process 覆盖 dotenv
    }
    merged
}

/// 代字 fuse：`${VAR}` → 查环境，缺失 → Err（点名变量）；
/// 非代字 = 字面量直用；空串 = 未配置 → Err。绝不原样吐出 ${VAR}。
pub fn resolve_key(raw: &str, env: &HashMap<String, String>) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("apiKey 为空（未配置）".to_string());
    }
    if let Some(inner) = raw.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        let var = inner.trim();
        return env
            .get(var)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| format!("apiKey 代字 ${{{var}}} 缺失：env 与 .env 都查不到"));
    }
    Ok(raw.to_string())
}
