//! plugins/term_alacritty.rs — alacritty 芯终端模拟器 provider（规格书 §3 第一批）
//!
//! 设计页：`/root/kfmv4/experiments/dsh-na/na/terminal-emulator.md`（v0，
//! 评审五条裁决落地）。契约考题：tests/term_emu_spec.rs（考题 4-8）。
//!
//! 职责边界：apply 只注册「终端模拟器工厂」服务（`dyn TermEmuFactory`），
//! 瞬时返回、**不建终端**（瞬时返回契约）。终端实例（含 scrollback 的
//! 长寿命 mutable 状态）归调用方持有——基座 registry 的 Arc 共享形态装不下
//! 每帧 &mut 渲染的独占对象，工厂形态是必然不是选择（评审裁决 1）。

use std::sync::Arc;

use crate::base::{Ctx, Plugin, ServiceKey};
use crate::termview::{self, AlacrittyEmuFactory, TermEmuFactory};

/// 插件名 = 启动配置表条目 id（v1 零配置，条目可省——设计页 §5）
pub const PLUGIN_NAME: &str = "term-alacritty";

pub struct TermAlacritty {
    /// 字体来源注入缝：生产 = 编译期内嵌（零探测，BAR-021），
    /// 考题 = 候选表探测夹具（host/Termux 双环境解析）
    fonts: termview::FactoryFonts,
}

impl TermAlacritty {
    /// 生产构造：编译期内嵌字体直载，启动零探测
    pub fn new() -> Self {
        TermAlacritty {
            fonts: termview::FactoryFonts::Vendored,
        }
    }

    /// 注入字体候选表（契约考题用夹具；host 无 /system/fonts）
    pub fn with_candidates(candidates: &'static [&'static str]) -> Self {
        TermAlacritty {
            fonts: termview::FactoryFonts::Probed(candidates),
        }
    }
}

impl Default for TermAlacritty {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for TermAlacritty {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::of::<dyn TermEmuFactory>()]
    }

    /// 无 inject（设计页 §3：渲染底座还是占位，帧缓冲归应用壳）
    fn apply(&self, ctx: &mut Ctx) -> Result<(), String> {
        let factory: Arc<dyn TermEmuFactory> = Arc::new(match &self.fonts {
            termview::FactoryFonts::Vendored => AlacrittyEmuFactory::vendored(),
            termview::FactoryFonts::Probed(c) => AlacrittyEmuFactory::new(c),
        });
        // 单一来源纪律：同名二次 provide = AlreadyProvided → apply Err →
        // 钉死 Failed，先到者服务不变（考题 8）
        let undo = ctx
            .provide::<dyn TermEmuFactory>(factory)
            .map_err(|e| format!("注册终端工厂失败: {e:?}"))?;
        ctx.effect(undo);
        Ok(())
    }
}
