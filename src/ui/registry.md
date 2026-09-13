# ui 控件注册表（第 3 层，2026-09-01 立形）

> 每个控件一行：状态核/视图/注入通道/考题/token 字段/规格文档。
> 故障定位坐标的查询入口：症状 → 查此表得控件 → 控件内逐层走判据。
> 新控件入册才算立形；视图不认字面颜色（token 层），不读邻居状态核。

| 控件 | 状态核 | 视图 | 注入通道 | 考题 | token 字段 | 规格文档 |
|---|---|---|---|---|---|---|
| orb 光球 | `ai_presence::AiPresenceState` | `src/ui/orb.rs` | orb-inject | ai_presence_spec 逐像素钉 | 无（自带 D8 拟合配方，改配方须重跑 orb-fit.py） | docs/active/ai-presence.md |
| prompt_bar 输入栏 | `input_bar::InputBarState` | `ui/prompt_bar.rs`（impl TermView） | bar-inject | input_bar_spec 18 + termview_spec caret/bar 系列 | `theme.bar.*` | docs/active/插件档案-输入栏.md |
| keybar 快捷键行 | 无状态核（直绘+keymap 服务） | `ui/keybar.rs`（impl TermView） | keys-in | keybar_spec 6 + theme_spec keybar | `theme.keybar.*` | docs/ledger/bugs.md BAR-017/018 |
| ai_page 全屏占位 | `ai_presence::Page` | `termview::render_ai_page`（期0 组件④将替换） | — | ai_presence_spec 冒烟 | AI_PAGE_*（待 token 化） | docs/active/ai-presence.md |
| selection 选择/放大镜 | `termview.selection` | `termview`（与终端网格耦合，随选择系统重构入册） | touch-in | select_spec 28 | SELECT_BG | docs/ledger/bugs.md BAR-025 |
| gear 设置钮 | 无（栈归 ai_presence） | `ui/gear.rs`（termview 终卡槽内 paint） | touch-in（android_app Started 命中分流） | gear_spec 3 | GEAR_INK（随终卡族待 token 化） | docs/active/ai-presence.md §五B |
| tab_bar 标签栏 | `ui/tab_bar.rs`（弹簧滑块） | `termview::paint_cfg_tab_bar_impl` | touch-in（Started 行带仲裁） | tab_bar_spec + theme_spec | TAB_*（termview 标定） | docs/active/theme.md §四 |
| cfg_page 配置页 | `ui/cfg_page.rs`（目录核+tab/modal 维） | `termview::paint_cfg_pool_content_impl` | touch-in（池区手势槽） | cfg_page_spec 23 | CELL_* 网格标定 | docs/active/theme.md §五 + 设置页.md |
| modal 跳框 | `ui/cfg_page.rs` modal 维 + `ui/modal.rs` 几何 | `termview::paint_modal_impl` | touch-in（模态手势槽吃下层） | modal_spec 10 | CELL_* 网格标定 | docs/active/theme.md §六 跳框 |
| comp_registry 组件池 | `ui/comp_registry.rs`（常量表=唯一信息源） | 读表渲染（复用双池行+跳框） | — | comp_registry_spec 7（symbol 实存棘轮） | 无 | docs/active/theme.md §五 目录语义 7 |
