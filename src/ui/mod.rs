//! ui/ — 控件库（UI 四层架构第 3 层，2026-09-01 立形，BeautifulUI 思路
//! 的 Rust 等价物）。
//!
//! 一个控件 = 一个自包含单元：状态核（A 档考题）+ 视图（只读 token，
//! 本层）+ 注入通道（gate）+ 规格卡（档案/registry）+ 覆盖棘轮按模块
//! 计账。控件视图不认字面颜色（token 层 src/theme.rs），不读邻居控件
//! 的状态核——越界即破坏故障坐标（症状→控件→逐层判据）。
//!
//! 成员与登记：src/ui/registry.md（名字/状态核/token 字段/通道/考题）。
//! TermEmu trait 的 render_* 方法转发到这里各控件的 render 本体。
//! seam/fx_spring 是采样缝与第一动画件（ui-base.md §三/§五），
//! 不属控件不登记 registry。ai_page 是对话页视口状态机（期 0④），
//! 纯逻辑无视图，同样不登记。
pub mod ai_page;
pub mod fx_spring;
pub mod keybar;
pub mod orb;
pub mod prompt_bar;
pub mod seam;
