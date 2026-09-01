//! theme.rs — 设计 token 层（UI 体系四层架构的第 2 层，2026-09-01 立层）。
//!
//! 四层：①图元族（termview Frame，SDF-AA）②**设计 token（本模块）**
//! ③控件层（输入栏/快捷键行/光球/对话页控件，只读 token 不认颜色）
//! ④主题包插件（换 Theme 即换肤，.so 热更上机）。
//!
//! 纪律：控件渲染代码里不许出现字面颜色常量——所有颜色从 Theme 走，
//! 换肤 = 换这个结构体的字段值。默认配方 = kfmv4 紫-青暗色系
//! （base.css 直译，配方逐项注释保留 CSS 原值，防手滑）。
//! 终端选区 SELECT_BG 暂留 termview（终端线，28 道像素钉着），
//! 后续 token 化跟随选择系统重构一起走。

/// 输入栏配色组（期 0 组件三）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarTheme {
    /// 栏带底：rgba(18,18,26,.85) 叠 #0a0a0f 的事后色
    pub bg: u32,
    /// 文本区内芯横向渐变两端：左紫调 (29,23,57) → 右青调 (12,40,54)，
    /// 比参考图实测稍沉一档（防塑料蓝，2026-08-31 ⑥号迭代）
    pub field_bg_l: u32,
    pub field_bg_r: u32,
    /// 聚焦亮一档（同向同幅）
    pub field_focus_bg_l: u32,
    pub field_focus_bg_r: u32,
    /// 正文
    pub text: u32,
    /// rgba(224,224,224,0.4) 叠近黑底的事后色
    pub placeholder: u32,
    /// 135° 描边渐变：--primary #7c3aed → rgba(0,212,255,0.8) 事后色
    pub border_l: u32,
    pub border_r: u32,
    /// 带顶发丝线中点：--accent #00d4ff
    pub accent: u32,
    /// 聚焦紫外发光（kfmv4 focus box-shadow 0 0 20px α0.35）
    pub glow: u32,
    /// 发送钮 135° 渐变（同描边）
    pub send_tl: u32,
    pub send_br: u32,
    /// 发送钮图标白（▶/⏸ 同色）
    pub send_tri: u32,
}

/// 全局主题（第 2 层单源；控件各取所需字段组）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub bar: BarTheme,
}

impl Default for Theme {
    /// 默认 = kfmv4 紫-青暗色系（配方直译，值与 2026-08-31 v2 质感版
    /// termview BAR_* 常量逐项一致——考题 spec_theme_默认kfmv4配方 钉值）
    fn default() -> Self {
        Theme {
            bar: BarTheme {
                bg: 0x0011_1119,
                field_bg_l: 0x0018_1532,
                field_bg_r: 0x000D_2231,
                field_focus_bg_l: 0x0020_1C40,
                field_focus_bg_r: 0x0012_2B3E,
                text: 0x00E0_E0E0,
                placeholder: 0x0061_6165,
                border_l: 0x007C_3AED,
                border_r: 0x0003_ADD1,
                accent: 0x0000_D4FF,
                glow: 0x007C_3AED,
                send_tl: 0x007C_3AED,
                send_br: 0x0003_ADD1,
                send_tri: 0x00FF_FFFF,
            },
        }
    }
}
