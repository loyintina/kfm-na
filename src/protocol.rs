//! protocol.rs — re-export 壳（2026-09-20 na-server 立项上提）
//!
//! 协议单源已迁至 `crates/na-protocol`（na 壳与 na-server 双端共享）。
//! 本壳保持 `kfm_na::protocol::*` 消费路径不变；考题 tests/protocol_spec.rs
//! 一字未动，迁移零漂移以它全绿为证。

pub use na_protocol::*;
