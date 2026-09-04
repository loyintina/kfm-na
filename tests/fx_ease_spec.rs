//! fx_ease_spec.rs — AI 面板定时缓动考题（A 档纯逻辑，2026-09-04 用户
//! 拍板：下落 500ms ease-out / 收起 400ms ease-in；答案
//! src/ui/fx_ease.rs + src/plugins/ui_fx.rs 装配）
//!
//! 判卷维度：
//! - 端点精确：t=0 在起点、elapsed ≥ 时长贴死目标（帧时钟停表的判据）
//! - 时长分档：进场 500ms（400ms 时不许提前贴死）/ 离场 400ms
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
    // 进场 500ms：400ms 处不许贴死（变异：时长对调会在这里露馅）
    let mid = fx_ease::panel_ease_pos(-2800.0, 0.0, 400);
    assert!(mid < 0.0, "进场 400ms 时必须还在路上（500ms 档），得 {mid}");
    // 离场 400ms：399ms 在路上、400ms 贴死
    let almost = fx_ease::panel_ease_pos(0.0, -2800.0, 399);
    assert!(almost > -2800.0, "离场 399ms 必须还在路上，得 {almost}");
}

#[test]
fn spec_ease_曲线形状() {
    // ease-out（进场落下）：开头快——半程时刻进度必须 >50%
    let half = fx_ease::panel_ease_pos(-2800.0, 0.0, 250);
    assert!(half > -1400.0, "ease-out 半程必须过半（开头快），得 {half}");
    // ease-in（离场收起）：开头慢——半程时刻进度必须 <50%
    let half_up = fx_ease::panel_ease_pos(0.0, -2800.0, 200);
    assert!(
        half_up > -1400.0,
        "ease-in 半程必须未过半（开头慢），得 {half_up}"
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
