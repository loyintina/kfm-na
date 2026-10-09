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
    /// BAR-238 keys-in 归属校验真源：各腿当前附着的 tmux 会话名。
    /// 与 sender 同锁同槽、切换随槽互换——inject_keys 在同一锁内
    /// 「读名比对 → 注入」，竞态窗结构性消除（MAIN0134 误投事故：
    /// 裸字节盲发活跃会话，注入与切换之间的窗口无人裁决）
    active_attached: Option<String>,
    standby_attached: Option<String>,
}

impl SessionRouter {
    /// 活跃会话起步(待机槽后补——后台接通的远程会话到位再 add)
    pub fn new(active_tx: Sender<TermCmd>, active_name: &'static str) -> Self {
        SessionRouter {
            active: (active_tx, active_name),
            standby: None,
            active_attached: None,
            standby_attached: None,
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

    /// 带回执的发送(调试闸门判案用):true = 通道活着,false = 对端
    /// 已死(僵尸通道——send 会静默吞,闸门注入不能再瞎注)
    pub fn send_checked(&self, cmd: TermCmd) -> bool {
        self.active.0.send(cmd).is_ok()
    }

    pub fn active_name(&self) -> &'static str {
        self.active.1
    }

    /// 全部会话名(活跃在前;stats 快照用)
    pub fn names(&self) -> Vec<&'static str> {
        let mut v = vec![self.active.1];
        if let Some((_, n)) = &self.standby {
            v.push(n);
        }
        v
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
        // BAR-238：附着账随槽互换——名跟人走，活跃槽恒读到当前活跃腿的附着
        std::mem::swap(&mut self.active_attached, &mut self.standby_attached);
        Some((old_name, new_name))
    }

    /// BAR-238：写某腿的附着 tmux 会话名（腿名 = local/remote 槽位名；
    /// 壳侧 attach/脱离/重孵勾销时同步）。腿名不在槽 = 装配错位，报错不静默
    pub fn set_attached(&mut self, leg: &str, name: Option<String>) -> Result<(), String> {
        if self.active.1 == leg {
            self.active_attached = name;
            return Ok(());
        }
        if let Some((_, n)) = &self.standby
            && *n == leg
        {
            self.standby_attached = name;
            return Ok(());
        }
        Err(format!(
            "set_attached: 腿 {leg} 不在槽（活跃={}）",
            self.active.1
        ))
    }

    /// BAR-238：活跃腿当前附着的 tmux 会话名（keys-in 归属校验比对源；
    /// None = 活跃腿裸 shell/未附着）
    pub fn active_attached(&self) -> Option<&str> {
        self.active_attached.as_deref()
    }

    /// BAR-238：活跃腿当前身份（归属校验/统计快照同源，单源）——附着了
    /// tmux 会话 = 会话名；裸 shell = 腿名（local/remote）。keys-in 载荷
    /// 的目标名与此比对：说出你以为在跟谁说话，不符即拒
    pub fn active_identity(&self) -> &str {
        self.active_attached.as_deref().unwrap_or(self.active.1)
    }

    /// 换心脏（断线重连，2026-08-21）：会话线程死了旧 sender 是僵尸
    /// （出向全被静默吞，conn.rs 转发循环注释有实锤）——活跃槽 sender
    /// 换新，槽位名不动
    pub fn replace_active(&mut self, tx: Sender<TermCmd>) {
        self.active.0 = tx;
    }

    /// 待机槽同款换心脏；无待机 = Err（装配错误不许静默——换了也没人收）
    pub fn replace_standby(&mut self, tx: Sender<TermCmd>) -> Result<(), String> {
        let Some((slot, _)) = &mut self.standby else {
            return Err("无待机槽可换心脏".into());
        };
        *slot = tx;
        Ok(())
    }
}

/// BAR-124 切换强制重画的抖动尺寸：行数减一（保底 1 行——0 行 pty 是
/// 畸形）。切换时先发抖动尺寸、120ms 后归位原尺寸，两次净变化对端
/// tmux 才收得到 SIGWINCH 全屏重画（同尺寸 Resize 净变化为零,内核
/// 不发信号,静默待机会话切回全是残影）。
pub fn jog_resize(cols: u32, rows: u32) -> (u32, u32) {
    (cols, rows.saturating_sub(1).max(1))
}
