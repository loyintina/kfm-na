//! logcap.rs — 日志限流（BAR-171 日志面）：同款报错不许刷屏
//!
//! 判据（A 档纯函数 should_emit）：每 key 前 3 条直发；其后距上次
//! 出闸不满 60s 的同款压制，下一条出闸时带压制计数。fd 枯竭期
//! accept 空转 99108 行 / 日志 29GB（2026-09-28 晨实录）不许再发生——
//! 日志面同吃「回收纪律」：可见性留给第一现场与每分钟汇总，不留给洪水。
//!
//! fail-open：表满（64 key）时直发不压——限流器自己不许成为丢现场
//! 的病。调用方拿返回值（true = 已出闸）做派生判据也行，忽略也行。

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

/// 出闸判据（A 档纯函数）：前 3 次必发；之后 60s 内同款压制。
/// count = 该 key 累计出现次数（含本次，从 1 起）。
pub fn should_emit(count: u64, elapsed_s: u64) -> bool {
    count <= 3 || elapsed_s >= 60
}

struct Entry {
    count: u64,
    suppressed: u64,
    last_emit: Instant,
}

static TABLE: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

const MAX_KEYS: usize = 64;

fn lock_table() -> MutexGuard<'static, Option<HashMap<String, Entry>>> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 同款限流打印：key 归并（调用方给稳定 key，别把端口/秒数塞进去），
/// line 原样出闸；压制后的首次出闸带「同款压制 N 条」尾巴。
/// 返回 true = 本次出闸。
pub fn throttled(key: &str, line: &str) -> bool {
    let mut g = lock_table();
    let table = g.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    let Some(e) = table.get_mut(key) else {
        if table.len() >= MAX_KEYS {
            eprintln!("{line}"); // fail-open：表满不压
            return true;
        }
        table.insert(
            key.to_string(),
            Entry {
                count: 1,
                suppressed: 0,
                last_emit: now,
            },
        );
        eprintln!("{line}");
        return true;
    };
    e.count += 1;
    if should_emit(e.count, now.duration_since(e.last_emit).as_secs()) {
        if e.suppressed > 0 {
            eprintln!("{line}（同款压制 {} 条）", e.suppressed);
        } else {
            eprintln!("{line}");
        }
        e.suppressed = 0;
        e.last_emit = now;
        true
    } else {
        e.suppressed += 1;
        false
    }
}
