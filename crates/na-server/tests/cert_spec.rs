//! crates/na-server/tests/cert_spec.rs — BAR-200 缺证拒静默重生考题
//!
//! 定罪（2026-09-30 承影 0065）：搬家漏留 `$HOME/kfm-na-certs` 链接 →
//! 16:43 重启时 load_or_gen_* 静默自签新对 → 手机 pin 旧指纹全灭。
//! 契约：三件齐才许载；全缺须显式授权（NA_QUIC_GEN_KEYS=1）才准首跑生成；
//! 半缺永远拒启（静默补齐 = 新对覆盖幸存件，pin 照废）。

use na_server::cert::{IdentityVerdict, identity_verdict};

#[test]
fn spec_bar200_身份裁决真值表() {
    use IdentityVerdict::*;
    // 三件齐：授权与否都载（授权不改变既有身份的优先权）
    assert_eq!(identity_verdict(true, true, true, false), Load);
    assert_eq!(identity_verdict(true, true, true, true), Load);
    // 全缺：授权才准首跑生成，未授权拒启
    assert_eq!(identity_verdict(false, false, false, true), Gen);
    assert_eq!(identity_verdict(false, false, false, false), FailMissing);
    // 半缺六种形态：永远拒启——授权也不许静默补齐覆盖幸存件
    for (c, k, p) in [
        (true, true, false),
        (true, false, true),
        (false, true, true),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        assert_eq!(identity_verdict(c, k, p, false), FailPartial, "{c},{k},{p}");
        assert_eq!(
            identity_verdict(c, k, p, true),
            FailPartial,
            "{c},{k},{p} 授权也拒"
        );
    }
}
