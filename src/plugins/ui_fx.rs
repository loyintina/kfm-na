//! plugins/ui_fx.rs — ui-fx 动画插件（ui-base.md §五：动画全是插件，
//! 占槽驱动插值，拔掉后功能等价只是变糙）。
//!
//! 两道缝两件曲线（2026-09-04 用户拍板分档）：
//! - 「AI 面板 Y 偏移」缝 = 定时缓动（src/ui/fx_ease.rs）：落下 500ms
//!   ease-out / 收起 400ms ease-in（CSS transition 语言）。
//! - 「键盘 inset」缝 = 弹簧平滑（src/ui/fx_spring.rs）：100ms 轮询
//!   轨迹是阶梯，纯镜像太硬。
//!
//! 无 provides——占的是缝不是服务；disabled 一键关 =
//! 不占槽 = 全局硬切（na-regress 禁用 ui-fx 全卷绿的兑现路径）。

use crate::base::{Ctx, Plugin};

/// 插件名 = 启动配置表条目 id（disabled 一键关按它寻址）
pub const PLUGIN_NAME: &str = "ui-fx";

pub struct UiFx;

impl UiFx {
    pub fn new() -> Self {
        UiFx
    }
}

impl Default for UiFx {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for UiFx {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    /// 占缝采样，瞬时返回；卸载逆元 = 拔槽回硬切
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        // AI 面板缝 = 定时缓动（2026-09-04 拍板：落 500ms ease-out /
        // 收 400ms ease-in，取代弹簧墩感）
        crate::ui::seam::occupy_ai_panel_offset_y(crate::ui::fx_ease::ease_occupier());
        // 第二道缝（2026-09-04）：键盘 inset chrome 跟随——输入栏/快捷键行
        // 跟键盘开合的弹簧平滑（轮询轨迹是阶梯，纯镜像太硬，用户拍板）
        crate::ui::seam::occupy_chrome_ime_inset(crate::ui::fx_spring::spring_occupier());
        ctx.effect(Box::new(|| {
            crate::ui::seam::release_ai_panel_offset_y();
            crate::ui::seam::release_chrome_ime_inset();
        }));
        Ok(())
    }
}
