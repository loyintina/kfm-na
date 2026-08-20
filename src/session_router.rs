//! session_router.rs — 双会话输入路由(L1;多端分层评审裁决 4 附议考题:
//! 「切换后输入路由」比输出渲染更容易出 bug,抽纯数据面上 A 档钉)
//!
//! 纯路由核:零 IO、零平台依赖,host 可判卷。壳(android_app)持有它,
//! 击键/IME 字节经它发往活跃会话;Ctrl-] 切换 = 活跃/待机互换。
//! 注意分工:本结构只管**出向**(input 发谁);入向事件通道(event_rx)
//! 归壳持有,切换时壳同步换 rx——同一方法内完成,不许分开动。

use std::sync::mpsc::Sender;

use crate::conn::TermCmd;

pub struct SessionRouter {
    active: (Sender<TermCmd>, &'static str),
    standby: Option<(Sender<TermCmd>, &'static str)>,
}

impl SessionRouter {
    /// 活跃会话起步(待机槽后补——后台接通的远程会话到位再 add)
    pub fn new(active_tx: Sender<TermCmd>, active_name: &'static str) -> Self {
        SessionRouter {
            active: (active_tx, active_name),
            standby: None,
        }
    }

    /// 待机会话到位(只可补一次;重复补 = 装配错误,覆盖会丢会话通道)
    pub fn add_standby(&mut self, tx: Sender<TermCmd>, name: &'static str) -> Result<(), String> {
        if let Some((_, occupied)) = &self.standby {
            return Err(format!("待机槽已被 {occupied} 占据,拒绝覆盖"));
        }
        self.standby = Some((tx, name));
        Ok(())
    }

    /// 输入路由唯一入口:一切出向命令发往活跃会话
    pub fn send(&self, cmd: TermCmd) {
        let _ = self.active.0.send(cmd);
    }

    pub fn active_name(&self) -> &'static str {
        self.active.1
    }

    pub fn has_standby(&self) -> bool {
        self.standby.is_some()
    }

    /// 切换:活跃/待机互换,返回 (旧活跃名, 新活跃名);无待机 = None(不动)
    pub fn switch(&mut self) -> Option<(&'static str, &'static str)> {
        let standby = self.standby.take()?;
        let new_name = standby.1;
        let old_name = self.active.1;
        let old = std::mem::replace(&mut self.active, standby);
        self.standby = Some(old);
        Some((old_name, new_name))
    }
}
