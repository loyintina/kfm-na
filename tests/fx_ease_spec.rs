//! fx_ease_spec.rs — AI 面板定时缓动考题（A 档纯逻辑，2026-09-04 用户
//! 拍板：下落 ease-out / 收起 ease-in；时长沿革 350/250 → 09-11 提速
//! 250/180；答案 src/ui/fx_ease.rs + src/plugins/ui_fx.rs 装配）
//!
//! 判卷维度：
//! - 端点精确：t=0 在起点、elapsed ≥ 时长贴死目标（帧时钟停表的判据）
//! - 时长分档：进场更长（EXIT_MS 处不许贴死）/ 离场更短（ENTER_MS 处
//!   早已贴死）——探测点全部从常量推导，改时长常量考题自动跟随
//! - 曲线形状：ease-out 前半程 >50%（开头快）/ ease-in 前半程 <50%
//!   （开头慢）——CSS transition 手感与弹簧墩感的分野
//! - 单调性：全程不向反方向走（无弹簧式过冲）
//! - 占槽行为：首采样直通不重放 / 目标变化重定基位置不跳变 / 收敛后
//!   活性探针 false（帧时钟停表）
//!
//! 变异抽检：时长对调（ENTER↔EXIT）咬时长题；曲线对调（ease-out↔
//! ease-in）咬形状题；贴死删除咬端点题。

use kfm_na::ui::fx_ease;

// ---- 裸曲线（端点 + 形状签名；覆盖矩阵入账） ----

#[test]
fn spec_ease_裸曲线端点与形状() {
    assert_eq!(fx_ease::ease_out_cubic(0.0), 0.0);
    assert_eq!(fx_ease::ease_out_cubic(1.0), 1.0);
    assert_eq!(fx_ease::ease_in_cubic(0.0), 0.0);
    assert_eq!(fx_ease::ease_in_cubic(1.0), 1.0);
    // 形状签名：ease-out 前半跑赢直线（开头快）；ease-in 反之
    assert!(fx_ease::ease_out_cubic(0.5) > 0.5);
    assert!(fx_ease::ease_in_cubic(0.5) < 0.5);
}

// ---- 曲线端点与时长分档（纯函数零墙钟） ----

#[test]
fn spec_ease_端点精确() {
    assert_eq!(
        fx_ease::panel_ease_pos(-2800.0, 0.0, 0),
        -2800.0,
        "t=0 起点"
    );
    assert_eq!(fx_ease::panel_ease_pos(0.0, 0.0, 0), 0.0, "零位移直出目标");
    assert_eq!(
        fx_ease::panel_ease_pos(-2800.0, 0.0, fx_ease::ENTER_MS),
        0.0,
        "进场满 500ms 必须贴死目标"
    );
    assert_eq!(
        fx_ease::panel_ease_pos(0.0, -2800.0, fx_ease::EXIT_MS),
        -2800.0,
        "离场满 400ms 必须贴死目标"
    );
    assert_eq!(
        fx_ease::panel_ease_pos(-2800.0, 0.0, 99_999),
        0.0,
        "超时兜底贴死（不许渐近空烧帧）"
    );
}

#[test]
fn spec_ease_时长分档() {
    // 两档交界处：EXIT_MS 处进场必须还在路上（ENTER 更长）、ENTER_MS 处
    // 离场早已贴死（变异：时长对调会在这里露馅）
    let mid = fx_ease::panel_ease_pos(-2800.0, 0.0, fx_ease::EXIT_MS);
    assert!(
        mid < 0.0,
        "EXIT_MS 处进场必须还在路上（ENTER 更长档），得 {mid}"
    );
    assert_eq!(
        fx_ease::panel_ease_pos(0.0, -2800.0, fx_ease::ENTER_MS),
        -2800.0,
        "ENTER_MS 处离场必须早已贴死（EXIT 更短档）"
    );
    // 离场贴死前一刻必须还在路上（不许提前收表）
    let almost = fx_ease::panel_ease_pos(0.0, -2800.0, fx_ease::EXIT_MS - 10);
    assert!(
        almost > -2800.0,
        "离场末段前 10ms 必须还在路上，得 {almost}"
    );
}

#[test]
fn spec_ease_曲线形状() {
    // power2.out 落下（进场）：起步即快——半程时刻进度必须 >50%
    // （2026-09-11 定稿换曲线：重力 t² 起步亚像素蠕动+alpha 双加速
    // 被用户逐帧实锤判「冻结→跳变」，对齐 nz 的 GSAP power2.out）
    let half = fx_ease::panel_ease_pos(-2800.0, 0.0, fx_ease::ENTER_MS / 2);
    assert!(
        half >= -1400.0,
        "power2.out 落下半程必须已过半（起步即快），得 {half}"
    );
    // 镜像收起（离场）：起步快——半程时刻进度必须 >50%（减速上升）
    let half_up = fx_ease::panel_ease_pos(0.0, -2800.0, fx_ease::EXIT_MS / 2);
    assert!(
        half_up < -1400.0,
        "收起起步快减速走——半程时刻必须已过半，得 {half_up}"
    );
}

#[test]
fn spec_ease_单调无过冲() {
    // 定时缓动不过冲——全程单调向目标（弹簧的墩感已被拍板换下）
    let mut prev = -2800.0_f32;
    for t in (0..=fx_ease::ENTER_MS).step_by(16) {
        let pos = fx_ease::panel_ease_pos(-2800.0, 0.0, t);
        assert!(
            pos >= prev - f32::EPSILON,
            "进场 t={t} 不许回头：{prev}→{pos}"
        );
        assert!(pos <= 0.0, "进场 t={t} 不许过冲越过目标：{pos}");
        prev = pos;
    }
    let mut prev = 0.0_f32;
    for t in (0..=fx_ease::EXIT_MS).step_by(16) {
        let pos = fx_ease::panel_ease_pos(0.0, -2800.0, t);
        assert!(
            pos <= prev + f32::EPSILON,
            "离场 t={t} 不许回头：{prev}→{pos}"
        );
        assert!(pos >= -2800.0, "离场 t={t} 不许过冲越过目标：{pos}");
        prev = pos;
    }
}

// ---- 占槽行为（采样器 + 活性探针） ----

#[test]
fn spec_ease_占槽首采样直通() {
    let o = fx_ease::ease_occupier();
    // 首采样直通目标值：冷启动/插件热装不补演一场历史
    assert_eq!((o.sampler)(0.0, 1000), 0.0);
    assert!(!(o.is_active)(), "直通即稳态，不许起表");
}

#[test]
fn spec_ease_占槽重定基不跳变() {
    let o = fx_ease::ease_occupier();
    assert_eq!((o.sampler)(-2800.0, 0), -2800.0); // 首采样直通（屏外稳态）
    (o.sampler)(0.0, 100); // 目标改 0 = 开始落下
    let mid_ms = 100 + fx_ease::ENTER_MS / 2;
    let mid = (o.sampler)(0.0, mid_ms); // 半程处，路上某点
    assert!(mid > -2800.0 && mid < 0.0, "中途必须在路上，得 {mid}");
    // 半路反目标（点光球收起）：重定基从当前值续走——下一采样位置
    // 必须紧邻 mid（位置不跳变），方向掉头向 -2800
    let back1 = (o.sampler)(-2800.0, mid_ms + 16);
    assert!(
        (back1 - mid).abs() < 400.0,
        "重定基位置必须连续（mid={mid} 下一步 {back1}）"
    );
    assert!(back1 < 0.0, "掉头后必须向屏外走，得 {back1}");
    assert!((o.is_active)(), "动画途中活性探针必须 true");
}

#[test]
fn spec_ease_占槽收敛停表() {
    let o = fx_ease::ease_occupier();
    (o.sampler)(-2800.0, 0);
    (o.sampler)(0.0, 100);
    let end = (o.sampler)(0.0, 100 + fx_ease::ENTER_MS);
    assert_eq!(end, 0.0, "满时长贴死");
    assert!(!(o.is_active)(), "收敛后停表（夜判据红线：零额外帧）");
}

// ---- 入场重播踢（BAR-079：面板栈坍缩②「覆盖再召唤=重播入场」的缝侧原语） ----

#[test]
fn spec_bar079_入场重播踢_从屏外位起播() {
    let o = fx_ease::ease_occupier();
    let kick = o.replay.as_ref().expect("面板缝必须带重播踢");
    assert_eq!((o.sampler)(0.0, 0), 0.0); // 首采直通靠泊（被覆盖稳态）
    assert!(!(o.is_active)());
    kick(-2800.0, 1000); // 覆盖再召唤：坍缩为屏外收起（变异：不重置 start_ms 咬此）
    assert!(
        (o.is_active)(),
        "踢后采样前活性探针必须已 true——帧时钟即刻起表（变异：不置 settled=false 咬此）"
    );
    let mid = (o.sampler)(0.0, 1000 + 100); // 目标没变，但必须从屏外位起播
    assert_eq!(
        mid,
        fx_ease::panel_ease_pos(-2800.0, 0.0, 100),
        "踢后采样 = 从屏外位重播入场（变异：不设 from 咬此）"
    );
    assert!(mid < -1000.0, "100ms 处必须还在落程前段，得 {mid}");
    assert!(
        (o.is_active)(),
        "重播期间活性探针必须 true——帧时钟起表（变异：不置 settled=false 咬此）"
    );
    let end = (o.sampler)(0.0, 1000 + fx_ease::ENTER_MS);
    assert_eq!(end, 0.0, "重播满时长贴死靠泊");
    assert!(!(o.is_active)(), "重播收敛停表");
}

#[test]
fn spec_bar079_入场重播踢_目标即屏外位静默瞬移() {
    // 静默挤出（坍缩③）语义：被覆盖者离栈，目标=屏外位——踢后 from==target
    // 即刻落定，不许空烧一轮隐形动画帧
    let o = fx_ease::ease_occupier();
    let kick = o.replay.as_ref().unwrap();
    assert_eq!((o.sampler)(0.0, 0), 0.0);
    kick(-2800.0, 1000);
    let pos = (o.sampler)(-2800.0, 1000);
    assert_eq!(pos, -2800.0);
    assert!(
        !(o.is_active)(),
        "from==target 必须即刻停表（静默挤出零帧空烧）"
    );
}

#[test]
fn spec_bar079_重播踢_未首采忽略() {
    let o = fx_ease::ease_occupier();
    let kick = o.replay.as_ref().unwrap();
    kick(-2800.0, 500); // 冷启动未首采：踢必须忽略
    assert_eq!((o.sampler)(0.0, 1000), 0.0, "首采样仍直通（不补演历史）");
    assert!(!(o.is_active)());
}

#[test]
fn spec_bar079_缝重播踢_中继三态() {
    // 缝层 replay_* 中继（覆盖矩阵入账）：①占槽带踢=必达且参数原样；
    // ②占槽无踢（弹簧件形态）=空操作不 panic；③拔槽=空操作
    use kfm_na::ui::seam;
    use std::sync::{Arc, Mutex};
    let got = Arc::new(Mutex::new(None));
    let got2 = Arc::clone(&got);
    seam::occupy_ai_panel_offset_y(seam::Occupier {
        sampler: Arc::new(|t, _| t),
        is_active: Arc::new(|| false),
        replay: Some(Arc::new(move |off: f32, now: u64| {
            *got2.lock().unwrap() = Some((off, now));
        })),
    });
    seam::replay_ai_panel_offset_y(-2800.0, 777);
    assert_eq!(
        *got.lock().unwrap(),
        Some((-2800.0, 777)),
        "踢必达占槽件，(屏外位, 时刻) 原样"
    );
    seam::occupy_ai_panel_offset_y(seam::Occupier {
        sampler: Arc::new(|t, _| t),
        is_active: Arc::new(|| false),
        replay: None, // 弹簧件形态：无入场概念
    });
    seam::replay_ai_panel_offset_y(-2800.0, 778); // 空操作不 panic 即过
    seam::release_ai_panel_offset_y();
    seam::replay_ai_panel_offset_y(-2800.0, 779); // 拔槽空操作
    seam::replay_config_panel_offset_x(1260.0, 100); // 无占槽空操作（入账）
}

// ---- 落下曲线族（2026-09-11 定稿：落下=power2.out 减速，nz 同款；
// 前身重力 t² 被逐帧实锤判「冻结→跳变」，退役） ----

#[test]
fn spec_ease_落下曲线_端点与签名() {
    assert_eq!(fx_ease::power2_out(0.0), 0.0);
    assert_eq!(fx_ease::power2_out(1.0), 1.0);
    assert_eq!(fx_ease::rise_release(0.0), 0.0);
    assert_eq!(fx_ease::rise_release(1.0), 1.0);
    // 减速签名：1-(1-t)² 在 10% 时间已走 19% 路程（起步即快，第一帧
    // 就有可见位移），90% 时间走 99%（缓停到位）
    let early = fx_ease::power2_out(0.1);
    assert!(
        (early - 0.19).abs() < 1e-4,
        "power2.out 起步必须已经在走，得 {early}"
    );
    let late = fx_ease::power2_out(0.9);
    assert!(
        (late - 0.99).abs() < 1e-4,
        "power2.out 90% 时间 = 99% 路程，得 {late}"
    );
    // 镜像：收起 10% 时间走 19%（起步快，减速上升）
    let r_early = fx_ease::rise_release(0.1);
    assert!(
        (r_early - 0.19).abs() < 1e-4,
        "收起起步必须已经在走，得 {r_early}"
    );
    // 单调扫（无回环无过冲）
    let mut prev = 0.0;
    for i in 1..=100 {
        let t = i as f32 / 100.0;
        let y = fx_ease::power2_out(t);
        assert!(y >= prev && y <= 1.0, "落下 t={t} 回环");
        prev = y;
    }
}

#[test]
fn spec_ease_面板曲线_半程判定() {
    // 落下的行为契约：半程必须「已过半」（起步即快）——
    // 与 09-06 重力 t² 的「半程未过半」相反，此题锁住曲线性格
    let half = fx_ease::panel_ease_pos(-2800.0, 0.0, fx_ease::ENTER_MS / 2);
    assert!(half >= -1400.0, "落下半程必须已过半，得 {half}");
    let half_up = fx_ease::panel_ease_pos(0.0, -2800.0, fx_ease::EXIT_MS / 2);
    assert!(
        half_up < -1400.0,
        "收起半程必须已过半（起步快），得 {half_up}"
    );
}

// ---- 滑动淡入（2026-09-10 用户拍板试方：alpha 从 placement 推导） ----

#[test]
fn spec_fade_端点与硬切等价() {
    // 靠泊 = 全实；屏外 = 全隐——硬切基座下 off 只取这两值，
    // alpha 恒 1/0，与硬切像素等价（无 fx 占槽不引入新行为）
    assert_eq!(fx_ease::panel_fade_alpha(0.0, 2800.0), 1.0, "靠泊全实");
    assert_eq!(fx_ease::panel_fade_alpha(-2800.0, 2800.0), 0.0, "屏外全隐");
    // 越界钳制：off 冲出 [-h, 0] 不许 NaN/负 alpha/超 1
    assert_eq!(
        fx_ease::panel_fade_alpha(-9999.0, 2800.0),
        0.0,
        "屏外越界钳 0"
    );
    assert_eq!(
        fx_ease::panel_fade_alpha(100.0, 2800.0),
        1.0,
        "靠泊越界钳 1"
    );
    // 病态尺寸直通全实（不许黑屏）
    assert_eq!(fx_ease::panel_fade_alpha(-100.0, 0.0), 1.0, "零高直通");
    assert_eq!(fx_ease::panel_fade_alpha(-100.0, -5.0), 1.0, "负高直通");
}

#[test]
fn spec_fade_淡入窗与单调() {
    let h = 2800.0;
    // 淡入窗内：落程 10%（off=-0.9h）→ alpha = 0.10/0.35 ≈ 0.286
    // （变异抽检：FADE_PORTION 改 1.0 得 0.10 必红；恒返 1 必红）
    let a = fx_ease::panel_fade_alpha(-0.9 * h, h);
    assert!((a - 0.10 / 0.35).abs() < 1e-4, "窗内线性显影，得 {a}");
    // 窗沿：落程恰好 FADE_PORTION → 恰全实
    let edge = fx_ease::panel_fade_alpha(-(1.0 - fx_ease::FADE_PORTION) * h, h);
    assert!((edge - 1.0).abs() < 1e-4, "窗沿恰全实，得 {edge}");
    // 窗外：落程过半早已全实（残余高速段满对比落地）
    assert_eq!(fx_ease::panel_fade_alpha(-0.4 * h, h), 1.0, "窗外全实");
    // 单调：从屏外到靠泊全程不降（显影不许回头）
    let mut prev = 0.0_f32;
    for i in 0..=100 {
        let off = -h * (1.0 - i as f32 / 100.0);
        let a = fx_ease::panel_fade_alpha(off, h);
        assert!(a >= prev, "off={off} 显影回头：{prev}→{a}");
        prev = a;
    }
}

#[test]
fn spec_fade_与落下曲线咬合() {
    // 配方语义：落下起步即显影，落地前全实（保留砸底手感）。
    // power2.out 下 t=0.1 落程已达 0.19 → alpha≈0.54（起步即显影，
    // 不许近乎全隐——这正是重力 t² 时代「冻结→跳变」的病灶反面）；
    // alpha 全实点 = 落程 FADE_PORTION=0.35 处即 t=1-√0.65≈0.194，
    // t=0.2（落程 0.36）必须已全实
    let h = 2800.0;
    let t_early = fx_ease::power2_out(0.1); // 落程 0.19
    let a_early = fx_ease::panel_fade_alpha(-h * (1.0 - t_early), h);
    assert!(a_early > 0.5, "起步必须即显影，得 {a_early}");
    let t_done = fx_ease::power2_out(0.2); // 落程 0.36 > FADE_PORTION
    let a_done = fx_ease::panel_fade_alpha(-h * (1.0 - t_done), h);
    assert!(a_done > 0.95, "20% 时长处必须近乎全实，得 {a_done}");
}
