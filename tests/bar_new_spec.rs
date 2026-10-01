//! bar_new_spec.rs — BAR-217 bar-new.sh 信箱自动 commit 闸源码守卫（B 档
//! 胶水配源码机械钉）：MAILBOX 与 DEFAULT_MAILBOX 必须同经 readlink -f
//! 归一再比字符串——2026-10-01 承影 lane 定罪：只归一 MAILBOX（60 行）
//! 后与未归一 DEFAULT 比恒假，「= 默认信箱才自动 commit」闸永不进，
//! 追踪信 untracked 漂流（0138/0143/0144 三封实证，report 线已手补）。
//! 变异留档：摘 DEFAULT 归一行 → 本钉红（cp 备份复原复跑绿）。

/// DEFAULT_MAILBOX 必须同归一（闸的比较两边同一口径）
#[test]
fn spec_bar217_default_mailbox_同归一() {
    let s = include_str!("../scripts/bar-new.sh");
    assert!(
        s.contains("MAILBOX=\"$(readlink -f \"$MAILBOX\")\""),
        "MAILBOX 归一行不在（BAR-202 旧钉面）"
    );
    assert!(
        s.contains("DEFAULT_MAILBOX=\"$(readlink -f \"$DEFAULT_MAILBOX\")\""),
        "DEFAULT_MAILBOX 必须同归一——不归一 = 与归一后的 MAILBOX 比串恒假，\
         追踪信永不自动 commit（BAR-217 本体）"
    );
    // 归一必须先于比较（顺序反了等于没修）：取两行行号比大小
    let norm = s
        .lines()
        .position(|l| l.contains("DEFAULT_MAILBOX=\"$(readlink -f"))
        .expect("DEFAULT 归一行");
    let cmp = s
        .lines()
        .position(|l| l.contains("[ \"$MAILBOX\" = \"$DEFAULT_MAILBOX\" ]"))
        .expect("自动 commit 闸比较行");
    assert!(norm < cmp, "DEFAULT 归一必须排在自动 commit 闸比较之前");
}
