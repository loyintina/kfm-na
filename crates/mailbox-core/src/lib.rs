//! mailbox-core — 信箱工具链纯函数核（kfmv4 JS 三件套忠实移植）。
//!
//! 纪律：本 crate 零 IO——所有函数收输入返回结果/诊断，不碰文件系统、不取时间。
//! 行为基准 = kfmv4/scripts/check/{new-letter,check-letter-token,gen-agent-inbox}.mjs，
//! 格式面逐字节兼容（fp/token 行/台账行/索引行/README 两区段）。

pub mod floor;
pub mod header;
pub mod json;
pub mod name;
pub mod newletter;
pub mod projection;
pub mod roster;
pub mod status;
pub mod token;
pub mod verify;
