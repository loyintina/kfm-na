//! trace — 自观测·滚动行踪环（2026-08-26 与用户定：自观测第二块）
//!
//! 补的是「死前 30 秒发生了什么」：field-reports 要过 HTTP+隧道，进程
//! 暴死时队列随行就殉（BAR-022 实踩）；本环是 report 流的**本地滚动
//! 副本**——纯内存、进程死了从闸门目录捞（panic 钩子自动落尾 64 行，
//! 平时 `trace-req` 通道随查全量）。
//!
//! tap 纪律（一处接全部）：report/report_sync 是全网唯一上报咽喉，
//! 在咽喉处旁路进环 = boot/death/loop/ws/gate/session 全部事件自动
//! 入环，调用点零改动。代价是噪声——两类心跳（alive 3s、loop 10s）
//! 会稀释信号，`should_trace` 过滤掉它们（它们有自己的专用通道，
//! 环里只留「发生了什么」，不留「我还活着」）。
//!
//! 观测铁律同 gate：入环是 Mutex<VecDeque> 一次 push，O(1)、零 IO、
//! 锁即取即还；落盘全在 panic 钩子/值守线程侧，业务路径永不碰盘。

use std::collections::VecDeque;
use std::sync::Mutex;

/// 环帽（条）。心跳已滤，256 条 ≈ 安静期也能盖住案发前足够长的行踪
pub const TRACE_CAP: usize = 256;

/// 一条行踪：距 android_main 的毫秒 + 阶段名 + 详情
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    pub boot_ms: u128,
    pub stage: String,
    pub msg: String,
}

/// 滚动环（纯数据核，host 可判卷）：满帽推新挤旧
pub struct TraceRing {
    cap: usize,
    inner: VecDeque<TraceEntry>,
}

impl TraceRing {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            inner: VecDeque::with_capacity(cap.min(1024)),
        }
    }

    pub fn push(&mut self, e: TraceEntry) {
        if self.inner.len() >= self.cap {
            self.inner.pop_front();
        }
        self.inner.push_back(e);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 末 n 条（n 超存量则全给）
    pub fn tail(&self, n: usize) -> Vec<TraceEntry> {
        self.inner
            .iter()
            .skip(self.inner.len().saturating_sub(n))
            .cloned()
            .collect()
    }

    /// 格式化（钉死行格式）：`[+00012345ms stage] msg`
    pub fn format_entries(entries: &[TraceEntry]) -> String {
        let mut out = String::new();
        for e in entries {
            out.push_str(&format!("[+{:08}ms {}] {}\n", e.boot_ms, e.stage, e.msg));
        }
        out
    }

    pub fn format_tail(&self, n: usize) -> String {
        Self::format_entries(&self.tail(n))
    }
}

/// 入环过滤（纯函数，钉死）：两类周期心跳不入环——
/// alive 3s 一跳是进程活性探针（有独立直报），loop 10s 一跳是循环活性
/// 戳（有看门狗档案）；它们入环只会把真事件挤出帽外
pub fn should_trace(stage: &str, msg: &str) -> bool {
    if stage == "alive" {
        return false;
    }
    if stage == "loop" && msg.starts_with("事件循环心跳") {
        return false;
    }
    true
}

// ---- 进程级单例（report 咽喉 tap 到这里） ----

static TRACE: Mutex<Option<TraceRing>> = Mutex::new(None);

/// report 咽喉处旁路入环（report.rs 每个上报函数调一次）。
/// 锁即取即还，零 IO——业务路径感觉不到它。
pub fn tap(stage: &str, msg: &str) {
    if !should_trace(stage, msg) {
        return;
    }
    let mut g = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(|| TraceRing::new(TRACE_CAP))
        .push(TraceEntry {
            boot_ms: crate::report::boot_ms(),
            stage: stage.to_owned(),
            msg: msg.to_owned(),
        });
}

/// 全量格式化导出（trace-req 通道用）
pub fn dump_all() -> String {
    let g = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some(r) => r.format_tail(usize::MAX),
        None => String::from("(环空——本进程还没有任何事件)\n"),
    }
}

/// 末 n 行导出（panic 钩子落尾用）
pub fn dump_tail(n: usize) -> String {
    let g = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some(r) => r.format_tail(n),
        None => String::new(),
    }
}
