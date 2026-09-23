//! na-quic — QUIC 隧道核心（设计：docs/active/quic隧道.md）
//!
//! 桥接模型：本机 TCP 监听器 → QUIC bidirectional stream → 对端回联 TCP。
//! M1 spike：先验证 quinn 双端可编 + echo 双通，迁移考题见 M2（netns）。
