//! crates/na-sys/tests/sys_spec.rs — A 档考题：环境体征解析与格式
//!
//! 答案区：crates/na-sys/src/lib.rs。本文件是考题，不许改。
//!
//! 变异抽检：①parse_loadavg 把 l5/l15 顺序颠倒必须咬；②fmt_usage
//! 漏算百分比必须咬；③parse_meminfo 把 MemAvailable 当 MemTotal 用
//! 必须咬；④collect 把磁盘路的失败连坐到内存路必须咬（2026-09-20
//! Android SELinux 拒 /proc/loadavg 实锤：每路独立显形是硬需求）；
//! ⑤parse_loadavg 把 procs 的 running/total 颠倒必须咬；⑥fmt_uptime
//! 漏「天」档必须咬；⑦SwapFree 被当 Swap 已用量必须咬。

use na_sys::{
    LoadAvg, fmt_bytes, fmt_load, fmt_uptime, fmt_usage, parse_loadavg, parse_meminfo, parse_uptime,
};

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
            l15: 0.35,
            procs: Some((2, 123))
        }
    );
    assert_eq!(fmt_load(&l), "0.42 0.38 0.35");
}

#[test]
fn spec_loadavg_procs_逐路显形() {
    // 第 4 段 "2/123"：正常解析 → Some((running, total))
    assert_eq!(parse_loadavg(LOADAVG).unwrap().procs, Some((2, 123)));
    // 第 4 段缺失/坏件：procs 显形 None，前三段不许连坐
    let l = parse_loadavg("0.1 0.2 0.3\n").unwrap();
    assert_eq!(l.procs, None);
    let l = parse_loadavg("0.1 0.2 0.3 garbage 4567\n").unwrap();
    assert_eq!(l.procs, None);
    assert!(
        (l.l1 - 0.1).abs() < f64::EPSILON,
        "procs 坏了不许连坐负载三段"
    );
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
fn spec_meminfo_swap_逐路显形() {
    // SwapTotal/SwapFree 双行都在 → Some((total, free))
    let m = parse_meminfo(MEMINFO).unwrap();
    assert_eq!(m.swap, Some((4096000, 4096000)));
    // 双行缺一：swap 显形 None，内存主路不许连坐（缺 SwapTotal
    // 不是错——MemTotal/MemAvailable 缺行才是 err，契约不动）
    let m = parse_meminfo("MemTotal: 16384000 kB\nMemAvailable: 8192000 kB\n").unwrap();
    assert_eq!(m.swap, None);
    let m =
        parse_meminfo("MemTotal: 16384000 kB\nMemAvailable: 8192000 kB\nSwapTotal: 4096000 kB\n")
            .unwrap();
    assert_eq!(m.swap, None, "SwapFree 缺席 = swap 整路 None，不许编造");
    assert_eq!(m.total_kb, 16384000, "swap 缺行不许连坐主路");
    // 交换已用 = total - free（变异⑦：拿 free 当 used 必须咬）
    let m = parse_meminfo(
        "MemTotal: 16384000 kB\nMemAvailable: 8192000 kB\nSwapTotal: 4096000 kB\nSwapFree: 1024000 kB\n",
    )
    .unwrap();
    let (t, f) = m.swap.unwrap();
    assert_eq!(t - f, 3072000);
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
    // uptime 不钉必须 Some：手机 Termux 拒 /proc/uptime（EACCES 实锤
    // 2026-09-20），服务器可读——显形差异本身就是契约
    if let Some(u) = s.uptime_s {
        assert!(u > 0, "开机秒数必须为正");
    }
}

#[test]
fn spec_parse_uptime_与_fmt_uptime() {
    // /proc/uptime: "7849375.29 31234482.71"（首 token = 开机秒）
    assert_eq!(parse_uptime("7849375.29 31234482.71\n").unwrap(), 7849375);
    assert!(parse_uptime("").is_err(), "空必须报错");
    assert!(parse_uptime("abc 1.0\n").is_err(), "非数字必须报错");
    // 文案档位：>1天 "{d}天{h}时"，>1时 "{h}时{m}分"，否则 "{m}分"（不补零）
    assert_eq!(fmt_uptime(45), "0分");
    assert_eq!(fmt_uptime(90 * 60), "1时30分");
    assert_eq!(fmt_uptime(3 * 3600 + 5 * 60 + 12), "3时5分");
    assert_eq!(fmt_uptime(2 * 86400 + 7 * 3600), "2天7时");
    assert_eq!(fmt_uptime(90 * 86400 + 23 * 3600 + 59 * 60), "90天23时");
}

#[test]
fn spec_collect_坏件显形() {
    // 磁盘路指向不存在的路径：只许 disk 显形 None，不许连坐其他路
    let s = na_sys::collect("/no/such/path-kfm-na-spec");
    assert!(s.disk.is_none(), "statvfs 失败必须显形 None");
    assert!(s.mem.is_some(), "磁盘路坏了不许连坐内存路");
}

#[test]
fn spec_collect_核数显形() {
    // 核数（2026-09-21 负载判色）：采集侧自报本机核数——两台机器各报
    // 自己的，绝不写死设备常量。不可用的平台 = None（客户端回退窗峰归一）
    let s = na_sys::collect("/");
    let c = s
        .cores
        .expect("Linux/Android 上 available_parallelism 必有值");
    assert!(c > 0, "核数下限 1");
    assert!(
        c <= 1024,
        "核数上限护栏（防采集侧读到垃圾值把占比归一除歪）"
    );
}
