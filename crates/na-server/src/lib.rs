//! lib.rs — na-server 库面：四模块对考题开放（main.rs 只是 tokio 入口壳）
//!
//! 设计 docs/active/na-server.md。只绑 127.0.0.1，鉴权 = SSH 本身。

pub mod httpd;
pub mod pty_sess;
pub mod state;
pub mod utf8x;
pub mod wsterm;
