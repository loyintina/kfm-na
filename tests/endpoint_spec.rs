//! endpoint_spec.rs — 对象轴注册表/当前对象核的 A 档考题（契约：
//! docs/active/解析页.md §二。新终端律：第 N 终端 = 注册表加一行，
//! 插件卡零改动）

use kfm_na::endpoint::{self, EndpointKind, EndpointState};
use kfm_na::settings::DefaultSession;

#[test]
fn spec_endpoint_注册表两席有序() {
    // 现状两席：服务器在前（现状锚——解析页恒服务器相）、本地在后；
    // 有序 = 未来「下一个对象」轮换语义的根据
    assert_eq!(endpoint::REGISTRY.len(), 2);
    assert_eq!(endpoint::REGISTRY[0].kind, EndpointKind::Server);
    assert_eq!(endpoint::REGISTRY[1].kind, EndpointKind::Local);
    // def() 反查与注册表同一份（不许另写一份漂移）
    for def in endpoint::REGISTRY {
        let looked = endpoint::def(def.kind);
        assert!(std::ptr::eq(looked, def), "def() 必须返回注册表内的同一席");
    }
}

#[test]
fn spec_endpoint_能力面_两席全开() {
    // 现状契约：两席四能力全开——本地相链路 = 恒在线回环（三卡结构
    // 稳定，用户拍板「前者」：本地相连接卡显自查信息不收起）；未来
    // 某席给不出某能力 = 该席 false，插件卡进降级相
    for def in endpoint::REGISTRY {
        assert!(def.caps.exec, "{:?} exec", def.kind);
        assert!(def.caps.sys_info, "{:?} sys_info", def.kind);
        assert!(def.caps.health, "{:?} health", def.kind);
        assert!(def.caps.link, "{:?} link", def.kind);
    }
}

#[test]
fn spec_endpoint_设置映射() {
    // DefaultSession::Server(id) 的 id 归属 servers 表裁决，对象轴只认「是服务器」
    assert_eq!(
        endpoint::of_default_session(&DefaultSession::Local),
        EndpointKind::Local
    );
    assert_eq!(
        endpoint::of_default_session(&DefaultSession::Server("home".into())),
        EndpointKind::Server
    );
}

#[test]
fn spec_endpoint_翻相epoch() {
    // 现状锚：默认 = 服务器（解析页恒服务器相的现状不变，壳同步才翻相）
    let mut s = EndpointState::default();
    assert_eq!(s.current(), EndpointKind::Server);
    let e0 = s.epoch();
    // 翻相：epoch 必须 +1（进涂装 sig 自动重烘——对象切换画面必须变）
    assert_eq!(s.set_current(EndpointKind::Local), e0 + 1);
    assert_eq!(s.current(), EndpointKind::Local);
    assert_eq!(s.epoch(), e0 + 1);
    // 同值同步 = 不抖 epoch（壳每圈同步不许引发重烘风暴）
    assert_eq!(s.set_current(EndpointKind::Local), e0 + 1);
    assert_eq!(s.epoch(), e0 + 1);
    // 翻回
    assert_eq!(s.set_current(EndpointKind::Server), e0 + 2);
    assert_eq!(s.current(), EndpointKind::Server);
}

#[test]
fn spec_endpoint_exec通道裁决() {
    use kfm_na::endpoint::{ExecPlan, plan_exec};
    // 服务器相 + 已配置 = ws 通道（url 原样透传——壳的 remote_conn_cfg）
    match plan_exec(EndpointKind::Server, Some("ws://127.0.0.1:9021/ws")) {
        ExecPlan::Ws(u) => assert_eq!(u, "ws://127.0.0.1:9021/ws"),
        _ => panic!("服务器相+已配置必须走 ws"),
    }
    // 服务器相 + 无配置 = NoServer（各调用点自有报错语义，裁决层不措辞）
    assert!(matches!(
        plan_exec(EndpointKind::Server, None),
        ExecPlan::NoServer
    ));
    // 本地相 = LocalPty（第 6 步接线；与有无服务器配置无关）
    assert!(matches!(
        plan_exec(EndpointKind::Local, None),
        ExecPlan::LocalPty
    ));
    assert!(matches!(
        plan_exec(EndpointKind::Local, Some("ws://x")),
        ExecPlan::LocalPty
    ));
}
