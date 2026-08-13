//! lib.rs — kfm-na 库入口（集成测试与 Android cdylib 共享）

pub mod protocol;
pub mod report;

#[cfg(target_os = "android")]
pub mod android_app;
