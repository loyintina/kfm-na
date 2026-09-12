//! panel_drag_spec.rs — 面板跟手拖拽考题（2026-09-11 用户拍板手势升级）。
//!
//! 模式（业界通称 interactive gesture-driven transition / 跟手拖拽）：
//! 拖拽期面板位置绑手指位移（不绑时间），松手瞬间按「进度 + 甩速」裁决
//! 完成或取消，收尾从当前位置重定基续播（缝 replay 踢复用 BAR-079）。
//! 对标：iOS interactive dismissal / 安卓抽屉与 BottomSheet。
//!
//! 契约要点：
//! - 方向锁：横向 ≥24px 且 |dx|>1.8|dy| 才接管（比 SWIPE_MIN_PX 90 小——
//!   拖拽要尽早接管；纵向滚屏/AI 页滚行不冲突）
//! - 角色（2026-09-12 四公民·三缘语义）：左滑+顶非右缘家 = 召唤解析页
//!   拖拽；右滑+顶是右缘家（配置/解析）= 推回拖拽；右滑+顶非左缘家 =
//!   召唤文件树拖拽；左滑+顶是文件树 = 推回拖拽；本家在顶反向滑 = 不锁。
//!   **设置页永不走手势召唤**（SummonConfig 变体已删除——齿轮钮唯一
//!   召唤口，右滑唯一关闭口）
//! - 跟手映射零跳变：锁定瞬间偏移 = 角色基位（锁定阈值位移被吃掉），
//!   其后偏移 = 基位 + 手指相对锁点位移 ×DRAG_GAIN(2.0，手指不从屏缘
//!   起手的补偿，2026-09-11 拍板)，钳 [0, 屏宽]
//! - 松手裁决：甩速优先（朝完成 ≥0.8px/ms 完成；反甩 ≤-0.8 取消），
//!   静止松手看进度（≥50% 完成）
//! - 速度窗 100ms；窗内不足两样本 = 速度 0（退进度判）

use kfm_na::ui::panel_drag::{
    DRAG_GAIN, DRAG_LOCK_PX, DragRole, DragTop, FLING_PX_PER_MS, PanelDrag, ReleaseDecision,
};

/// 起手式：顶非抽屉面板（Other = 终端裸奔或 AI 在顶）的拖拽会话（召唤候选）
fn summon_session() -> PanelDrag {
    PanelDrag::new(1000.0, 900.0, 10_000)
}

// 钉①：方向锁——横向 24px 起锁；斜率不过 1.8 不锁（纵向滚屏让路）；
// 锁定前 on_move 不回拉面板（None）。
// 变异抽检：DRAG_LOCK_PX 调小，23px 臂必红；斜率 1.8 改小，斜滑臂必红。
#[test]
fn spec_拖拽_方向锁() {
    let mut d = summon_session();
    // 未过阈值：23px 纯横移不锁
    assert!(
        d.on_move(977.0, 900.0, 10_010, 1200.0, DragTop::Other)
            .is_none()
    );
    // 斜滑（斜率 1.0 < 1.8）：位移再大也不锁——纵向手势让路
    assert!(
        d.on_move(900.0, 800.0, 10_020, 1200.0, DragTop::Other)
            .is_none()
    );
    // 过阈值且方向锁：24px 纯横移锁定位移
    assert!(
        d.on_move(976.0, 900.0, 10_030, 1200.0, DragTop::Other)
            .is_some()
    );
}

// 钉②：角色仲裁八臂全表（2026-09-12 四公民·三缘语义重钉，4 顶×2 向）——
// 左滑：顶 FileTree = DismissFileTree / 顶 Config = 不锁 / 顶 Parser =
// 不锁 / 顶 Other = SummonParser（设置页永不走手势召唤：SummonConfig 已删，
// 原位是 SummonParser——右缘家，偏移/甩速/裁决符号语义与旧配置系完全
// 一致）；右滑：顶 Config = DismissConfig（设置页唯一关闭路径）/ 顶
// Parser = DismissParser / 顶 FileTree = 不锁 / 顶 Other = SummonFileTree。
// 变异抽检：角色判反（左滑给 Dismiss）/DragTop 映射错一家/Other+左给错
// 公民，对应臂必红。
#[test]
fn spec_拖拽_角色仲裁八臂全表() {
    // 左滑，顶非抽屉面板（终端裸奔或 AI 在顶）→ 召唤解析页
    let mut d = PanelDrag::new(1000.0, 900.0, 0);
    match d.on_move(960.0, 900.0, 10, 1200.0, DragTop::Other) {
        Some((DragRole::SummonParser, _)) => {}
        other => panic!("左滑+顶Other应为召唤解析页锁定，得 {other:?}"),
    }
    // 左滑，顶是文件树 → 推回文件树
    let mut d = PanelDrag::new(600.0, 900.0, 0);
    match d.on_move(560.0, 900.0, 10, 1200.0, DragTop::FileTree) {
        Some((DragRole::DismissFileTree, _)) => {}
        other => panic!("左滑+顶是文件树应为推回锁定，得 {other:?}"),
    }
    // 左滑，顶已是配置 → 不锁（右缘本家在顶）
    let mut d = PanelDrag::new(1000.0, 900.0, 0);
    assert!(
        d.on_move(960.0, 900.0, 10, 1200.0, DragTop::Config)
            .is_none()
    );
    // 左滑，顶已是解析页 → 不锁（本家在顶）
    let mut d = PanelDrag::new(1000.0, 900.0, 0);
    assert!(
        d.on_move(960.0, 900.0, 10, 1200.0, DragTop::Parser)
            .is_none()
    );
    // 右滑，顶是配置 → 推回配置（设置页唯一关闭路径）
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    match d.on_move(340.0, 900.0, 10, 1200.0, DragTop::Config) {
        Some((DragRole::DismissConfig, _)) => {}
        other => panic!("右滑+顶是配置应为推回锁定，得 {other:?}"),
    }
    // 右滑，顶是解析页 → 推回解析页
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    match d.on_move(340.0, 900.0, 10, 1200.0, DragTop::Parser) {
        Some((DragRole::DismissParser, _)) => {}
        other => panic!("右滑+顶是解析页应为推回锁定，得 {other:?}"),
    }
    // 右滑，顶非抽屉面板 → 召唤文件树（左缘家）
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    match d.on_move(340.0, 900.0, 10, 1200.0, DragTop::Other) {
        Some((DragRole::SummonFileTree, _)) => {}
        other => panic!("右滑+顶Other应为召唤文件树锁定，得 {other:?}"),
    }
    // 右滑，顶已是文件树 → 不锁
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    assert!(
        d.on_move(340.0, 900.0, 10, 1200.0, DragTop::FileTree)
            .is_none()
    );
}

// 钉③：跟手映射零跳变 + 钳制（载具 = SummonParser，2026-09-12 三缘语义
// 换载具——原 SummonConfig 已删；右缘家符号语义完全一致）。召唤：锁定瞬
// 偏移≈屏宽（屏外右），手指继续左移偏移等幅减小（面板跟进），到 0 钳死
// 不许负（面板不许越过靠泊位左缘飞出）；推回：0 起右移等幅增大，到屏宽
// 钳死。
// 变异抽检：clamp 摘掉，越界臂必红；映射符号反，跟进方向臂必红。
#[test]
fn spec_拖拽_跟手映射与钳制() {
    let w = 1200.0;
    // 召唤解析页：down@1000，锁点应在 1000-24=976，锁定瞬偏移=屏宽（还没跟手位移）
    let mut d = summon_session();
    let (role, off0) = d.on_move(970.0, 900.0, 10_030, w, DragTop::Other).unwrap();
    assert_eq!(role, DragRole::SummonParser, "载具必须是召唤解析页");
    assert!(
        (off0 - (w - DRAG_LOCK_PX as f32)).abs() < 40.0,
        "锁定瞬偏移应≈屏外位（阈值位移被吃掉防跳变），得 {off0}"
    );
    // 继续左移 200px → 偏移减 400（×2 增益）
    let off1 = d
        .on_move(770.0, 900.0, 10_060, w, DragTop::Other)
        .map(|(_, o)| o)
        .unwrap_or(off0);
    assert!(
        (off0 - off1 - 200.0 * DRAG_GAIN as f32).abs() < 1.0,
        "手指左移 200 偏移应减 400（增益×2）：{off0}→{off1}"
    );
    // 左移过界（锁点 - 屏宽之外，越过靠泊位）→ 钳 0
    let off2 = d
        .on_move(-300.0, 900.0, 10_090, w, DragTop::Other)
        .map(|(_, o)| o)
        .unwrap_or(off1);
    assert_eq!(off2, 0.0, "越过靠泊位必须钳 0，得 {off2}");
    // 推回：锁定瞬 ≈0，右移等幅增，越界钳 w
    let mut d = PanelDrag::new(300.0, 900.0, 20_000);
    let (_, off3) = d.on_move(330.0, 900.0, 20_030, w, DragTop::Config).unwrap();
    assert!(off3.abs() < 40.0, "推回锁定瞬偏移应≈0，得 {off3}");
    let off4 = d
        .on_move(300.0 + 24.0 + 500.0, 900.0, 20_060, w, DragTop::Config)
        .map(|(_, o)| o)
        .unwrap_or(off3);
    assert!(
        (off4 - 500.0 * DRAG_GAIN as f32).abs() < 1.0,
        "推回跟手：锁点后右移 500 偏移应 1000（增益×2），得 {off4}"
    );
    let off5 = d
        .on_move(300.0 + 24.0 + 5000.0, 900.0, 20_090, w, DragTop::Config)
        .map(|(_, o)| o)
        .unwrap_or(off4);
    assert_eq!(off5, w, "推回越过屏宽必须钳 w，得 {off5}");
}

// 钉④：松手裁决表——静止松手看进度（≥50% 完成 / <50% 取消）；
// 甩速优先：朝完成方向 ≥FLING 完成（哪怕进度 30%），反甩 ≤-FLING
// 取消（哪怕进度 70%）；速度窗内不足两样本 = 速度 0 退进度判。
// 变异抽检：裁决阈值改反/甩速符号反，对应臂必红。
#[test]
fn spec_拖拽_松手裁决表() {
    let w = 1200.0;
    // 进度 60% 静止松手 → 完成（×2 增益下手指只需拖 0.3w）
    let mut d = summon_session();
    d.on_move(970.0, 900.0, 10_030, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.6 * 1200.0 / DRAG_GAIN,
        900.0,
        10_200,
        w,
        DragTop::Other,
    );
    // 末段静止：100ms 窗内只有同位样本（速度≈0）
    assert_eq!(
        d.on_release(10_400, w),
        ReleaseDecision::Complete,
        "进度 60% 静止松手应完成"
    );
    // 进度 30% 静止松手 → 取消
    let mut d = summon_session();
    d.on_move(970.0, 900.0, 20_030, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.3 * 1200.0 / DRAG_GAIN,
        900.0,
        20_200,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(20_400, w),
        ReleaseDecision::Cancel,
        "进度 30% 静止松手应取消"
    );
    // 进度 30% 但末段猛甩（100ms 内左移 200px = 2px/ms ≥ 0.8）→ 完成
    let mut d = summon_session();
    d.on_move(970.0, 900.0, 30_030, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.3 * 1200.0 / DRAG_GAIN + 200.0,
        900.0,
        30_100,
        w,
        DragTop::Other,
    );
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.3 * 1200.0 / DRAG_GAIN,
        900.0,
        30_180,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(30_180, w),
        ReleaseDecision::Complete,
        "进度 30% 猛甩（{}px/ms 窗内实测超阈）应完成",
        FLING_PX_PER_MS
    );
    // 进度 70% 但反甩（100ms 内右移 200px）→ 取消
    let mut d = summon_session();
    d.on_move(970.0, 900.0, 40_030, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.7 * 1200.0 / DRAG_GAIN - 200.0,
        900.0,
        40_100,
        w,
        DragTop::Other,
    );
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.7 * 1200.0 / DRAG_GAIN,
        900.0,
        40_180,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(40_180, w),
        ReleaseDecision::Cancel,
        "进度 70% 反甩应取消"
    );
    // 猛甩后停下 200ms 再松手：窗外甩速样本滤掉、速度归零退进度判 →
    // 进度 30% 取消（旧实现按推送时刻剪窗，松手时仍读旧甩速误判完成
    // ——clippy unused 参数顺手钓出的陈样本 bug，此题钉住新语义）
    let mut d = summon_session();
    d.on_move(970.0, 900.0, 60_030, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.3 * 1200.0 / DRAG_GAIN,
        900.0,
        60_100,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(60_300, w),
        ReleaseDecision::Cancel,
        "猛甩后停 200ms 再松手：窗外甩速不计，进度 30% 应取消"
    );
    // 推回对称：进度=推出比例。推 60% 静止松手 → 完成（推出）
    let mut d = PanelDrag::new(300.0, 900.0, 50_000);
    d.on_move(330.0, 900.0, 50_030, w, DragTop::Config);
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.6 * 1200.0 / DRAG_GAIN,
        900.0,
        50_200,
        w,
        DragTop::Config,
    );
    assert_eq!(
        d.on_release(50_400, w),
        ReleaseDecision::Complete,
        "推回 60% 静止松手应完成推出"
    );
}

// 钉⑤：反悔回拉——拖拽中反向滑回起点，进度归零，静止松手必取消
// （用户点名场景：「左滑没离开屏幕又右滑，页面不会出场而是回去」）。
#[test]
fn spec_拖拽_反悔回拉必取消() {
    let w = 1200.0;
    let mut d = summon_session();
    d.on_move(970.0, 900.0, 10_030, w, DragTop::Other);
    // 拉到 50% 开
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.5 * 1200.0 / DRAG_GAIN,
        900.0,
        10_100,
        w,
        DragTop::Other,
    );
    // 反悔：原路滑回起点
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.2 * 1200.0 / DRAG_GAIN,
        900.0,
        10_200,
        w,
        DragTop::Other,
    );
    d.on_move(1000.0 - DRAG_LOCK_PX, 900.0, 10_300, w, DragTop::Other);
    // 回拉速度 0.2w/100ms 在反向上……但末 100ms 窗内位移 =
    // (锁点→锁点) ≈ 0，静止；进度≈0 → 必取消
    assert_eq!(
        d.on_release(10_460, w),
        ReleaseDecision::Cancel,
        "反悔回拉到起点静止松手必取消"
    );
}

// 钉⑥：文件树家镜像（2026-09-11 三公民）——召唤文件树：手指右移，偏移
// （距靠泊距离）从屏宽递减；推回文件树：手指左移，偏移从 0 递增；甩速
// 符号与配置家镜像（召唤 FT 右移=朝完成；推回 FT 左移=朝完成）。
// 变异抽检：SummonFileTree/DismissFileTree 的 map_offset 或 velocity_toward
// 符号反一个，本题对应臂必红。
#[test]
fn spec_拖拽_文件树家镜像() {
    let w = 1200.0;
    // 召唤 FT：down@300，锁点 300+24=324，锁定瞬偏移≈屏宽（屏外左）
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    let (role, off0) = d.on_move(340.0, 900.0, 10, w, DragTop::Other).unwrap();
    assert_eq!(role, DragRole::SummonFileTree);
    assert!(
        (off0 - (w - DRAG_LOCK_PX as f32)).abs() < 40.0,
        "召唤FT锁定瞬偏移应≈屏外位，得 {off0}"
    );
    // 继续右移 200px → 偏移减 400（×2 增益）
    let off1 = d
        .on_move(540.0, 900.0, 40, w, DragTop::Other)
        .map(|(_, o)| o)
        .unwrap_or(off0);
    assert!(
        (off0 - off1 - 200.0 * DRAG_GAIN as f32).abs() < 1.0,
        "手指右移 200 偏移应减 400（增益×2）：{off0}→{off1}"
    );
    // 甩速：召唤 FT 末段猛右甩 = 朝完成（100ms 窗内右移 200px = 2px/ms）
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    d.on_move(340.0, 900.0, 10, w, DragTop::Other);
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.3 * w as f64 / DRAG_GAIN - 200.0,
        900.0,
        50,
        w,
        DragTop::Other,
    );
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.3 * w as f64 / DRAG_GAIN,
        900.0,
        130,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(130, w),
        ReleaseDecision::Complete,
        "召唤FT进度30%但末段右甩应完成"
    );
    // 推回 FT：锁定瞬≈0，左移等幅增；末段左甩 = 朝完成
    let mut d = PanelDrag::new(600.0, 900.0, 0);
    let (role, off2) = d.on_move(560.0, 900.0, 10, w, DragTop::FileTree).unwrap();
    assert_eq!(role, DragRole::DismissFileTree);
    assert!(off2.abs() < 40.0, "推回FT锁定瞬偏移应≈0，得 {off2}");
    d.on_move(
        600.0 - DRAG_LOCK_PX - 0.3 * w as f64 / DRAG_GAIN + 200.0,
        900.0,
        50,
        w,
        DragTop::FileTree,
    );
    d.on_move(
        600.0 - DRAG_LOCK_PX - 0.3 * w as f64 / DRAG_GAIN,
        900.0,
        130,
        w,
        DragTop::FileTree,
    );
    assert_eq!(
        d.on_release(130, w),
        ReleaseDecision::Complete,
        "推回FT进度30%但末段左甩应完成"
    );
    // 反甩取消镜像：召唤 FT 进度 70% 但末段左甩（反甩）→ 取消
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    d.on_move(340.0, 900.0, 10, w, DragTop::Other);
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.7 * w as f64 / DRAG_GAIN + 200.0,
        900.0,
        50,
        w,
        DragTop::Other,
    );
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.7 * w as f64 / DRAG_GAIN,
        900.0,
        130,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(130, w),
        ReleaseDecision::Cancel,
        "召唤FT进度70%但反甩应取消"
    );
}

// 钉⑦：解析家右缘镜像（2026-09-12 四公民·三缘语义）——与旧配置家符号
// 语义完全一致：召唤解析页手指左移、偏移（距靠泊距离）从屏宽递减，末段
// 左甩 = 朝完成；推回解析页手指右移、偏移从 0 递增，末段右甩 = 朝完成、
// 反甩（左甩）= 取消。SummonParser/DismissParser 就是 SummonConfig/
// DismissConfig 符号语义的原位继承人（设置页退手势召唤后的右缘新家）。
// 变异抽检：SummonParser/DismissParser 的 map_offset 或 velocity_toward
// 符号反一个，本题对应臂必红。
#[test]
fn spec_拖拽_解析家右缘镜像() {
    let w = 1200.0;
    // 召唤 PT：down@1000，锁点 1000-24=976，锁定瞬偏移≈屏宽（屏外右）
    let mut d = PanelDrag::new(1000.0, 900.0, 0);
    let (role, off0) = d.on_move(960.0, 900.0, 10, w, DragTop::Other).unwrap();
    assert_eq!(role, DragRole::SummonParser);
    assert!(
        (off0 - (w - DRAG_LOCK_PX as f32)).abs() < 40.0,
        "召唤PT锁定瞬偏移应≈屏外位，得 {off0}"
    );
    // 继续左移 200px → 偏移减 400（×2 增益）
    let off1 = d
        .on_move(760.0, 900.0, 40, w, DragTop::Other)
        .map(|(_, o)| o)
        .unwrap_or(off0);
    assert!(
        (off0 - off1 - 200.0 * DRAG_GAIN as f32).abs() < 1.0,
        "手指左移 200 偏移应减 400（增益×2）：{off0}→{off1}"
    );
    // 甩速：召唤 PT 末段猛左甩 = 朝完成（100ms 窗内左移 200px = 2px/ms）
    let mut d = PanelDrag::new(1000.0, 900.0, 0);
    d.on_move(960.0, 900.0, 10, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.3 * w as f64 / DRAG_GAIN + 200.0,
        900.0,
        50,
        w,
        DragTop::Other,
    );
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.3 * w as f64 / DRAG_GAIN,
        900.0,
        130,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(130, w),
        ReleaseDecision::Complete,
        "召唤PT进度30%但末段左甩应完成"
    );
    // 推回 PT：顶=Parser 右滑锁定瞬≈0，右移等幅增；末段右甩 = 朝完成
    let mut d = PanelDrag::new(300.0, 900.0, 0);
    let (role, off2) = d.on_move(340.0, 900.0, 10, w, DragTop::Parser).unwrap();
    assert_eq!(role, DragRole::DismissParser);
    assert!(off2.abs() < 40.0, "推回PT锁定瞬偏移应≈0，得 {off2}");
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.3 * w as f64 / DRAG_GAIN - 200.0,
        900.0,
        50,
        w,
        DragTop::Parser,
    );
    d.on_move(
        300.0 + DRAG_LOCK_PX + 0.3 * w as f64 / DRAG_GAIN,
        900.0,
        130,
        w,
        DragTop::Parser,
    );
    assert_eq!(
        d.on_release(130, w),
        ReleaseDecision::Complete,
        "推回PT进度30%但末段右甩应完成"
    );
    // 反甩取消镜像：召唤 PT 进度 70% 但末段右甩（反甩）→ 取消
    let mut d = PanelDrag::new(1000.0, 900.0, 0);
    d.on_move(960.0, 900.0, 10, w, DragTop::Other);
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.7 * w as f64 / DRAG_GAIN - 200.0,
        900.0,
        50,
        w,
        DragTop::Other,
    );
    d.on_move(
        1000.0 - DRAG_LOCK_PX - 0.7 * w as f64 / DRAG_GAIN,
        900.0,
        130,
        w,
        DragTop::Other,
    );
    assert_eq!(
        d.on_release(130, w),
        ReleaseDecision::Cancel,
        "召唤PT进度70%但反甩应取消"
    );
}
