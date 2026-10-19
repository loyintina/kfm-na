//! lockprobe.rs — 锁探针（MAIN0192 楼596／601 定性、楼629 派单、楼654／655／657／658 契约）
//!
//! 病：主线程在绘制路径上**同线程重入同一把 std Mutex**（外层 guard 活到块尾，
//! 分支里又 lock 同一 Arc）⇒ 不可重入 ⇒ futex 死等（CPU 不涨）⇒ 输入 5 秒不消费
//! ⇒ am_anr ⇒ AMS 杀。修法已单独落在 draw 路径；本模块只负责**把这类错从「运行期
//! 偶发」变成「构造期必现」**：谁持锁、谁在等、等的是不是自己。
//!
//! **死锁路径白名单（楼658 定稿）**——`enter` 到 `Drop` 之间只许出现**已预先取得**的
//! 资源：预打开的文件描述符、线程令牌、栈上定长缓冲。其余一律不许：
//!   - 不许 `format!`／`to_string`（分配器本身可取锁）
//!   - 不许 `thread_local!` 懒初始化（首次访问会分配／加锁）——令牌必须由
//!     `prefetch_token()` 在帧外取好再传进来
//!   - 不许走 `report::report*` 通道（它内部取 SENDER 锁，此刻正是「锁被自己持有」）
//!
//! 落盘用 `O_APPEND` 单次 `write`，一行一条，天然原子。

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// 判定三分类（纯逻辑，A 档可判）
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Verdict {
    /// holder 非 0 且等于自己 ⇒ 同线程重入
    SelfDeadlock,
    /// holder 非 0 且不是自己 ⇒ 正常竞争
    Contend,
    /// holder 为 0 ⇒ 有人持着却没登记（本身也是读数）
    Unowned,
}

/// 分类（纯函数，钉在 web_page_spec）：「是不是自己」才是自锁的唯一判据——tid 会
/// 复用，所以令牌必须进程内自增。
pub fn classify(holder: u64, me: u64) -> Verdict {
    match holder {
        0 => Verdict::Unowned,
        h if h == me => Verdict::SelfDeadlock,
        _ => Verdict::Contend,
    }
}

/// 命中来源：自然（用户真实使用中撞上）——主动触发一律不用探针记录，见楼659。
pub const MODE_NATURAL: &str = "natural";

/// 进程内自增令牌（**帧外**调用；不要在 enter 之后调用）
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

pub fn prefetch_token() -> u64 {
    NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
}

/// holder 登记表：极小的定长槽（按 arc_id 的指针哈希），只存 u64，不做堆分配
const SLOTS: usize = 8;
static HOLDERS: [AtomicU64; SLOTS] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

fn slot_of(arc: &'static str) -> usize {
    // FNV-1a 的极简版：只求分散，不求密码学
    let mut h: usize = 2166136261;
    for b in arc.as_bytes() {
        h ^= *b as usize;
        h = h.wrapping_mul(16777619);
    }
    h % SLOTS
}

pub fn set_holder(arc: &'static str, tok: u64) {
    HOLDERS[slot_of(arc)].store(tok, Ordering::Release);
}

pub fn clear_holder(arc: &'static str) {
    HOLDERS[slot_of(arc)].store(0, Ordering::Release);
}

pub fn holder_of(arc: &'static str) -> u64 {
    HOLDERS[slot_of(arc)].load(Ordering::Acquire)
}

/// 落盘句柄：**帧外预打开**（`init_log`），死锁路径上只用这个 fd
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

/// 帧外调用一次：预打开探针日志（追加写）。失败不影响主流程，只是探针静默。
pub fn init_log(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        && let Ok(mut g) = LOG_FILE.lock()
    {
        *g = Some(f);
    }
}

/// 因果锚（楼665）：命中那一帧的 web_on_top 与 ai_snap 取值——不取锁，纯局部量。
#[derive(Debug, Clone, Copy, Default)]
pub struct Anchors {
    pub web_on_top: bool,
    pub ai_snap: bool,
}

/// 单调钟毫秒（**差值只用它**；墙钟只做跨机对表，见楼662）
pub fn mono_ms() -> u64 {
    use std::sync::OnceLock;
    static BASE: OnceLock<std::time::Instant> = OnceLock::new();
    BASE.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// 栈上定长写入器（**不分配**，符合死锁路径白名单）
struct Line {
    buf: [u8; 224],
    n: usize,
}

impl Line {
    fn new() -> Self {
        Line {
            buf: [0u8; 224],
            n: 0,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        for b in bytes {
            if self.n < self.buf.len() {
                self.buf[self.n] = *b;
                self.n += 1;
            }
        }
    }
    fn push_u64(&mut self, mut v: u64) {
        let mut tmp = [0u8; 20];
        let mut i = 0usize;
        if v == 0 {
            tmp[0] = b'0';
            i = 1;
        }
        while v > 0 {
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
        while i > 0 {
            i -= 1;
            let b = tmp[i];
            self.push(&[b]);
        }
    }
    fn flush(&self) {
        if let Ok(mut g) = LOG_FILE.lock()
            && let Some(f) = g.as_mut()
        {
            let _ = f.write_all(&self.buf[..self.n]);
            let _ = f.flush();
        }
    }
}

/// 一行落盘。`mode` 区分**自然命中**与**我们主动触发**（楼660：账上必须能分开，
/// 否则将来看到一条自锁行，分不清是不是自己制造的）。
fn write_line(
    site: &'static str,
    arc: &'static str,
    tok: u64,
    holder: u64,
    verdict: &str,
    mode: &str,
    anchors: Anchors,
) {
    let mut l = Line::new();
    l.push(b"mono_ms=");
    l.push_u64(mono_ms());
    l.push(b" ");
    l.push(b"probe=lock mode=");
    l.push(mode.as_bytes());
    l.push(b" site=");
    l.push(site.as_bytes());
    l.push(b" arc=");
    l.push(arc.as_bytes());
    l.push(b" tok=");
    l.push_u64(tok);
    l.push(b" holder=");
    l.push_u64(holder);
    l.push(b" verdict=");
    l.push(verdict.as_bytes());
    // 因果锚（楼665）：两个都是绘制路径上的**局部量**，不取任何锁
    l.push(b" web_on_top=");
    l.push(if anchors.web_on_top { b"1" } else { b"0" });
    l.push(b" ai_snap=");
    l.push(if anchors.ai_snap { b"1" } else { b"0" });
    l.push(b"\n");
    l.flush();
}

/// 探针守卫：`enter` 写 begin（此处以「登记尝试」代替时间戳，避免取时钟锁），
/// 正常路径 `acquired` 登记 holder 并在 `Drop` 写 end；**自锁路径故意不写 end**
/// ——「有 begin 无 end 且 verdict=self」就是判据，不是漏写（楼653 契约）。
pub struct ProbeGuard {
    pub site: &'static str,
    pub arc: &'static str,
    pub tok: u64,
    anchors: Anchors,
    /// 是否真的登记上了 holder（自锁时走不到 acquired，于是 Drop 不写 end）
    acquired: bool,
    ended: bool,
}

/// 帧外／临界区前调用。`tok` 必须来自 `prefetch_token()`（帧外取好）。
pub fn enter(site: &'static str, arc: &'static str, tok: u64, anchors: Anchors) -> ProbeGuard {
    let holder = holder_of(arc);
    let v = classify(holder, tok);
    match v {
        Verdict::SelfDeadlock => write_line(site, arc, tok, holder, "self", MODE_NATURAL, anchors),
        Verdict::Contend => write_line(site, arc, tok, holder, "contend", MODE_NATURAL, anchors),
        Verdict::Unowned => {}
    }
    ProbeGuard {
        site,
        arc,
        tok,
        anchors,
        acquired: false,
        ended: false,
    }
}

impl ProbeGuard {
    /// try_lock 成功（或已知可进）后调用：登记 holder
    pub fn acquired(&mut self) {
        set_holder(self.arc, self.tok);
        self.acquired = true;
    }
}

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        // 只有**真的登记过 holder** 的守卫才写 end。自锁路径走不到 acquired，
        // 因此即使此时 holder 恰等于自己，也不许补 end——缺 end 正是自锁判据
        // （楼653：这条是故意的，不是漏写）。
        if !self.ended && self.acquired && holder_of(self.arc) == self.tok {
            clear_holder(self.arc);
            write_line(
                self.site,
                self.arc,
                self.tok,
                0,
                "end",
                MODE_NATURAL,
                self.anchors,
            );
            self.ended = true;
        }
    }
}

/// 验收用：读回日志行数（宿主/夹具用，不进死锁路径）
pub fn log_path_read(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}
