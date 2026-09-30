//! BAR-186 臂② 追赶状态机纯逻辑钉（A 档：进入/退出/跳底三考+速率触发
//! +窗滚动边界）。时钟全注入，零平台依赖。壳接线钉在
//! catchup_wiring_spec.rs。

use kfm_na::catchup::{CatchAct, Catchup, QUIET_MS, RATE_BYTES, RATE_WINDOW_MS};

/// 稳态：低速字节永不进场，tick 永不发 Land——打字回显不许被误压帧
#[test]
fn spec_bar186_追赶_稳态不进场() {
    let mut c = Catchup::new();
    assert!(!c.catching(), "出厂即稳态");
    for i in 0..100 {
        let t = i * 20; // 每 20ms 一笔小字节（打字回显量级）
        assert!(
            !c.note_bytes(t, 64),
            "稳态小流不许进场（t={t}）——误进场 = 打字不即画"
        );
        assert_eq!(c.tick(t), CatchAct::None, "稳态 tick 永不 Land");
    }
}

/// 显式入场（重连）→ 滴灌期压帧 → 静默窗满 → Land 跳底，且只发一次
#[test]
fn spec_bar186_追赶_入场静默追平跳底一次() {
    let mut c = Catchup::new();
    c.enter(1_000);
    assert!(c.catching(), "enter 后必在追赶期");
    // 滴灌三笔，每笔都刷新静默计时
    assert!(c.note_bytes(1_050, 100), "追赶期 note_bytes 恒真（压帧）");
    assert!(c.note_bytes(1_100, 100));
    assert!(c.note_bytes(1_200, 100));
    assert_eq!(
        c.tick(1_200 + QUIET_MS - 1),
        CatchAct::None,
        "静默窗差 1ms 不满不许落地——早一帧都是慢滚复辟"
    );
    assert_eq!(
        c.tick(1_200 + QUIET_MS),
        CatchAct::Land,
        "静默窗满必须 Land（追平跳底亮出）"
    );
    assert!(!c.catching(), "落地后回稳态");
    assert_eq!(
        c.tick(1_200 + QUIET_MS + 10),
        CatchAct::None,
        "Land 只发一次——二次 Land = 无故跳底抢用户滚动条"
    );
    assert!(!c.note_bytes(2_000, 64), "落地后小流即回稳态即画");
}

/// 播种尾锚：追赶中撞锚立即 Land；稳态撞锚 = None（不抢稳态的画）
#[test]
fn spec_bar186_追赶_尾锚落地() {
    let mut c = Catchup::new();
    assert_eq!(c.anchor(), CatchAct::None, "稳态撞锚不许 Land");
    c.enter(5_000);
    assert!(c.note_bytes(5_010, 4096));
    assert_eq!(c.anchor(), CatchAct::Land, "追赶中播种尾锚必须立即落地");
    assert!(!c.catching());
    assert_eq!(c.anchor(), CatchAct::None, "锚只咬一次");
    // 尾锚落地后仍有尾滴 → 速率/显式入场才再进场，小流稳态即画
    assert!(!c.note_bytes(5_100, 64));
}

/// 速率触发：窗内字节洪峰自动进场（非重连的滴灌洪峰也罩住）；
/// 窗口滚动后洪峰记忆清零——跨窗累计不许误触发
#[test]
fn spec_bar186_追赶_速率触发与窗滚动() {
    let mut c = Catchup::new();
    // 窗内半阈×2 跨窗：每窗都不到阈，永不进场
    assert!(!c.note_bytes(0, RATE_BYTES / 2));
    assert!(!c.note_bytes(RATE_WINDOW_MS + 1, RATE_BYTES / 2));
    assert!(!c.catching(), "跨窗各半阈不许累计触发");
    // 同窗洪峰：两笔各超半阈 → 第二笔进场，且本批即压帧
    assert!(!c.note_bytes(10_000, RATE_BYTES / 2 + 1));
    assert!(
        c.note_bytes(10_050, RATE_BYTES / 2 + 1),
        "同窗超阈必须进场且本批即压帧（返回 true）"
    );
    assert!(c.catching());
    // 洪峰过境静默追平
    assert_eq!(c.tick(10_050 + QUIET_MS), CatchAct::Land);
}

/// 边界：enter 重置速率窗——重连前的洪峰记忆不许带进新窗口
#[test]
fn spec_bar186_追赶_入场重置窗账() {
    let mut c = Catchup::new();
    assert!(!c.note_bytes(0, RATE_BYTES - 1));
    c.enter(50); // 重连：窗账清零
    assert!(c.catching());
    // 新窗第一笔小字节：若旧账未清，1 字节即误触发（本就在场无妨，
    // 但静默计时必须重置——旧 last_byte 会让 tick 立即落地）
    assert!(c.note_bytes(60, 10));
    assert_eq!(
        c.tick(60 + QUIET_MS - 1),
        CatchAct::None,
        "enter 必须重置静默计时——否则重连即刻假追平"
    );
    assert_eq!(c.tick(60 + QUIET_MS), CatchAct::Land);
}
