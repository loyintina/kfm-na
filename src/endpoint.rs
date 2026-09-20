//! endpoint.rs — 对象轴注册表与当前对象核（解析页两轴插件契约 §二，
//! 宪法 docs/active/解析页.md。2026-09-20 用户拍板两条适配律：新终端
//! 进来现有卡不许改、新功能卡进来现有终端不许改）
//!
//! 纯逻辑核：零 IO、零平台依赖。对象轴 = 数据源提供方（exec 通道/
//! 体征/会话清单/链路状态四能力面），能力轴（插件卡）只认数据类型
//! 不认来源。注册表静态两席（服务器/本地）——未来第 N 终端 = 加一行
//! + 实现数据源，解析页与插件卡一行不动。
//!
//! 当前对象语义（宪法 §一）：解析页是「中央对象的解析器」——中央终端
//! 连着谁就解析谁，本核不自己裁决，壳在启动（设置默认会话）/切换
//! （Ctrl-]）时同步；epoch 进涂装 sig，对象切换自动重烘（与
//! parser_page epoch 同规）。

use crate::settings::DefaultSession;

/// 终端对象种类（注册表席位 id）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    Server,
    Local,
}

/// 能力面（插件卡数据契约的对端）：对象给不出 = false，插件卡据此进
/// 自声明的降级相（契约 §三），不许假装能给
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// 短命执行通道（tmux 卡吃）：服务器 = ws exec；本地 = local PTY exec
    pub exec: bool,
    /// 体征（环境卡吃）：服务器 = HTTP 轮询；本地 = na_sys::collect 直读
    pub sys_info: bool,
    /// 会话清单（服务卡吃）：服务器 = HTTP health；本地 = 本地 PTY 表
    pub health: bool,
    /// 链路状态（连接卡吃）：服务器 = 隧道快照；本地 = 恒在线回环
    pub link: bool,
}

/// 注册表席位
pub struct EndpointDef {
    pub kind: EndpointKind,
    /// 对象词静态兜底（动态词——服务器名/sup.target——仍归各卡现状
    /// 数据源；本词只在无动态词时兜底）
    pub display: &'static str,
    pub caps: Caps,
}

/// 注册表（有序——未来「下一个对象」轮换语义的根据；新终端律：
/// 加一行 = 新对象，插件卡零改动）。服务器在前 = 现状锚（解析页恒
/// 服务器相的现状不变，壳同步才翻相）。static 不 const——const 逐
/// 处内联会产生多份分配，def() 反查的同一席保证就没了
pub static REGISTRY: &[EndpointDef] = &[
    EndpointDef {
        kind: EndpointKind::Server,
        display: "服务器",
        caps: Caps {
            exec: true,
            sys_info: true,
            health: true,
            link: true,
        },
    },
    EndpointDef {
        kind: EndpointKind::Local,
        display: "本地",
        // 四能力全开（2026-09-20 用户拍板「前者」：本地相连接卡显
        // 自查信息，三卡结构稳定不收起）——本地链路 = 恒在线回环，
        // 与服务器隧道同一契约面
        caps: Caps {
            exec: true,
            sys_info: true,
            health: true,
            link: true,
        },
    },
];

/// 种类 → 席位反查（与注册表同一份，不许另写一份漂移）
pub fn def(kind: EndpointKind) -> &'static EndpointDef {
    REGISTRY
        .iter()
        .find(|d| d.kind == kind)
        .expect("EndpointKind 全枚举必有席位——注册表漏席 = 装配错误")
}

/// 设置 → 对象映射（壳启动同步用）：DefaultSession::Server(id) 的 id
/// 归属 servers 表裁决，对象轴只认「是服务器」
pub fn of_default_session(d: &DefaultSession) -> EndpointKind {
    match d {
        DefaultSession::Local => EndpointKind::Local,
        DefaultSession::Server(_) => EndpointKind::Server,
    }
}

/// 当前对象状态核（纯数据面——A 档判卷不碰全局；全局句柄是壳/涂装
/// 的薄共享，同 parser_page 全局句柄模式）
pub struct EndpointState {
    cur: EndpointKind,
    epoch: u64,
}

impl Default for EndpointState {
    /// 现状锚：默认 = 服务器（解析页恒服务器相的现状不变——壳启动
    /// 按设置同步后才可能翻相，同步本身 = 第 7 步的既定行为）
    fn default() -> Self {
        EndpointState {
            cur: EndpointKind::Server,
            epoch: 0,
        }
    }
}

impl EndpointState {
    pub fn current(&self) -> EndpointKind {
        self.cur
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 同步当前对象，返回同步后的 epoch。翻相 epoch +1（进涂装 sig
    /// 自动重烘——对象切换画面必须变）；同值 = 不抖（壳每圈同步不许
    /// 引发重烘风暴）
    pub fn set_current(&mut self, kind: EndpointKind) -> u64 {
        if self.cur != kind {
            self.cur = kind;
            self.epoch += 1;
        }
        self.epoch
    }
}
