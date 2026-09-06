//! fx_ease_spec.rs — AI 面板定时缓动考题（A 档纯逻辑，2026-09-04 用户
//! 拍板：下落 ease-out / 收起 ease-in，同日实测定档 350ms/250ms；答案
//! src/ui/fx_ease.rs + src/plugins/ui_fx.rs 装配）
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
    // 重力落下（进场）：起步静止——半程时刻进度必须 <50%（后段加速砸底）
    let half = fx_ease::panel_ease_pos(-2800.0, 0.0, fx_ease::ENTER_MS / 2);
    assert!(
        half < -1400.0,
        "重力落下半程必须未过半（后段加速），得 {half}"
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
    let mid = (o.sampler)(0.0, 350); // 250ms 处，路上某点
    assert!(mid > -2800.0 && mid < 0.0, "中途必须在路上，得 {mid}");
    // 半路反目标（点光球收起）：重定基从当前值续走——下一采样位置
    // 必须紧邻 mid（位置不跳变），方向掉头向 -2800
    let back1 = (o.sampler)(-2800.0, 366);
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

// ---- 物理重力族（2026-09-06 定稿：落下=自由落体 t²，收起=镜像） ----

#[test]
fn spec_ease_重力落下_端点与物理签名() {
    assert_eq!(fx_ease::gravity_fall(0.0), 0.0);
    assert_eq!(fx_ease::gravity_fall(1.0), 1.0);
    assert_eq!(fx_ease::rise_release(0.0), 0.0);
    assert_eq!(fx_ease::rise_release(1.0), 1.0);
    // 物理签名：t² 在 10% 时间只走 1% 路程（起步静止），
    // 90% 时间走 81%（加速砸底）——匀加速运动的精确解
    let early = fx_ease::gravity_fall(0.1);
    assert!(early < 0.05, "落下起步必须近乎静止，得 {early}");
    let late = fx_ease::gravity_fall(0.9);
    assert!(
        (late - 0.81).abs() < 1e-4,
        "t² 落体 90% 时间 = 81% 路程，得 {late}"
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
        let y = fx_ease::gravity_fall(t);
        assert!(y >= prev && y <= 1.0, "落下 t={t} 回环");
        prev = y;
    }
}

#[test]
fn spec_ease_面板重力曲线_半程判定() {
    // 换装后面板行为契约：落下半程必须「未过半」（重力前段慢）——
    // 与 09-05 emphasized 的「半程大幅过半」相反，此题锁住直觉方向
    let half = fx_ease::panel_ease_pos(-2800.0, 0.0, fx_ease::ENTER_MS / 2);
    assert!(half < -1400.0, "重力落下半程必须未过半，得 {half}");
    let half_up = fx_ease::panel_ease_pos(0.0, -2800.0, fx_ease::EXIT_MS / 2);
    assert!(
        half_up < -1400.0,
        "收起半程必须已过半（起步快），得 {half_up}"
    );
}
