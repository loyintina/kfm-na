//! config.rs — line.toml：线配置（只记创建时间/默认 provider/model/workdir，
//! 不搞注册表）。手写 key = "value" 子集解析——不引 toml crate（离线
//! vendor 纪律），四字段平面结构用不起一个解析器依赖。

/// 默认燃料：bigmodel-coding 的 glm-5.3-flash（coding 套餐）。工单拍板。
pub const DEFAULT_PROVIDER: &str = "bigmodel-coding";
pub const DEFAULT_MODEL: &str = "glm-5.3-flash";
pub const DEFAULT_WORKDIR: &str = "/root/kfm-na";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineConfig {
    pub created_at: String,
    pub provider: String,
    pub model: String,
    pub workdir: String,
}

impl LineConfig {
    pub fn new(created_at: &str) -> Self {
        Self {
            created_at: created_at.to_string(),
            provider: DEFAULT_PROVIDER.into(),
            model: DEFAULT_MODEL.into(),
            workdir: DEFAULT_WORKDIR.into(),
        }
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# line.toml — na agent 线配置（花名册=线文件夹，本文件只记四项）\n\
             created_at = \"{}\"\nprovider = \"{}\"\nmodel = \"{}\"\nworkdir = \"{}\"\n",
            self.created_at, self.provider, self.model, self.workdir
        )
    }

    /// 解析（宽容：缺字段补默认；坏行跳过——line.toml 是人工可改文件，
    /// 不许一行手滑炸掉整条线）。
    pub fn from_toml(text: &str) -> Self {
        let mut cfg = Self::new("");
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "created_at" => cfg.created_at = v.to_string(),
                "provider" if !v.is_empty() => cfg.provider = v.to_string(),
                "model" if !v.is_empty() => cfg.model = v.to_string(),
                "workdir" if !v.is_empty() => cfg.workdir = v.to_string(),
                _ => {}
            }
        }
        cfg
    }
}
