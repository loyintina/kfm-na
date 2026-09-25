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
//! KFMHDR 2790 10000 5 50 %3
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
    /// 走了 %output 覆盖不到的变化——调用方清零对账账，下拍重播种）
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

/// 播种/续喂相位机（实例归 App 持有；复位/发种/完工语义见各方法）
pub struct CtrlFeed {
    phase: Phase,
    /// 播种认领的活动 pane id（%output 过滤凭据；None = 未播种）
    pane: Option<u64>,
    /// 播种头行的 pane 游标（列,行 屏相对 0 基；BAR-156：Build 携它
    /// 给 Canvas 归位——capture 尾空行会把文本尾拖离真实游标）
    cursor: (u32, u32),
    /// capture 正文行账（Build 时 \r\n 缝合——与 v3 capture_parse 同料。
    /// Vec 不用 String+is_empty 判首行：首行恰为空行时 is_empty 分不
    /// 出「还没行」和「有一行空行」，缝会丢一个 \r\n）
    cap: Vec<String>,
    /// AwaitHeader 期本块见过正文（空块跳过判据——attach 回应块零
    /// 正文，跳过；有正文但认不出头行才判负）
    saw_body: bool,
    /// AwaitHeader 期本块首个 Plain 行存证（判负报表的铁证载荷；
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
    /// 正文/见证清零。pane 保留旧账（重播种期间 v3 闸不许开）
    pub fn seed_sent(&mut self) {
        self.phase = Phase::AwaitHeader;
        self.cap.clear();
        self.saw_body = false;
    }

    /// 归零（判负/拆除/换主体）：相位稳态 + pane 清账（v3 闸开）
    pub fn reset(&mut self) {
        self.phase = Phase::Steady;
        self.pane = None;
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
            CtrlEvent::Output { pane, bytes } => match self.phase {
                // 播种窗口内的输出已在 capture 快照内（带内对齐）——丢弃
                Phase::AwaitHeader | Phase::HdrEnd | Phase::AwaitBody | Phase::InCapture => {
                    CtrlAct::None
                }
                Phase::Building => {
                    if self.pane == Some(pane) {
                        CtrlAct::Pend(bytes)
                    } else {
                        CtrlAct::None
                    }
                }
                Phase::Steady => {
                    if self.pane == Some(pane) {
                        CtrlAct::Feed(bytes)
                    } else {
                        CtrlAct::None
                    }
                }
            },
            CtrlEvent::Plain => match self.phase {
                Phase::AwaitHeader => {
                    if !self.saw_body {
                        self.first_line = line.chars().take(80).collect();
                    }
                    self.saw_body = true;
                    if let Some(h) = parse_seed_header(line) {
                        self.pane = Some(h.pane);
                        self.cursor = (h.cursor_x, h.cursor_y);
                        self.phase = Phase::HdrEnd;
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
                        // 有正文但认不出头行 = 真播种失败（带首行存证）
                        let clue = format!(
                            "头块有正文但无 KFMHDR 头行: 首行={:.60}",
                            self.first_line.replace(['\r', '\n'], " ")
                        );
                        self.reset();
                        CtrlAct::SeedFail(clue)
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
