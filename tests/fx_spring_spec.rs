//! fx_spring_spec.rs — 采样缝 + 弹簧落下考题（A 档纯逻辑，ui-base.md
//! §三/§四/§五 第一消费者，答案 src/ui/seam.rs + src/ui/fx_spring.rs +
//! src/plugins/ui_fx.rs + termview::blit_panel_shifted）
//!
//! 判卷维度：
//! - 弹簧曲线：端点精确 / 过冲存在且有界（物理感的来源）/ 收敛贴死
//!   不渐近（帧时钟停表的判据）
//! - 缝：无占槽直通目标值（硬切基座）/ 占槽驱动插值 / 目标变化重定基
//!   不跳变 / 拔槽回直通 / 插件 disabled 一键关不占槽
//! - blit 移位压盖三态：0=原样全盖（与直接渲染像素等价）/ -h=屏外不动 /
//!   中间值=上截下补
//! - 帧时钟：无活跃动画零帧（夜判据红线）/ 有动画 ≤60fps
//!
//! 变异抽检：ζ→0.99 过冲消失咬曲线题；收敛贴死删除（无限渐近）咬
//! 收敛题与重定基题；缝 None 臂返回非目标值咬直通题；primed 删除
//! （首采样重放）咬占槽题。

use kfm_na::base::{Base, PluginEntry};
use kfm_na::termview::blit_panel_shifted;
use kfm_na::ui::{fx_spring, seam};

// 文件内串行锁：占全局槽的题必须串行进场（BAR-057 PUMP_LOCK 同款；
// 集成测试按文件分进程，本锁只管本文件内并行）
static SEAM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---- 弹簧曲线（纯函数零墙钟） ----

#[test]
fn spec_spring_端点精确() {
    assert_eq!(
        fx_spring::spring_pos(-2800.0, 0.0, 0),
        -2800.0,
        "t=0 必须在起点"
    );
    assert_eq!(fx_spring::spring_pos(0.0, 0.0, 0), 0.0, "零位移直出目标");
    assert_eq!(
        fx_spring::spring_pos(-2800.0, 0.0, 10_000),
        0.0,
        "超时兜底必须贴死目标（病态也不许永动）"
    );
}

#[test]
fn spec_spring_过冲存在且有界() {
    // 1260×2800 屏全高落下：过冲 = 越过目标的最大距离。物理感来自
    // 过冲——ζ 调高（过阻尼化）过冲消失，本题必须红（变异咬点）
    let from = -2800.0_f32;
    let mut max_over = 0.0_f32; // pos - target 的最大正值
    let mut prev = from;
    for t in (0..600).step_by(4) {
        let pos = fx_spring::spring_pos(from, 0.0, t);
        max_over = max_over.max(pos);
        if pos != 0.0 {
            // 未贴死前不许反向跳回起点方向（弹簧不是来回弹的皮球——
            // 欠阻尼但单次过冲，振铃 ≥2 次 = 参数病态）
            assert!(pos >= from, "位置不许越过起点反向: t={t} pos={pos}");
            prev = pos;
        }
    }
    let _ = prev;
    let pct = max_over / 2800.0;
    assert!(
        (0.005..=0.035).contains(&pct),
        "过冲必须存在且有界（0.5%~3.5% 屏高），实测 {pct:.4}——\
         过小=物理感没了（ζ 变异），过大=拍桌子不是墩一下"
    );
}

#[test]
fn spec_spring_收敛贴死不渐近() {
    // 必须存在有限时刻贴死目标（== target 的精确值），此后不再离开——
    // 帧时钟靠这个停表；无限渐近 = 动画永不结束 = 空烧帧（变异咬点）
    let from = -2800.0_f32;
    let mut settle_t = None;
    for t in (0..=600).step_by(4) {
        if fx_spring::spring_pos(from, 0.0, t) == 0.0 {
            settle_t = Some(t);
            break;
        }
    }
    let settle_t = settle_t.expect("600ms 内必须收敛贴死（否则帧时钟永不停表）");
    assert!(settle_t <= 500, "收敛时刻 {settle_t}ms 超出体感窗口");
    for t in (settle_t..700).step_by(20) {
        assert_eq!(
            fx_spring::spring_pos(from, 0.0, t),
            0.0,
            "贴死后不许再离开目标: t={t}"
        );
    }
}

// ---- 缝（全局槽，串行进场） ----

#[test]
fn spec_seam_无占槽直通目标值() {
    let _g = SEAM_LOCK.lock().unwrap();
    seam::release_ai_panel_offset_y(); // 防前题残槽
    assert_eq!(
        seam::sample_ai_panel_offset_y(7.0, 0),
        7.0,
        "无占槽 = 硬切直通"
    );
    assert_eq!(seam::sample_ai_panel_offset_y(-99.0, 123), -99.0);
    assert!(!seam::ai_panel_offset_y_active(), "无占槽恒无活跃动画");
}

#[test]
fn spec_seam_占槽插值与重定基不跳变() {
    let _g = SEAM_LOCK.lock().unwrap();
    seam::release_ai_panel_offset_y();
    seam::occupy_ai_panel_offset_y(fx_spring::spring_occupier());
    // 首采样直通（primed：冷启动/热装不补演历史）
    assert_eq!(seam::sample_ai_panel_offset_y(-2800.0, 0), -2800.0);
    assert!(!seam::ai_panel_offset_y_active(), "首采样即稳态，无动画");
    // 目标翻转：当刻不许跳变（从当前值续弹，来回狂点不瞬移）
    let p0 = seam::sample_ai_panel_offset_y(0.0, 100);
    assert!(
        (p0 - -2800.0).abs() < 1.0,
        "目标翻转当刻必须还在起点附近，实测 {p0}"
    );
    assert!(seam::ai_panel_offset_y_active(), "目标变了 = 动画开始");
    // 中途：在起点与目标之间（插值真在跑）
    let p_mid = seam::sample_ai_panel_offset_y(0.0, 200);
    assert!(
        (-2800.0..1.0).contains(&p_mid),
        "中途位置必须在行程内，实测 {p_mid}"
    );
    // 收敛：贴死目标且活性灭（帧时钟停表的依据）
    let p_end = seam::sample_ai_panel_offset_y(0.0, 900);
    assert_eq!(p_end, 0.0, "超时必贴死");
    assert!(!seam::ai_panel_offset_y_active());
    // 拔槽回硬切（ui-fx 禁用/卸载的功能等价路径）
    seam::release_ai_panel_offset_y();
    assert_eq!(seam::sample_ai_panel_offset_y(-5.0, 1000), -5.0);
}

#[test]
fn spec_插件_ui_fx_disabled一键关不占槽() {
    let _g = SEAM_LOCK.lock().unwrap();
    seam::release_ai_panel_offset_y();
    let base = Base::new(vec![PluginEntry {
        id: kfm_na::plugins::ui_fx::PLUGIN_NAME,
        disabled: true,
        config: None,
    }]);
    base.load(kfm_na::plugins::ui_fx::UiFx::new())
        .expect("disabled 也该注册得进（只是不激活）");
    assert_eq!(
        seam::sample_ai_panel_offset_y(3.0, 0),
        3.0,
        "disabled → 不占槽 → 全局硬切（na-regress 禁用 ui-fx 全卷绿的兑现）"
    );
    // 对照：启用 = 占槽（apply 真装了采样器）
    let base2 = Base::new(vec![PluginEntry {
        id: kfm_na::plugins::ui_fx::PLUGIN_NAME,
        disabled: false,
        config: None,
    }]);
    base2
        .load(kfm_na::plugins::ui_fx::UiFx::new())
        .expect("启用必须装得上");
    assert_eq!(
        seam::sample_ai_panel_offset_y(3.0, 0),
        3.0,
        "启用首采样直通（primed）"
    );
    let p = seam::sample_ai_panel_offset_y(0.0, 50);
    assert!(
        (2.0..=3.0).contains(&p),
        "启用后目标变化走插值不是直通，实测 {p}"
    );
    seam::release_ai_panel_offset_y(); // 收场：不留残槽给后题
}

// ---- blit 移位压盖（渲染合成原子） ----

#[test]
fn spec_blit_移位压盖三态() {
    let (w, h) = (2u32, 4u32);
    let src: Vec<u32> = (1..=8).collect(); // 行 [1,2] [3,4] [5,6] [7,8]
    // y_off=0：原样全盖（与直接渲染像素等价——靠泊帧不许多一分一毫）
    let mut dst = vec![99u32; 8];
    blit_panel_shifted(&mut dst, &src, w, h, 0);
    assert_eq!(dst, src, "y_off=0 必须像素等价全盖");
    // y_off=-h：完全屏外，dst 一个像素都不许动（终端页稳态零成本）
    let mut dst = vec![99u32; 8];
    blit_panel_shifted(&mut dst, &src, w, h, -(h as i32));
    assert_eq!(dst, vec![99u32; 8], "屏外压盖 = 不动 dst");
    // y_off=-2（半程）：上截下补——dst 前 2 行保持（终端内容），
    // 后 2 行 = src 前 2 行（面板顶落进屏心）
    let mut dst = vec![99u32; 8];
    blit_panel_shifted(&mut dst, &src, w, h, -2);
    assert_eq!(dst, vec![99, 99, 99, 99, 1, 2, 3, 4], "半程 = 上截下补");
    // 越界钳制：比 -h 还小按屏外算，不许 panic 不许动 dst
    let mut dst = vec![99u32; 8];
    blit_panel_shifted(&mut dst, &src, w, h, -100);
    assert_eq!(dst, vec![99u32; 8]);
}

// ---- 帧时钟（ui-base §四 按需启停） ----

#[test]
fn spec_帧时钟_无动画零帧有动画限频() {
    let _g = SEAM_LOCK.lock().unwrap();
    seam::release_ai_panel_offset_y();
    // 无占槽：恒不产帧（0.45% 单核夜判据红线——动画系统不许抬升基线）
    assert!(!fx_spring::panel_frame_due(0));
    assert!(!fx_spring::panel_frame_due(1000));
    // 有活跃动画：首帧即产，16ms 内不重复（≤60fps），到点再产
    seam::occupy_ai_panel_offset_y(fx_spring::spring_occupier());
    assert_eq!(seam::sample_ai_panel_offset_y(-100.0, 2000), -100.0); // primed
    seam::sample_ai_panel_offset_y(0.0, 2000); // 目标翻转 = 动画开始
    assert!(fx_spring::panel_frame_due(2000), "动画开始即产首帧");
    assert!(
        !fx_spring::panel_frame_due(2008),
        "16ms 内不重复产帧（限频）"
    );
    assert!(fx_spring::panel_frame_due(2016), "到点再产");
    // 收敛后：停表——零额外帧
    seam::sample_ai_panel_offset_y(0.0, 2900); // 超时贴死
    assert!(!fx_spring::panel_frame_due(2916), "动画停即停表");
    seam::release_ai_panel_offset_y();
}
