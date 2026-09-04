//! lib.rs — kfm-na 库入口（集成测试与 Android cdylib 共享）

/// 插件基座 = cordis-na 通用运行时（2026-08-17 阶段 1 搬家：src/base/ →
/// crates/cordis-na)。harness re-export 保持 `kfm_na::base::` 路径可用,
/// 消费侧零 churn;新代码可直接 `use cordis_na::…`
pub use cordis_na as base;
pub mod ai_chat;
pub mod ai_presence;
pub mod bootstrap;
pub mod brain;
pub mod brain_ep;
pub mod conn;
pub mod crash;
pub mod direct_brain;
pub mod exec_probe;
pub mod gate;
pub mod glyph_atlas;
pub mod http1;
pub mod ime_queue;
pub mod input_bar;
pub mod insets;
pub mod keybar;
pub mod keymap;
pub mod local_pty;
pub mod plugins;
pub mod protocol;
pub mod providers;
pub mod report;
pub mod scroll;
pub mod session;
pub mod session_router;
pub mod termview;
pub mod theme;
pub mod trace;
pub mod ui;

#[cfg(target_os = "android")]
pub mod android_app;
#[cfg(target_os = "android")]
pub mod clipboard;
#[cfg(target_os = "android")]
pub mod gles_present;
#[cfg(target_os = "android")]
pub mod ime_bridge;
