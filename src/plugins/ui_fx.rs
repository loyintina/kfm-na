//! plugins/ui_fx.rs — ui-fx 动画插件（ui-base.md §五：动画全是插件，
//! 占槽驱动插值，拔掉后功能等价只是变糙）。
//!
//! v1 唯一的活：占「AI 面板 Y 偏移」缝（seam.rs），弹簧曲线驱动
//! 面板落下/升起（曲线与状态核在 src/ui/fx_spring.rs，本件只是
//! 装配工）。无 provides——占的是缝不是服务；disabled 一键关 =
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
        crate::ui::seam::occupy_ai_panel_offset_y(crate::ui::fx_spring::spring_occupier());
        ctx.effect(Box::new(|| {
            crate::ui::seam::release_ai_panel_offset_y();
        }));
        Ok(())
    }
}
