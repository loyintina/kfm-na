//! BAR-186 臂②：追赶模式（catch-up）——弱网重连/播种窗口字节滴灌
//! 逐帧慢滚（用户 2026-09-29 报障「tmux 窗口疯狂大滚动、滚得很慢」）
//! 的修。定罪见 0047 §二：字节滴灌无帧合并，唯一合批是事件循环 4ms
//! 节拍——弱网滴灌下等于每包一帧。
//!
//! 修律：追赶期 advance 照喂 grid 但**不置脏不重画**；追平判据
//! （静默窗满 / 播种尾锚到达）满足后一帧跳底亮出。稳态不动。
//!
//! 纯逻辑件（A 档）：时钟由宿主注入（now_ms 参数，壳侧
//! crate::report::boot_ms()），零平台依赖零 IO。壳接线钉在
//! tests/catchup_wiring_spec.rs。

/// 追平判据一：静默窗口——最后一笔字节后这么久无新字节即视为追平
pub const QUIET_MS: u128 = 150;
/// 速率触发窗长：窗内字节量超阈即自动进场（非重连的滴灌洪峰也罩住）
pub const RATE_WINDOW_MS: u128 = 100;
/// 速率触发阈（窗内字节数；打字回显量级远低于此，不误伤稳态）
pub const RATE_BYTES: usize = 16 * 1024;

/// 追赶动作（壳把枚举翻译成平台操作：跳底 + 像素零头归零 + 置脏亮出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchAct {
    /// 无动作
    None,
    /// 追平落地——只发一次（退出追赶即回稳态，后续 tick 不再发）
    Land,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Steady,
    Catching,
}

/// 追赶状态机。两入口：显式 enter（重连/发种/重播种窗口，壳侧钩子）
/// 与速率自动进场（note_bytes 窗内字节超阈）；两出口：静默窗 tick
/// 与播种尾锚 anchor。Land 都发且只发一次。
pub struct Catchup {
    phase: Phase,
    last_byte_ms: u128,
    win_start_ms: u128,
    win_bytes: usize,
}

impl Default for Catchup {
    fn default() -> Self {
        Self::new()
    }
}

impl Catchup {
    pub fn new() -> Self {
        Self {
            phase: Phase::Steady,
            last_byte_ms: 0,
            win_start_ms: 0,
            win_bytes: 0,
        }
    }

    /// 显式入场：重连/发种/重播种窗口开始（壳在 seed_sent/respawn/
    /// browse 重抓 处调）。重复入场 = 重新计窗，幂等无栈。
    pub fn enter(&mut self, now_ms: u128) {
        self.phase = Phase::Catching;
        self.last_byte_ms = now_ms;
        self.win_start_ms = now_ms;
        self.win_bytes = 0;
    }

    /// 是否处于追赶期（壳据此抑制置脏：字节照喂 grid，帧不画）
    pub fn catching(&self) -> bool {
        self.phase == Phase::Catching
    }

    /// 字节到账登记（两条进料通路都调）。返回登记后的追赶态——
    /// 本笔字节若触发速率进场，返回值即 true（本批也不置脏，归并
    /// 到落地帧一起亮）。
    pub fn note_bytes(&mut self, now_ms: u128, nbytes: usize) -> bool {
        if now_ms.saturating_sub(self.win_start_ms) > RATE_WINDOW_MS {
            self.win_start_ms = now_ms;
            self.win_bytes = 0;
        }
        self.win_bytes += nbytes;
        self.last_byte_ms = now_ms;
        if self.phase == Phase::Steady && self.win_bytes >= RATE_BYTES {
            self.phase = Phase::Catching;
        }
        self.catching()
    }

    /// 追平判据二：播种尾锚到达（推流画布安装 / ctrl_feed built）。
    /// 只在追赶期发 Land；稳态撞锚 = None（不抢稳态的画）。
    pub fn anchor(&mut self) -> CatchAct {
        if self.phase == Phase::Catching {
            self.phase = Phase::Steady;
            CatchAct::Land
        } else {
            CatchAct::None
        }
    }

    /// 每圈节拍（壳在 about_to_wait 调）：静默窗满 → 追平落地。
    pub fn tick(&mut self, now_ms: u128) -> CatchAct {
        if self.phase == Phase::Catching && now_ms.saturating_sub(self.last_byte_ms) >= QUIET_MS {
            self.phase = Phase::Steady;
            CatchAct::Land
        } else {
            CatchAct::None
        }
    }
}
