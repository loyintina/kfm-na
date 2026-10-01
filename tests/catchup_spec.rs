//! BAR-186 臂② 追赶状态机纯逻辑钉（A 档：进入/退出/跳底三考+速率触发
//! +窗滚动边界）。时钟全注入，零平台依赖。壳接线钉在
//! catchup_wiring_spec.rs。

use kfm_na::catchup::{
    CatchAct, Catchup, EnterCause, QUIET_MS, RATE_BYTES, RATE_WINDOW_MS, THROTTLE_MS,
};

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

/// BAR-216 观测账①（进场沿记因）：显式/速率进场各记各的因，且只记
/// 稳态→追赶边沿——追赶期重复 enter（5s 对账单档连发）不双边不双计
#[test]
fn spec_bar216_观测账_进场沿记因() {
    let mut c = Catchup::new();
    // 显式进场沿
    c.enter(1_000);
    assert_eq!(
        c.take_enter(),
        Some(EnterCause::Explicit),
        "显式进场必须产沿"
    );
    assert_eq!(c.take_enter(), None, "沿一记一取——重复取 = 壳重复报账");
    let st = c.stats();
    assert_eq!((st.enter_count, st.rate_enter_count), (1, 0));
    assert_eq!(st.catching_since_ms, 1_000);
    // 追赶期重复显式 enter（对账连档）：不产沿、不计数、账不清零
    assert!(c.note_bytes(1_050, 777));
    c.enter(1_100);
    assert_eq!(c.take_enter(), None, "追赶期重复 enter 不许产新沿");
    let st = c.stats();
    assert_eq!(st.enter_count, 1, "重复 enter 不双计（幂等续窗）");
    assert_eq!(st.held_bytes, 777, "续窗不许清压制账");
    // 落地后再进场 = 新沿
    assert_eq!(c.tick(1_100 + QUIET_MS), CatchAct::Land);
    c.enter(5_000);
    assert_eq!(c.take_enter(), Some(EnterCause::Explicit));
    assert_eq!(c.stats().enter_count, 2);
    assert_eq!(c.stats().held_bytes, 0, "新沿压制账归零重计");
}

/// BAR-216 观测账②（速率进场记因 + 压制量逐笔累计）：速率触发产
/// Rate 沿且本批即入账；held_bytes 是「回显无影案」的第一嫌疑账
#[test]
fn spec_bar216_观测账_速率进场与压制量() {
    let mut c = Catchup::new();
    assert!(!c.note_bytes(10_000, RATE_BYTES / 2 + 1));
    assert_eq!(c.take_enter(), None, "未到阈不产沿");
    assert!(
        c.note_bytes(10_050, RATE_BYTES / 2 + 1),
        "同窗超阈进场且本批即压帧"
    );
    assert_eq!(
        c.take_enter(),
        Some(EnterCause::Rate),
        "速率进场必须记 Rate 因"
    );
    let st = c.stats();
    assert_eq!((st.enter_count, st.rate_enter_count), (1, 1));
    assert_eq!(st.catching_since_ms, 10_050);
    assert_eq!(
        st.held_bytes,
        (RATE_BYTES / 2 + 1) as u64,
        "触发批自身必须计入压制量（本批也不画）"
    );
    // 续喂三笔连击键回显量级也照记——回显被压多久就压多少全留账
    assert!(c.note_bytes(10_060, 3));
    assert!(c.note_bytes(10_070, 5));
    assert_eq!(c.stats().held_bytes, (RATE_BYTES / 2 + 1 + 8) as u64);
    // 落地后账保留（壳取数报账窗口），落地计数 +1
    assert_eq!(c.tick(10_070 + QUIET_MS), CatchAct::Land);
    let st = c.stats();
    assert!(!st.catching);
    assert_eq!(st.land_count, 1);
    assert_eq!(
        st.held_bytes,
        (RATE_BYTES / 2 + 1 + 8) as u64,
        "落地沿账必须保留供报"
    );
}

/// BAR-216 观测账③（尾锚落地同记 + 稳态不产沿不记账）：稳态喂字节
/// 一笔都不许进压制账（即达即画的字节不是压制）
#[test]
fn spec_bar216_观测账_稳态零账() {
    let mut c = Catchup::new();
    assert!(!c.note_bytes(0, 64));
    assert!(!c.note_bytes(20, 64));
    assert_eq!(c.stats().held_bytes, 0, "稳态字节不许进压制账");
    assert_eq!(c.stats().enter_count, 0);
    // 尾锚落地：anchor 也记 land_count
    c.enter(1_000);
    assert!(c.note_bytes(1_010, 100));
    assert_eq!(c.anchor(), CatchAct::Land);
    assert_eq!(c.stats().land_count, 1);
    assert_eq!(c.stats().held_bytes, 100);
}

/// BAR-216 修复钉①（速率轮节流帧）：速率追赶期每 THROTTLE_MS 放一帧
/// ——洪峰压帧 ≠ 全冻，击键回显/流式进度有上屏路；节拍限流（到点前
/// 不许连放）且逐轮计数随落地报账；落地回稳态即闸死
#[test]
fn spec_bar216_节流帧_速率轮到点放帧() {
    let mut c = Catchup::new();
    c.note_bytes(10_000, RATE_BYTES); // 速率进场
    assert!(c.catching());
    assert!(
        !c.throttle_frame(10_000 + THROTTLE_MS - 1),
        "到点前不许放帧——连放 = 压帧合并形同虚设"
    );
    assert!(
        c.throttle_frame(10_000 + THROTTLE_MS),
        "速率轮到点必须放帧（回显上屏路）"
    );
    assert_eq!(c.stats().throttle_count, 1);
    assert!(
        !c.throttle_frame(10_000 + THROTTLE_MS + 1),
        "放帧后计时必须重置——立即再放 = 限流闸破"
    );
    assert!(c.throttle_frame(10_000 + THROTTLE_MS * 2));
    assert_eq!(c.stats().throttle_count, 2);
    // 落地回稳态：节拍闸随轮死
    assert_eq!(c.tick(10_000 + THROTTLE_MS * 2 + QUIET_MS), CatchAct::Land);
    assert!(!c.throttle_frame(10_000 + THROTTLE_MS * 2 + QUIET_MS + THROTTLE_MS));
    assert_eq!(c.stats().throttle_count, 2, "落地沿节拍账保留供报");
}

/// BAR-216 修复钉②（显式轮全压制）：重播种窗快照拼装期画中间态 =
/// 花屏——显式轮永不到点，放一帧都是回潮
#[test]
fn spec_bar216_节流帧_显式轮全压制() {
    let mut c = Catchup::new();
    c.enter(1_000);
    assert!(c.note_bytes(1_010, 4096));
    assert!(
        !c.throttle_frame(1_000 + THROTTLE_MS * 10),
        "显式轮永不许放节拍帧（快照拼装中间态不许上屏）"
    );
    assert_eq!(c.stats().throttle_count, 0);
}

/// BAR-216 修复钉③（速率轮显式升级）：洪峰追赶中重播种窗开启 =
/// 立即升级全压制（画拼装中间态 = 花屏）；升级前的节拍账随轮保留；
/// 落地后再速率进场 = 新轮，节流恢复且节拍账归零重计
#[test]
fn spec_bar216_节流帧_速率轮显式升级() {
    let mut c = Catchup::new();
    c.note_bytes(10_000, RATE_BYTES); // 速率轮
    assert!(c.throttle_frame(10_000 + THROTTLE_MS));
    assert_eq!(c.stats().throttle_count, 1);
    c.enter(20_000); // 重播种窗开 → 升级显式轮
    assert!(
        !c.throttle_frame(20_000 + THROTTLE_MS * 10),
        "升级显式轮后必须全压制——拼装中间态不许上屏"
    );
    assert_eq!(c.stats().throttle_count, 1, "升级不清账（本轮账随落地报）");
    assert_eq!(c.anchor(), CatchAct::Land);
    // 新速率轮：节流恢复、节拍账归零重计
    c.note_bytes(30_000, RATE_BYTES);
    assert_eq!(c.stats().throttle_count, 0, "新沿节拍账必须归零重计");
    assert!(c.throttle_frame(30_000 + THROTTLE_MS));
    assert_eq!(c.stats().throttle_count, 1);
}

/// BAR-216 观测账④（进场因报表词）：两因各守各的词——壳遥测行口径钉
/// （接线守卫咬字面，词漂移 = 两通路账对不上）
#[test]
fn spec_bar216_观测账_进场因报表词() {
    use kfm_na::catchup::enter_cause_name;
    assert_eq!(enter_cause_name(EnterCause::Explicit), "显式");
    assert_eq!(enter_cause_name(EnterCause::Rate), "速率洪峰");
}
