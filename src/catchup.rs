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
/// 速率轮节拍帧周期（BAR-216）：洪峰压帧 ≠ 全冻——速率追赶轮每这么久
/// 放一帧（击键回显/流式进度有上屏路，频闪 churn 变匀拍）。显式轮
/// （重播种窗快照拼装）维持全压制，不受此闸
pub const THROTTLE_MS: u128 = 400;

/// 追赶动作（壳把枚举翻译成平台操作：跳底 + 像素零头归零 + 置脏亮出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchAct {
    /// 无动作
    None,
    /// 追平落地——只发一次（退出追赶即回稳态，后续 tick 不再发）
    Land,
}

/// 进场缘由（BAR-216 观测账：显式 = 重连/发种/重播种窗；速率 = 窗内洪峰）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterCause {
    Explicit,
    Rate,
}

/// 壳侧报表用词（遥测行统一口径，接线守卫咬字面）
pub fn enter_cause_name(c: EnterCause) -> &'static str {
    match c {
        EnterCause::Explicit => "显式",
        EnterCause::Rate => "速率洪峰",
    }
}

/// 追赶观测账快照（BAR-216：壳侧遥测取数——只在 进场/落地 沿报账，
/// 不刷日志）。held_bytes = 本轮追赶期累计「喂而不画」的字节——回显
/// 无影案的第一嫌疑人就是这笔账里有用户的击键回显。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatchStats {
    pub catching: bool,
    /// 稳态→追赶 边沿计数（追赶期重复 enter 不计——幂等续窗非新沿）
    pub enter_count: u32,
    pub rate_enter_count: u32,
    pub land_count: u32,
    /// 本轮追赶期起算点（稳态期保留上轮的，供落地帧算持续时长）
    pub catching_since_ms: u128,
    /// 本轮追赶期累计压帧字节（落地沿壳取走报账；下一新沿清零）
    pub held_bytes: u64,
    /// 本轮速率追赶期已放节拍帧数（落地沿随账报；显式轮恒 0）
    pub throttle_count: u32,
    /// 击键落地累计（BAR-232：追赶期落键 = 打字了就是要看现在——
    /// 立即出追赶态落地亮出，回显不许再被压帧吞）
    pub keystroke_land_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Steady,
    Catching,
}

/// 追赶状态机。两入口：显式 enter（重连/发种/重播种窗口，壳侧钩子）
/// 与速率自动进场（note_bytes 窗内字节超阈）；三出口：静默窗 tick、
/// 播种尾锚 anchor、击键落地 keystroke_land（BAR-232）。Land 都发且只发一次。
pub struct Catchup {
    phase: Phase,
    last_byte_ms: u128,
    win_start_ms: u128,
    win_bytes: usize,
    // BAR-216 观测账（回显无影案仪器：压制量/进场因/落地因全留账）
    enter_count: u32,
    rate_enter_count: u32,
    land_count: u32,
    catching_since_ms: u128,
    held_bytes: u64,
    /// 进场沿待取（单槽——take_enter 取走前的新沿覆盖旧的，壳每圈必取
    /// 不会积压；只记 稳态→追赶 边沿，追赶期重复 enter 不产沿）
    pending_enter: Option<EnterCause>,
    /// 本轮追赶因（BAR-216：速率轮放节拍帧/显式轮全压制由此分流；
    /// 速率轮中显式 enter 升级为显式——重播种快照拼装期画中间态 = 花屏）
    round_cause: EnterCause,
    /// 上一节拍帧时刻（速率轮限拍账）
    last_throttle_ms: u128,
    /// 本轮已放节拍帧数（落地沿随账报，新沿清零）
    throttle_count: u32,
    /// 击键落地累计计数（BAR-232 观测账：击键出态发生了几次）
    keystroke_land_count: u32,
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
            enter_count: 0,
            rate_enter_count: 0,
            land_count: 0,
            catching_since_ms: 0,
            held_bytes: 0,
            pending_enter: None,
            round_cause: EnterCause::Explicit,
            last_throttle_ms: 0,
            throttle_count: 0,
            keystroke_land_count: 0,
        }
    }

    /// 显式入场：重连/发种/重播种窗口开始（壳在 seed_sent/respawn/
    /// attach/切会话/browse 重抓 处调——BAR-209① 三路同规）。重复
    /// 入场 = 重新计窗，幂等无栈（不产新进场沿、账不清零续累计）；
    /// 速率轮中显式入场 = 升级显式轮（重播种拼装期全压制，BAR-216 分流律）
    pub fn enter(&mut self, now_ms: u128) {
        if self.phase == Phase::Steady {
            self.enter_count += 1;
            self.catching_since_ms = now_ms;
            self.held_bytes = 0;
            self.throttle_count = 0;
            self.last_throttle_ms = now_ms;
            self.pending_enter = Some(EnterCause::Explicit);
        }
        self.round_cause = EnterCause::Explicit;
        self.phase = Phase::Catching;
        self.last_byte_ms = now_ms;
        self.win_start_ms = now_ms;
        self.win_bytes = 0;
    }

    /// 是否处于追赶期（壳据此抑制置脏：字节照喂 grid，帧不画）
    pub fn catching(&self) -> bool {
        self.phase == Phase::Catching
    }

    /// 观测账快照（壳在落地沿取数报账：压制量/持续时长从这出）
    pub fn stats(&self) -> CatchStats {
        CatchStats {
            catching: self.catching(),
            enter_count: self.enter_count,
            rate_enter_count: self.rate_enter_count,
            land_count: self.land_count,
            catching_since_ms: self.catching_since_ms,
            held_bytes: self.held_bytes,
            throttle_count: self.throttle_count,
            keystroke_land_count: self.keystroke_land_count,
        }
    }

    /// 取进场沿（稳态→追赶 边沿一记一取；壳在喂字节后随取随报）
    pub fn take_enter(&mut self) -> Option<EnterCause> {
        self.pending_enter.take()
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
            self.rate_enter_count += 1;
            self.enter_count += 1;
            self.catching_since_ms = now_ms;
            self.held_bytes = 0;
            self.throttle_count = 0;
            self.last_throttle_ms = now_ms;
            self.round_cause = EnterCause::Rate;
            self.pending_enter = Some(EnterCause::Rate);
        }
        if self.phase == Phase::Catching {
            self.held_bytes += nbytes as u64;
        }
        self.catching()
    }

    /// 速率轮节拍闸（BAR-216）：速率追赶期每 THROTTLE_MS 放一帧——洪峰
    /// 压帧本是合并手段，全冻却把击键回显/流式进度一并关黑（0141④
    /// 「输出时打字无回显+闪烁」定罪：落地→4ms→再进场的 churn 使回显
    /// 秒级压帧、屏幕频闪）。显式轮（重播种快照拼装）恒 false 全压制。
    /// 返回 true 即本圈该放帧（壳置脏即画，不跳底不抢滚动条）
    pub fn throttle_frame(&mut self, now_ms: u128) -> bool {
        if self.phase == Phase::Catching
            && self.round_cause == EnterCause::Rate
            && now_ms.saturating_sub(self.last_throttle_ms) >= THROTTLE_MS
        {
            self.last_throttle_ms = now_ms;
            self.throttle_count += 1;
            true
        } else {
            false
        }
    }

    /// 追平判据二：播种尾锚到达（推流画布安装 / ctrl_feed built）。
    /// 只在追赶期发 Land；稳态撞锚 = None（不抢稳态的画）。
    pub fn anchor(&mut self) -> CatchAct {
        if self.phase == Phase::Catching {
            self.phase = Phase::Steady;
            self.land_count += 1;
            CatchAct::Land
        } else {
            CatchAct::None
        }
    }

    /// 追平判据三：追赶期用户落键（BAR-232，打字了就是要看现在）——
    /// 立即退出追赶态判落地：壳据此 catchup_land（跳底+置脏亮出），
    /// 回显字节照流不丢（note_bytes 继续登记，洪峰未停则速率判据
    /// 自会再进场，下一次落键再救）。稳态/落地后二次撞键 = None
    /// （不抢稳态的画，Land 只发一次）。落地因独立计数留账。
    /// 落地沿同 enter 重置速率窗——旧窗里的洪峰记忆不清，落地后第
    /// 一笔回显即被旧账再进场压帧，击键落地形同虚设
    pub fn keystroke_land(&mut self, now_ms: u128) -> CatchAct {
        if self.phase == Phase::Catching {
            self.phase = Phase::Steady;
            self.land_count += 1;
            self.keystroke_land_count += 1;
            self.win_start_ms = now_ms;
            self.win_bytes = 0;
            CatchAct::Land
        } else {
            CatchAct::None
        }
    }

    /// 每圈节拍（壳在 about_to_wait 调）：静默窗满 → 追平落地。
    pub fn tick(&mut self, now_ms: u128) -> CatchAct {
        if self.phase == Phase::Catching && now_ms.saturating_sub(self.last_byte_ms) >= QUIET_MS {
            self.phase = Phase::Steady;
            self.land_count += 1;
            CatchAct::Land
        } else {
            CatchAct::None
        }
    }
}
