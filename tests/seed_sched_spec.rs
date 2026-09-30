//! BAR-209③ 自重启播种风暴调度钉（A 档纯逻辑：排头序/并发帽/在途
//! 口径）。病灶 = 0107 §二 真机实测：自重启后全会话串行重播种滚雪球
//! （第一批 8-21s，第二批 96.5s）。修 = 活动会话排头先点亮 + 在途
//! 播种并发帽防互踩。壳接线钉在 catchup_wiring_spec.rs
//! （spec_bar209_播种风暴接线守卫）。

use kfm_na::seed_sched::{SEED_INFLIGHT_CAP, admit_seed, inflight_of, order_seed};

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 活动会话排头：在名单中 → 提头；集合不变、无重复、其余保持真表原序
#[test]
fn spec_bar209_播种序_活动排头() {
    let src = names(&["闻灯", "观澜", "承影"]);
    assert_eq!(
        order_seed(&src, 8, Some("承影")),
        names(&["承影", "闻灯", "观澜"]),
        "活动会话必须排头，其余保持服务器真表原序（用户正在看的先点亮）"
    );
    // 活动本在首位 = 幂等原序
    assert_eq!(order_seed(&src, 8, Some("闻灯")), src);
    // 无活动（未附着/名单未落地）→ 原序截帽
    assert_eq!(
        order_seed(&src, 2, None),
        names(&["闻灯", "观澜"]),
        "None = 原序 take(cap)——现件截帽语义不动"
    );
    // 帽截断优先于提头之外的顺序：活动已在帽内则不扩容
    let src = names(&["a", "b", "c", "d"]);
    assert_eq!(
        order_seed(&src, 2, Some("b")),
        names(&["b", "a"]),
        "帽内活动提头不占新额（集合仍受帽约）"
    );
}

/// 活动不在名单 = 兜底追加在头（现件语义：当前会话永远在温，不受帽挤）
#[test]
fn spec_bar209_播种序_活动兜底追加() {
    let src = names(&["a", "b", "c"]);
    assert_eq!(
        order_seed(&src, 2, Some("z")),
        names(&["z", "a", "b"]),
        "帽外活动会话追加在头——当前会话永远在温，不受帽挤"
    );
}

/// 并发帽裁决：活动恒放行（排头语义不受帽拦）；非活动在途到帽即拦
#[test]
fn spec_bar209_并发帽裁决() {
    assert!(
        admit_seed(SEED_INFLIGHT_CAP, true),
        "活动会话到帽也放行——用户正在看的不许排队"
    );
    assert!(admit_seed(usize::MAX, true), "活动会话超帽也放行");
    assert!(
        admit_seed(0, false) && admit_seed(SEED_INFLIGHT_CAP - 1, false),
        "帽内放行"
    );
    assert!(
        !admit_seed(SEED_INFLIGHT_CAP, false),
        "到帽即拦——多放一路 = capture/构建洪峰互踩滚雪球复辟"
    );
    assert!(!admit_seed(SEED_INFLIGHT_CAP + 1, false), "超帽更拦");
}

/// 在途口径四象限：通道活着且相位机不在稳态 = 在途播种计 1
#[test]
fn spec_bar209_在途口径() {
    assert_eq!(inflight_of(true, false), 1, "通道活+播种在途 = 计 1");
    assert_eq!(inflight_of(true, true), 0, "稳态推流不算在途");
    assert_eq!(inflight_of(false, false), 0, "通道死/未起不计");
    assert_eq!(inflight_of(false, true), 0, "无通道无稳态也不计");
}
