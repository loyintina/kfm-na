//! cert.rs — QUIC 身份三件（服务器证书/私钥/客户端 psk）的缺件裁决（BAR-200）
//!
//! 病灶：旧 load_or_gen_* 缺件即静默重生——丢一个符号链接就换掉服务器身份，
//! 所有 pin 旧指纹的客户端永久失配（2026-09-30 事故：搬家漏留链接 → 16:43
//! 重启静默换新证 → 手机 QUIC 全灭、ssh 刷新风暴）。裁：缺件只有两条路——
//! 显式授权首跑生成（NA_QUIC_GEN_KEYS=1）或大声拒启；**半缺（事故形态）
//! 永远拒启**——静默补齐会用新对覆盖幸存件，pin 照废。

/// 身份裁决（A 档纯函数）：三件齐 → 载；全缺 + 授权 → 首跑生成；
/// 全缺未授权 → 拒启；半缺 → 拒启（不论授权）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityVerdict {
    Load,
    Gen,
    /// 全缺但未授权首跑生成
    FailMissing,
    /// 半缺 = 事故形态（搬家漏链/误删单件），授权也不许静默补齐
    FailPartial,
}

/// 三件在否 + 是否授权首跑生成 → 裁决。psk 长度非法按「不在」计（调用方口径）
pub fn identity_verdict(cert: bool, key: bool, psk: bool, gen_allowed: bool) -> IdentityVerdict {
    match (cert, key, psk) {
        (true, true, true) => IdentityVerdict::Load,
        (false, false, false) if gen_allowed => IdentityVerdict::Gen,
        (false, false, false) => IdentityVerdict::FailMissing,
        _ => IdentityVerdict::FailPartial,
    }
}
