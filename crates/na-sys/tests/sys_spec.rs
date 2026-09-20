//! crates/na-sys/tests/sys_spec.rs — A 档考题：环境体征解析与格式
//!
//! 答案区：crates/na-sys/src/lib.rs。本文件是考题，不许改。
//!
//! 变异抽检：①parse_loadavg 把 l5/l15 顺序颠倒必须咬；②fmt_usage
//! 漏算百分比必须咬；③parse_meminfo 把 MemAvailable 当 MemTotal 用
//! 必须咬；④collect 把磁盘路的失败连坐到内存路必须咬（2026-09-20
//! Android SELinux 拒 /proc/loadavg 实锤：每路独立显形是硬需求）。

use na_sys::{LoadAvg, fmt_bytes, fmt_load, fmt_usage, parse_loadavg, parse_meminfo};

const LOADAVG: &str = "0.42 0.38 0.35 2/123 4567\n";

const MEMINFO: &str = "MemTotal:       16384000 kB
MemFree:         1024000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
Cached:          2048000 kB
SwapTotal:       4096000 kB
SwapFree:        4096000 kB
";

#[test]
fn spec_loadavg_三段() {
    let l = parse_loadavg(LOADAVG).unwrap();
    assert_eq!(
        l,
        LoadAvg {
            l1: 0.42,
            l5: 0.38,
            l15: 0.35
        }
    );
    assert_eq!(fmt_load(&l), "0.42 0.38 0.35");
}

#[test]
fn spec_loadavg_坏件显形() {
    assert!(parse_loadavg("").is_err());
    assert!(parse_loadavg("0.1 0.2").is_err(), "缺 l15 必须报错");
    assert!(parse_loadavg("a b c").is_err(), "非数字必须报错");
}

#[test]
fn spec_meminfo_双行() {
    let m = parse_meminfo(MEMINFO).unwrap();
    assert_eq!(m.total_kb, 16384000);
    assert_eq!(m.avail_kb, 8192000, "可用 = MemAvailable 不是 MemFree");
    // 已用 = total - avail
    assert_eq!(m.total_kb - m.avail_kb, 8192000);
}

#[test]
fn spec_meminfo_缺行显形() {
    assert!(
        parse_meminfo("MemFree: 1 kB\n").is_err(),
        "缺 MemTotal 必须报错"
    );
    assert!(
        parse_meminfo("MemTotal: 16384000 kB\n").is_err(),
        "缺 MemAvailable 必须报错——不许静默当零"
    );
}

#[test]
fn spec_fmt_bytes_档位() {
    assert_eq!(fmt_bytes(512), "512B");
    assert_eq!(fmt_bytes(1024), "1K");
    assert_eq!(fmt_bytes(512 * 1024), "512K");
    assert_eq!(fmt_bytes(1024 * 1024), "1M");
    assert_eq!(fmt_bytes(512 * 1024 * 1024), "512M");
    assert_eq!(fmt_bytes(1024 * 1024 * 1024), "1.0G");
    assert_eq!(fmt_bytes(16384000u64 * 1024), "15.6G");
}

#[test]
fn spec_fmt_usage_合成() {
    assert_eq!(fmt_usage(8192000 * 1024, 16384000 * 1024), "7.8G/15.6G 50%");
    assert_eq!(fmt_usage(0, 0), "—", "total=0 除零防线");
    assert_eq!(fmt_usage(0, 1024), "0B/1K 0%");
}

#[test]
fn spec_collect_本机活件() {
    // B 档冒烟：collect 永不整组失败——每路独立显形（Android SELinux
    // 拒 /proc/loadavg 是合法常态，None 一路不许塌全组）
    let s = na_sys::collect("/");
    if let Some(m) = s.mem {
        assert!(m.total_kb > 0);
        assert!(m.avail_kb <= m.total_kb);
    }
    if let Some((total, avail)) = s.disk {
        assert!(total > 0);
        assert!(avail <= total);
    }
    if let Some(l) = s.load {
        assert!(l.l1 >= 0.0);
    }
    // 内存路在两个 chain 环境（服务器/手机 Termux）都真可读，钉住
    assert!(s.mem.is_some(), "meminfo 在 chain 环境必须可读");
}

#[test]
fn spec_collect_坏件显形() {
    // 磁盘路指向不存在的路径：只许 disk 显形 None，不许连坐其他路
    let s = na_sys::collect("/no/such/path-kfm-na-spec");
    assert!(s.disk.is_none(), "statvfs 失败必须显形 None");
    assert!(s.mem.is_some(), "磁盘路坏了不许连坐内存路");
}
