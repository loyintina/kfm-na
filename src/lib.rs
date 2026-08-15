//! lib.rs — kfm-na 库入口（集成测试与 Android cdylib 共享）

pub mod base;
pub mod conn;
pub mod ime_queue;
pub mod keybar;
pub mod keymap;
pub mod protocol;
pub mod report;
pub mod scroll;
pub mod session;
pub mod termview;

#[cfg(target_os = "android")]
pub mod android_app;
#[cfg(target_os = "android")]
pub mod ime_bridge;
#[cfg(target_os = "android")]
pub mod insets;
