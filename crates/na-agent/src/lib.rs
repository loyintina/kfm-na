//! lib.rs — na-agent：na agent 运行时核心库（工单④ BAR-161，平台无关）
//!
//! 分层铁律：核心循环（agent.rs）/ provider 方言（dialect.rs + providers.rs）/
//! 工具面（tools.rs）/ 会话存储（session.rs）全部平台无关；宿主依赖
//! （fs 根 / 命令执行 / 时钟）收敛到 host.rs 的 Host trait 后面——
//! 考题用 FakeHost 注入证明核心逻辑 host 无关，真机用 StdHost。
//! 唯一网络面 = httpc.rs（rustls 直连 OpenAI 兼容端点，非流式 v1）。
//!
//! 挂账：kimi-code（oauth）provider 方言不做，归工单⑤（providers.rs 里
//! 机械拒绝并指向工单⑤）。

pub mod agent;
pub mod config;
pub mod dialect;
pub mod host;
pub mod httpc;
pub mod oauth;
pub mod providers;
pub mod session;
pub mod tools;
pub mod utc;
