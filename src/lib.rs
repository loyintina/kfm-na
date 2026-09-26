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
pub mod ctrl_feed;
pub mod direct_brain;
pub mod endpoint;
pub mod exec_probe;
pub mod gate;
pub mod glyph_atlas;
pub mod http1;
pub mod ime_queue;
pub mod input_bar;
pub mod insets;
pub mod install;
pub mod keybar;
pub mod keymap;
pub mod local_pty;
pub mod na_server_sup;
pub mod offline_keys;
pub mod plugins;
pub mod protocol;
pub mod providers;
pub mod report;
pub mod scroll;
pub mod self_restart;
pub mod sess_mode;
pub mod sess_pool;
pub mod session;
pub mod session_router;
pub mod settings;
pub mod singleton;
pub mod svc_health;
pub mod sys_hist;
pub mod termview;
pub mod theme;
pub mod tmux_ctl;
pub mod tmux_exec;
pub mod trace;
pub mod tunnel;
pub mod ui;
pub mod vsync_book;
pub mod wire_render;

#[cfg(target_os = "android")]
pub mod android_app;
#[cfg(target_os = "android")]
pub mod clipboard;
#[cfg(target_os = "android")]
pub mod gles_present;
#[cfg(target_os = "android")]
pub mod ime_bridge;
