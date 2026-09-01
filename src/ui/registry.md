# ui 控件注册表（第 3 层，2026-09-01 立形）

> 每个控件一行：状态核/视图/注入通道/考题/token 字段/规格文档。
> 故障定位坐标的查询入口：症状 → 查此表得控件 → 控件内逐层走判据。
> 新控件入册才算立形；视图不认字面颜色（token 层），不读邻居状态核。

| 控件 | 状态核 | 视图 | 注入通道 | 考题 | token 字段 | 规格文档 |
|---|---|---|---|---|---|---|
| orb 光球 | `ai_presence::AiPresenceState` | `src/ui/orb.rs` | orb-inject | ai_presence_spec 逐像素钉 | 无（自带 D8 拟合配方，改配方须重跑 orb-fit.py） | docs/active/ai-presence.md |
| prompt_bar 输入栏 | `input_bar::InputBarState` | `termview::render_inputbar`（待迁 `ui/prompt_bar.rs`） | bar-inject | input_bar_spec 18 + termview_spec caret/bar 系列 | `theme.bar.*` | docs/active/插件档案-输入栏.md |
| keybar 快捷键行 | 无状态核（直绘+keymap 服务） | `termview::render_keybar`（待迁 `ui/keybar.rs`） | keys-in | keybar_spec 6 | KEYBAR_*（待 token 化） | docs/ledger/bugs.md BAR-017/018 |
| ai_page 全屏占位 | `ai_presence::Page` | `termview::render_ai_page`（期0 组件④将替换） | — | ai_presence_spec 冒烟 | AI_PAGE_*（待 token 化） | docs/active/ai-presence.md |
| selection 选择/放大镜 | `termview.selection` | `termview`（与终端网格耦合，随选择系统重构入册） | touch-in | select_spec 28 | SELECT_BG | docs/ledger/bugs.md BAR-025 |
