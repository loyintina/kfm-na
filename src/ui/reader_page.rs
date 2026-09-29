//! ui/reader_page.rs — 阅读页状态核 + 几何单源（BAR-170，2026-09-27 用户拍板
//! 「阅读页跟文件树是分开的两个插件」研究线亲做；契约档 docs/active/阅读页.md）。
//!
//! 分层：本册 = 纯逻辑（A 档考题 tests/reader_page_spec.rs）——分块账/字界
//! 前进/滚动钳/记忆恢复全在这里钉死；涂装在 termview（页环+顶栏+正文吃
//! BAR-169 md 管线），壳在 android_app（手势/取数/公民接线）。块几何与
//! 状态只读本册，不许各算。
//!
//! 账的本体三条（改前先读）：
//! ① **consumed ≠ text.len()**——服务端 lossy 显形时出参文本字节数 ≠ 源
//!    字节数，下一块的 offset 只能走服务端回执的 next_offset（原始侧计量）。
//! ② **到位不空涨代际**——scroll 没变就不许 epoch+1（脏帧 sig 吃 epoch，
//!    空涨 = 整页白烘）。
//! ③ **滚动记忆只进不出**——close/换文件回写，open 恢复；帽满摘字典序
//!    最小（filetree MEM_CAP 同款确定性策略）。

use std::collections::HashMap;

/// 顶栏高（物理 px，与文件树底栏 FT_BAR_H 同尺配方）
pub const TOP_BAR_H: i64 = 110;
/// 返回钮宽（物理 px，文件树底栏 × 盒 FT_CLOSE_W 同尺）
pub const RETURN_W: i64 = 130;
/// 进度线厚（物理 px；贴顶栏底缘下，accent 渐变）
pub const PROGRESS_H: i64 = 3;
/// 每块请求字节数（对齐服务端 fsapi::DEFAULT_MAX）
pub const CHUNK_MAX: usize = 65536;
/// 总量帽（块）：32 × 64KB = 2MB，到帽置 eof + capped（页脚显形）
pub const MAX_CHUNKS: usize = 32;
/// 滚动记忆帽（条）
pub const MEM_CAP: usize = 128;

/// 文件相（打开时按扩展名定，终身不变）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderKind {
    Md,
    Plain,
}

/// 阅读阶段（占位相与可读相分离——涂装按相出墨）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderPhase {
    /// 首块在途
    Loading,
    /// 正文可读（续块可能还在路上）
    Reading,
    /// 二进制拒读占位
    Binary,
    /// 读失败占位（文案自带）
    Error(String),
}

/// 扩展名定相：.md/.markdown（大小写不敏感）= Md，其余 = Plain
pub fn kind_of(path: &str) -> ReaderKind {
    let lower = path.to_lowercase();
    if lower.ends_with(".md") || lower.ends_with(".markdown") {
        ReaderKind::Md
    } else {
        ReaderKind::Plain
    }
}

/// 滚动上限（i64 域；内容矮于视口 = 0）
pub fn scroll_max(total_h: i64, view_h: i64) -> i64 {
    (total_h - view_h).max(0)
}

#[derive(Debug)]
pub struct ReaderPage {
    pub path: String,
    pub name: String,
    pub kind: ReaderKind,
    pub phase: ReaderPhase,
    /// 已取块拼接（md 排版的全量输入）
    pub text: String,
    /// 源字节消费量（下一块的 offset——本体①，不许拿 text.len() 顶）
    consumed: u64,
    /// 已取块数（帽账）
    chunks: usize,
    /// 服务端说没有了 或 到帽
    pub eof: bool,
    /// 到 MAX_CHUNKS 帽（页脚「文件过大只显示前 2MB」显形）
    pub capped: bool,
    /// 有块在途（首块或续块）
    pub loading: bool,
    pub scroll: i64,
    /// 待恢复滚动（open 时从 mem 取出；内容长到包得住才落——见 tick_restore）
    pending_restore: Option<i64>,
    /// 每路径滚动记忆（本体③）
    mem: HashMap<String, i64>,
    pub epoch: u64,
}

impl Default for ReaderPage {
    fn default() -> Self {
        Self::new()
    }
}

impl ReaderPage {
    pub fn new() -> Self {
        Self {
            path: String::new(),
            name: String::new(),
            kind: ReaderKind::Plain,
            phase: ReaderPhase::Loading,
            text: String::new(),
            consumed: 0,
            chunks: 0,
            eof: false,
            capped: false,
            loading: false,
            scroll: 0,
            pending_restore: None,
            mem: HashMap::new(),
            epoch: 0,
        }
    }

    /// 打开文件（文件树点击改道的唯一入口）：旧路径滚动回写 mem → 全清
    /// → 从 mem 取恢复值（挂 pending，内容长出来才落）。**首块请求由壳
    /// 据此后发**（`next_request` 会给 Some(0)）。
    pub fn open(&mut self, path: String, name: String) {
        self.writeback_mem();
        self.path = path;
        self.name = name;
        self.kind = kind_of(&self.path);
        self.phase = ReaderPhase::Loading;
        self.text.clear();
        self.consumed = 0;
        self.chunks = 0;
        self.eof = false;
        self.capped = false;
        self.loading = false;
        self.scroll = 0;
        self.pending_restore = self.mem.get(&self.path).copied();
        self.epoch += 1;
    }

    /// 关闭（面板推回）：滚动回写 mem，清空本体（mem 留住）
    pub fn close(&mut self) {
        self.writeback_mem();
        let mem = std::mem::take(&mut self.mem);
        let epoch = self.epoch;
        *self = Self::new();
        self.mem = mem;
        self.epoch = epoch + 1;
    }

    /// 回写当前路径滚动进 mem（空路径不写；帽满摘字典序最小——确定性，
    /// 与 filetree MEM_CAP 同款）
    fn writeback_mem(&mut self) {
        if self.path.is_empty() {
            return;
        }
        if !self.mem.contains_key(&self.path)
            && self.mem.len() >= MEM_CAP
            && let Some(victim) = self.mem.keys().min().cloned()
        {
            self.mem.remove(&victim);
        }
        self.mem.insert(self.path.clone(), self.scroll);
    }

    /// 文本块回执：账连续性守卫（offset 必须等于已消费量，否则块序错账
    /// 落 Error）；eof = 服务端 truncated=false；到帽强制 eof + capped
    pub fn apply_chunk(&mut self, offset: u64, next_offset: u64, truncated: bool, text: &str) {
        if offset != self.consumed {
            self.phase = ReaderPhase::Error(format!(
                "块序错账（期望 {}/来到 {}）——重新打开试试",
                self.consumed, offset
            ));
            self.loading = false;
            self.eof = true;
            self.epoch += 1;
            return;
        }
        self.text.push_str(text);
        self.consumed = next_offset;
        self.chunks += 1;
        self.loading = false;
        self.phase = ReaderPhase::Reading;
        self.eof = !truncated;
        if self.chunks >= MAX_CHUNKS {
            self.eof = true;
            self.capped = true;
        }
        self.epoch += 1;
    }

    /// 缓存先画（BAR-187）：本地缓存副本立即上屏，eof 置真挡住续块请求
    /// （缓存只知全文不知源字节账，consumed 归零等 refresh 重建）；后台
    /// refresh 首块走 `apply_refresh0` 换芯。text 由调用方带声明头
    pub fn apply_cached(&mut self, text: &str) {
        self.text = text.to_string();
        self.consumed = 0;
        self.chunks = 0;
        self.eof = true;
        self.capped = false;
        self.loading = false;
        self.phase = ReaderPhase::Reading;
        self.epoch += 1;
    }

    /// 后台换芯首块（BAR-187）：缓存视图在屏时 refresh 的 offset=0 回执——
    /// 重置正文与块账（声明头随之摘除）后按正常块入账；**只在缓存先画后**
    /// 合法（否则与 apply_chunk 重复入账），后续续块照常走 apply_chunk
    pub fn apply_refresh0(&mut self, next_offset: u64, truncated: bool, text: &str) {
        self.text = text.to_string();
        self.consumed = next_offset;
        self.chunks = 1;
        self.loading = false;
        self.phase = ReaderPhase::Reading;
        self.eof = !truncated;
        self.capped = false;
        self.epoch += 1;
    }

    /// 后台换芯失败（BAR-187）：缓存视图留场不动，只摘在途账（不摘会让
    /// next_request 永远 None 卡死后续操作）
    pub fn refresh_failed(&mut self) {
        if self.loading {
            self.loading = false;
            self.epoch += 1;
        }
    }

    /// 二进制回执（服务端 NUL 探测）：拒读占位
    pub fn apply_binary(&mut self) {
        self.phase = ReaderPhase::Binary;
        self.loading = false;
        self.eof = true;
        self.epoch += 1;
    }

    /// 读失败回执（网络/404/出参不认）
    pub fn apply_error(&mut self, msg: &str) {
        self.phase = ReaderPhase::Error(msg.to_string());
        self.loading = false;
        self.eof = true;
        self.epoch += 1;
    }

    /// 下一块请求的 offset（壳每帧/每次滚动后一问）：首块 = Some(0)；
    /// 续块 = Some(consumed)。占位相/eof/在途 = None
    pub fn next_request(&self) -> Option<u64> {
        if self.eof || self.loading {
            return None;
        }
        match self.phase {
            ReaderPhase::Loading | ReaderPhase::Reading => Some(self.consumed),
            _ => None,
        }
    }

    /// 壳发请求前记账（防同块并发）
    pub fn mark_loading(&mut self) {
        self.loading = true;
        self.epoch += 1;
    }

    /// 预取判定：滚动进内容尾 1.5 视口内 → 取下一块（内容不足 1.5 视口
    /// 时首块后连取——开门就是满满一屏半，不用等用户滚）
    pub fn need_prefetch(&self, total_h: i64, view_h: i64) -> Option<u64> {
        if view_h <= 0 {
            return None;
        }
        if self.scroll + view_h * 3 / 2 >= total_h {
            self.next_request()
        } else {
            None
        }
    }

    /// 恢复滚动：内容包得住恢复值 或 已到 eof（包不住也得落，钳在 max
    /// 内）才消费 pending——落一次即摘，不重复落
    pub fn tick_restore(&mut self, total_h: i64, view_h: i64) {
        let Some(r) = self.pending_restore else {
            return;
        };
        let max = scroll_max(total_h, view_h);
        if r <= max || self.eof {
            let to = r.min(max);
            if to != self.scroll {
                self.scroll = to;
                self.epoch += 1;
            }
            self.pending_restore = None;
        }
    }

    /// 滚动（眼手同尺单源）：钳 [0, max]，**到位不空涨代际**（本体②）
    pub fn scroll_by(&mut self, d: i64, max: i64) {
        let to = (self.scroll + d).clamp(0, max.max(0));
        if to != self.scroll {
            self.scroll = to;
            self.epoch += 1;
        }
    }

    /// 进度线分子分母（max<=0 = 不画）
    pub fn progress(&self, max: i64) -> Option<(i64, i64)> {
        if max <= 0 {
            None
        } else {
            Some((self.scroll, max))
        }
    }

    /// 考题/壳探针：mem 现值
    pub fn mem_of(&self, path: &str) -> Option<i64> {
        self.mem.get(path).copied()
    }
}

static READER: std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<ReaderPage>>>> =
    std::sync::Mutex::new(None);

/// 注册共享状态核（壳启动时一次；host 不注册 = 页面空转合法）
pub fn register_reader(s: std::sync::Arc<std::sync::Mutex<ReaderPage>>) {
    *READER.lock().unwrap() = Some(s);
}

/// 取共享状态核（涂装/手势/取数三处同源）
pub fn reader_handle() -> Option<std::sync::Arc<std::sync::Mutex<ReaderPage>>> {
    READER.lock().unwrap().clone()
}
