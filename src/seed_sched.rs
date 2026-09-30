//! seed_sched.rs — BAR-209③：自重启播种风暴的调度闸（0107 §二 真机
//! 实测：全会话串行重播种滚雪球——第一批 8-21s，第二批 96.5s，分钟级
//! 不可用）。两件：
//! ①活动会话排头——用户正在看的会话先播种先点亮，其余保持服务器
//!   真表原序；
//! ②在途播种并发帽——同时处于「通道已起、首播未落地」的会话数设
//!   上限，防 N 路 capture/构建洪峰互踩滚雪球（96.5s 那笔的形态
//!   就是堆积）。
//!
//! 纯逻辑件（A 档）：进出都是名单/计数，零平台依赖零 IO。壳接线钉在
//! tests/catchup_wiring_spec.rs（spec_bar209_播种风暴接线守卫）。

/// 在途播种并发帽（非活动会话）：到帽即排队下圈再放行。活动会话
/// 不受帽拦（排头优先的另一半语义——用户正在看的不许排队）
pub const SEED_INFLIGHT_CAP: usize = 2;

/// 播种顺序裁决：活动会话排头，其余保持名单原序截帽；活动不在名单
/// （名单未落地/附着不在表）兜底追加在头——现件「当前会话永远在温、
/// 不受帽挤」语义原样保住，只动顺序
pub fn order_seed(names: &[String], cap: usize, cur: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(c) = cur {
        out.push(c.to_string());
    }
    for n in names.iter().take(cap) {
        if Some(n.as_str()) != cur {
            out.push(n.clone());
        }
    }
    out
}

/// 在途放行裁决：活动会话恒放行；非活动在途数到帽即拦（下圈再看）
pub fn admit_seed(inflight: usize, is_cur: bool) -> bool {
    is_cur || inflight < SEED_INFLIGHT_CAP
}

/// 在途播种计数口径（壳逐条目归并求和）：通道活着且相位机不在稳态
/// = 首播/重播在途计 1。单一口径——各算各的 = 帽形同虚设
pub fn inflight_of(ctrl_alive: bool, steady: bool) -> usize {
    usize::from(ctrl_alive && !steady)
}
