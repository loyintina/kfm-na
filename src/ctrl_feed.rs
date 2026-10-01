//! ctrl_feed.rs — v4 推流画布「播种/续喂」相位机（A 档纯逻辑，2026-09-25）
//!
//! 为什么存在（BAR-155 定罪）：v4 首版把状态机写在 android_app
//! （cfg(android)，host 不可测），凭纸面设计没跑真流——上机即死：
//! ①`tmux -C attach-session` 命令行命令**自己的回应空块**最先到达，
//!   被当成播种头块判负「头块无 KFMHDR 头行」，真播种块随后到达时
//!   相位已回 Steady 全丢弃 → 永远播种不上（推流从未生效，用户看到
//!   的一直是 v3 轮询保底）；
//! ②就算空块跳过，头行认领后头块的 %end 会被当成 capture 块的 %end
//!   → 空 capture 提前 Build，真 capture 正文在 Building 相位被丢。
//! 两个病灶的根 = 相位粒度和 tmux 块序列对不上。本模块把相位机抽成
//! host 可测纯逻辑，实证 fixture 逐行钉死（pty 实录 2026-09-25）。
//!
//! 实证块序列（`tmux -C attach-session -f ignore-size` + 播种双块）：
//! ```text
//! %begin/%end            ← attach-session 命令行命令的空回应块（跳过）
//! %session-changed ...   ← Notify（无 pane 不逼播）
//! display-message ...    ← pty 回声（Plain，头行不认领，丢弃）
//! capture-pane ...       ← pty 回声（同上）
//! %begin                 ← 播种头块
//! KFMHDR 7 2790 10000 5 50 %3   ← BAR-211：KFMHDR 后首字段 = 播种 token
//! %end                   ← 头块关（不是 capture 关！BAR-155 病灶②）
//! %begin                 ← capture 块（%output 严格不插进块内）
//! ...capture 正文...
//! %end                   ← capture 收齐 → Build
//! %output %3 ...         ← 稳态续喂（pane 过滤）
//! ```
//!
//! 字节归属律（带内对齐天然零丢失零重复）：播种窗口（AwaitHeader 至
//! InCapture）内的 %output 字节**丢弃**——它们发生在 capture 执行前，
//! 已在快照内；capture 关块后的 %output 进 Building 的 pend 缓冲
//! （安装后补喂）；Steady 的 %output 直喂画布（pane id 过滤——非活动
//! 窗格的字节不许混进）。

use crate::tmux_ctl::{CtrlEvent, parse_ctrl_line, parse_seed_header};

/// 相位机动作（on_event 产物——android_app 薄壳照单执行，不自译）
#[derive(Debug, PartialEq, Eq)]
pub enum CtrlAct {
    /// 稳态 %output：字节续喂画布（pane 已过滤）
    Feed(Vec<u8>),
    /// Building 期 %output：进 pending（Canvas 安装后补喂）
    Pend(Vec<u8>),
    /// capture 块收齐：正文交后台线程建 Canvas（BAR-154：UI 零解析）。
    /// 携播种头行的 pane 游标（x,y 屏相对 0 基）——BAR-156：capture 恒
    /// 发全屏含尾空行，文本尾 ≠ 真实游标，建画布必须 CUP 归位，否则
    /// 续喂原位更新帧落屏底、旧帧留静态复制
    Build { cap: String, x: u32, y: u32 },
    /// 播种失败（调用方：相位归零 + 5s 退避 + 报表，v3 轮询兜底）。
    /// 携肇事块首行存证（BAR-155 续查：真机稳定失败而服务器复刻正常，
    /// 首行原文是唯一铁证——不许再猜第二轮）
    SeedFail(String),
    /// Notify 逼对账（%layout-change/%window-pane-changed 等：内容可能
    /// 走了 %output 覆盖不到的变化——BAR-211 F3：调用方不再绕过 cadence
    /// 逼即时重播，对账重播归 normal cadence 到点再播）
    Reconcile,
    /// %exit：通道死（调用方拆除回落 v3）
    Dead(&'static str),
    /// 无动作（播种窗口字节丢弃/回声丢弃/稳态杂散忽略）
    None,
}

/// 相位（六相——BAR-155：粒度必须和 tmux 块序列一一对上，两块的
/// 两个 %end 是两个不同相位，合并 = 病灶②）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// 稳态：未播种（pane None）或推流中（pane Some）
    Steady,
    /// 等播种头块正文（前置空块/回声都落这里跳过）
    AwaitHeader,
    /// 头行已认领，等头块的 %end（BAR-155 病灶②的分界相）
    HdrEnd,
    /// 头块已关，等 capture 块的 %begin
    AwaitBody,
    /// capture 块内，正文累积中
    InCapture,
    /// capture 收齐，Canvas 后台构建在途（built() 收口回 Steady）
    Building,
}

/// 陈旧块连跳上限（BAR-211）：AwaitHeader 期「有正文但非当前 token
/// 头」的块 = 保通道重播时 tmux 侧存活的旧播种对残骸（或无名杂块），
/// 整块静默跳过不判负；一轮内连跳到上限 = 真失败（残骸不可能这么
/// 多——旧死循环里每轮最多留一对两块），归零 + SeedFail
pub const STALE_SKIP_MAX: u32 = 4;

/// 播种/续喂相位机（实例归 App 持有；复位/发种/完工语义见各方法）
pub struct CtrlFeed {
    phase: Phase,
    /// 播种认领的活动 pane id（%output 过滤凭据；None = 未播种）
    pane: Option<u64>,
    /// 当前播种轮次 token（BAR-211：seed_sent 记账，AwaitHeader 认领
    /// 唯一凭据——认 token 不认到达序，陈旧头块/残骸块整块跳过）
    cur_token: u64,
    /// 本轮 AwaitHeader 连跳的陈旧/无名有正文块数（到 STALE_SKIP_MAX
    /// 才判负——真失败与旧对残骸同形，单块无从分辨，连跳才是铁证）
    stale_skips: u32,
    /// 播种头行的 pane 游标（列,行 屏相对 0 基；BAR-156：Build 携它
    /// 给 Canvas 归位——capture 尾空行会把文本尾拖离真实游标）
    cursor: (u32, u32),
    /// capture 正文行账（Build 时 \r\n 缝合——与 v3 capture_parse 同料。
    /// Vec 不用 String+is_empty 判首行：首行恰为空行时 is_empty 分不
    /// 出「还没行」和「有一行空行」，缝会丢一个 \r\n）
    cap: Vec<String>,
    /// AwaitHeader 期本块见过正文（空块跳过判据——attach 回应块零
    /// 正文，跳过；有正文但无当前 token 头 = 陈旧/无名块，静默跳过
    /// 记 stale_skips，连跳到顶才判负——BAR-211）
    saw_body: bool,
    /// AwaitHeader 期本块首个 Plain 行存证（连跳判负报表的铁证载荷；
    /// BlockBegin 清空，只留首行——一个块认不出头行时，第一行就是
    /// 最像样的嫌疑人）
    first_line: String,
    /// 行装配余量（%output 事件可跨包截半行；BAR-155 三号病灶定罪后
    /// 收编：装配/剥 \r/分类必须在 host 可测层，不许留在 cfg 壳里）
    line_buf: Vec<u8>,
}

impl Default for CtrlFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl CtrlFeed {
    pub fn new() -> Self {
        CtrlFeed {
            phase: Phase::Steady,
            pane: None,
            cur_token: 0,
            stale_skips: 0,
            cursor: (0, 0),
            cap: Vec::new(),
            saw_body: false,
            first_line: String::new(),
            line_buf: Vec::new(),
        }
    }

    /// 播种认领的 pane（ctrl_active 闸的凭据；None = v3 轮询兜底中）
    pub fn pane(&self) -> Option<u64> {
        self.pane
    }

    /// 稳态查询（重试/对账臂只在稳态点火——播种在途不叠发）
    pub fn is_steady(&self) -> bool {
        self.phase == Phase::Steady
    }

    /// 播种命令已发出（ctrl_seed_send 调用方同步）：转 AwaitHeader，
    /// 正文/见证清零，记本轮 token（BAR-211：认领凭据——保通道重播时
    /// 旧轮播种对仍在 tmux 侧排队/在途，token 不符的头块整块跳过）。
    /// pane 保留旧账（重播种期间 v3 闸不许开）
    pub fn seed_sent(&mut self, token: u64) {
        self.phase = Phase::AwaitHeader;
        self.cur_token = token;
        self.stale_skips = 0;
        self.cap.clear();
        self.saw_body = false;
    }

    /// 归零（判负/拆除/换主体）：相位稳态 + pane 清账（v3 闸开）
    pub fn reset(&mut self) {
        self.phase = Phase::Steady;
        self.pane = None;
        self.stale_skips = 0;
        self.cap.clear();
        self.saw_body = false;
    }

    /// Canvas 构建完工安装（ctrl_drain 调用方）：回 Steady 推流中
    pub fn built(&mut self) {
        self.phase = Phase::Steady;
    }

    /// 字节流入唯一口（行装配 + 剥 \r + 分类 + 消费全在本层）：
    /// 跨事件截半行留存余量；行尾 \r（pty 流恒带）在此剥——BAR-155
    /// 定罪：剥 \r 若留在调用方壳层，KFMHDR 头行的 pane 段会带 \r
    /// 尾导致数字解析失败，100% 判负（真机实录「首行=KFMHDR 2898
    /// 10000 5 50 %3」完全合法却判负的案发机理）
    pub fn feed_bytes(&mut self, data: &[u8]) -> Vec<CtrlAct> {
        self.line_buf.extend(data);
        let mut acts = Vec::new();
        while let Some(pos) = self.line_buf.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = self.line_buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw[..raw.len() - 1])
                .trim_end_matches('\r')
                .to_string();
            acts.push(self.on_event(parse_ctrl_line(&line), &line));
        }
        acts
    }

    /// 单行消费（feed_bytes 内部分发；Plain 行原文经 line 传入——
    /// 头行认领/正文累积要用）
    fn on_event(&mut self, ev: CtrlEvent, line: &str) -> CtrlAct {
        match ev {
            CtrlEvent::Output { pane, bytes } => {
                if self.pane != Some(pane) {
                    // 窗格未认领/非本格：无从归属，丢弃（首播种前的字节
                    // 恒在那轮 capture 内——tmux 事件串行，先到的输出
                    // 必先于 capture 命令执行，Rebuild 落地即对齐）
                    CtrlAct::None
                } else {
                    match self.phase {
                        // capture 关块后（Building）的字节恒在快照外
                        // （%end 后到达 = 快照后输出）→ Pend 待安装补喂
                        Phase::Building => CtrlAct::Pend(bytes),
                        // BAR-215③：播种窗（AwaitHeader..InCapture）的
                        // %output 不是「已在快照内」——capture 命令执行
                        // 与 %output 入队有先后竞速（命令先执行 = 后到的
                        // 输出不在快照里）。画布是面板字节流的活镜像，
                        // 窗期照喂：Rebuild 弃旧画布天然不双喂；Tail 靠
                        // 这批字节补齐 k 行（丢弃 = 5s 档 × 1s 行锁相
                        // 每档丢一行，画布缺号周期钉死档距——上滚断档
                        // 病灶，redroid 画布倒账 38 缺号/步距 5 定罪）
                        _ => CtrlAct::Feed(bytes),
                    }
                }
            }
            CtrlEvent::Plain => match self.phase {
                Phase::AwaitHeader => {
                    if !self.saw_body {
                        self.first_line = line.chars().take(80).collect();
                    }
                    self.saw_body = true;
                    if let Some(h) = parse_seed_header(line) {
                        // BAR-211：认 token 不认到达序——token 不符 = 陈旧
                        // 头块（保通道重播前发出的旧轮播种），不认领 pane、
                        // 不进 HdrEnd；块账留 saw_body，%end 时按陈旧块
                        // 静默跳过（连跳上限判负在 BlockEnd 臂）
                        if h.token == self.cur_token {
                            self.pane = Some(h.pane);
                            self.cursor = (h.cursor_x, h.cursor_y);
                            self.phase = Phase::HdrEnd;
                        }
                    }
                    CtrlAct::None
                }
                Phase::InCapture => {
                    self.cap.push(line.to_string());
                    CtrlAct::None
                }
                _ => CtrlAct::None, // 回声/稳态杂散：丢弃
            },
            CtrlEvent::BlockBegin => match self.phase {
                Phase::AwaitHeader => {
                    self.saw_body = false; // 新块起算（空块判据）
                    self.first_line.clear();
                    CtrlAct::None
                }
                Phase::AwaitBody => {
                    self.phase = Phase::InCapture;
                    self.cap.clear(); // capture 块开口 = 正文起算
                    CtrlAct::None
                }
                _ => CtrlAct::None,
            },
            CtrlEvent::BlockEnd => match self.phase {
                Phase::AwaitHeader => {
                    if self.saw_body {
                        // BAR-211：有正文但无当前 token 头 = 陈旧/无名块
                        // （保通道重播时 tmux 侧存活的旧播种对残骸：旧头块
                        // token 不符、旧 capture 块本就无头行）——静默跳过
                        // 不判负，清本块账继续等下一块（保持 AwaitHeader）；
                        // 一轮内连跳到上限 = 真失败（残骸不可能这么多），
                        // 归零 + SeedFail（新文案，与旧「无 KFMHDR 头行」
                        // 判负分列——判卷红线是旧计数不新增）
                        self.stale_skips += 1;
                        self.saw_body = false;
                        if self.stale_skips >= STALE_SKIP_MAX {
                            let clue = format!(
                                "播种头块连跳 {STALE_SKIP_MAX} 块无当前 token（真失败）: 首行={:.60}",
                                self.first_line.replace(['\r', '\n'], " ")
                            );
                            self.reset();
                            CtrlAct::SeedFail(clue)
                        } else {
                            self.first_line.clear();
                            CtrlAct::None
                        }
                    } else {
                        // 零正文空块 = 前置块（attach-session 自己的
                        // 回应，BAR-155 病灶①）——跳过继续等播种块
                        CtrlAct::None
                    }
                }
                Phase::HdrEnd => {
                    self.phase = Phase::AwaitBody;
                    CtrlAct::None
                }
                Phase::AwaitBody => {
                    self.reset();
                    CtrlAct::SeedFail("capture 块缺失（头块后直接 %end）".to_string())
                }
                Phase::InCapture => {
                    self.phase = Phase::Building;
                    CtrlAct::Build {
                        cap: std::mem::take(&mut self.cap).join("\r\n"),
                        x: self.cursor.0,
                        y: self.cursor.1,
                    }
                }
                _ => CtrlAct::None, // Steady/Building：我方只发播种双块
            },
            CtrlEvent::BlockError => match self.phase {
                Phase::AwaitHeader | Phase::HdrEnd | Phase::AwaitBody | Phase::InCapture => {
                    self.reset();
                    CtrlAct::SeedFail("命令块 %error".to_string())
                }
                _ => CtrlAct::None,
            },
            CtrlEvent::Notify => {
                if self.phase == Phase::Steady && self.pane.is_some() {
                    CtrlAct::Reconcile
                } else {
                    CtrlAct::None
                }
            }
            CtrlEvent::Exit => {
                self.reset();
                CtrlAct::Dead("tmux -C %exit（会话被 kill/断开）")
            }
        }
    }
}
