//! na-sys — 环境体征探针（负载/内存/磁盘）。
//!
//! 通用性落法（2026-09-20 用户拍板）：体征描述的是「中央终端所在环境
//! 的自身状态」，与设备无关——na-server（服务器/任何机器）与 na 客户
//! 端（手机本地相）吃同一份解析/采集代码，行为在类型层面不可能漂移。
//!
//! 分层：parse_* / fmt_* 是 A 档纯逻辑（tests/sys_spec.rs 钉死）；
//! collect 是 B 档薄胶水（读 /proc/loadavg /proc/meminfo + statvfs），
//! 对错判据 = 系统让不让你读，无输入输出可判卷。

/// 负载（/proc/loadavg 前三段：1/5/15 分钟）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadAvg {
    pub l1: f64,
    pub l5: f64,
    pub l15: f64,
}

/// 内存（/proc/meminfo：总量与可用，已用 = total - avail）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    pub total_kb: u64,
    pub avail_kb: u64,
}

/// 环境体征全集（每路独立显形：None = 系统不给读——Android SELinux
/// 拒 /proc/loadavg 是合法常态（2026-09-20 手机 Termux 实锤 EACCES）；
/// 卡面显「—」，不许编造零值，一路塌不许连坐他路）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SysInfo {
    pub load: Option<LoadAvg>,
    pub mem: Option<MemInfo>,
    /// 磁盘 (总量, 可用) 字节（采集点 = collect 的 path）
    pub disk: Option<(u64, u64)>,
}

/// /proc/loadavg 解析（A 档）："0.42 0.38 0.35 2/123 4567" → 前三段
pub fn parse_loadavg(text: &str) -> Result<LoadAvg, String> {
    let mut it = text.split_whitespace();
    let mut take = |name: &str| -> Result<f64, String> {
        it.next()
            .ok_or_else(|| format!("loadavg 缺 {name}"))?
            .parse()
            .map_err(|_| format!("loadavg {name} 非数字"))
    };
    Ok(LoadAvg {
        l1: take("l1")?,
        l5: take("l5")?,
        l15: take("l15")?,
    })
}

/// /proc/meminfo 解析（A 档）：抓 MemTotal/MemAvailable 两行（kB），
/// 其余行不看；缺行即错（老内核无 MemAvailable——那已经是 2014 年前
/// 的世界，显形报错不许静默当零）
pub fn parse_meminfo(text: &str) -> Result<MemInfo, String> {
    let mut total = None;
    let mut avail = None;
    for line in text.lines() {
        let Some((k, rest)) = line.split_once(':') else {
            continue;
        };
        let v: Option<u64> = rest.trim().trim_end_matches("kB").trim().parse().ok();
        match k.trim() {
            "MemTotal" => total = v,
            "MemAvailable" => avail = v,
            _ => {}
        }
    }
    Ok(MemInfo {
        total_kb: total.ok_or("meminfo 缺 MemTotal")?,
        avail_kb: avail.ok_or("meminfo 缺 MemAvailable")?,
    })
}

/// 字节 → 紧凑文案（A 档）：<1K = "512B"，<1M = "512K"，<1G = "512M"，
/// 否则 "12.3G"（一位小数——体征卡的精度预算就这档）
pub fn fmt_bytes(b: u64) -> String {
    const K: u64 = 1024;
    const M: u64 = K * 1024;
    const G: u64 = M * 1024;
    if b < K {
        format!("{b}B")
    } else if b < M {
        format!("{}K", b / K)
    } else if b < G {
        format!("{}M", b / M)
    } else {
        format!("{:.1}G", b as f64 / G as f64)
    }
}

/// 用量文案（A 档）："6.2G/15.6G 40%"；total=0 = "—"（除零防线）
pub fn fmt_usage(used: u64, total: u64) -> String {
    if total == 0 {
        return "—".into();
    }
    format!(
        "{}/{} {}%",
        fmt_bytes(used),
        fmt_bytes(total),
        used.saturating_mul(100) / total
    )
}

/// 负载文案（A 档）："0.42 0.38 0.35"（两位小数，与 loadavg 原刊同尺）
pub fn fmt_load(l: &LoadAvg) -> String {
    format!("{:.2} {:.2} {:.2}", l.l1, l.l5, l.l15)
}

/// 采集（B 档薄胶水）：/proc 两文件 + statvfs(path)。disk_path 服务器
/// 用 "/"，手机本地相用 "/data"（调用方定，探针不揣度哪块盘要紧）。
/// 每路独立显形：单路失败归该路 None，不报错不连坐——坏件形态由
/// 卡面「—」呈现（Android 拒 /proc/loadavg 时负载路就是常态 None）
pub fn collect(disk_path: &str) -> SysInfo {
    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|t| parse_loadavg(&t).ok());
    let mem = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| parse_meminfo(&t).ok());
    let disk = statvfs_bytes(disk_path).ok();
    SysInfo { load, mem, disk }
}

/// statvfs → (总量, 可用) 字节（B 档）：f_blocks×f_frsize 与
/// f_bavail×f_frsize（bavail = 非特权可用——用户视角的「还剩多少」）
fn statvfs_bytes(path: &str) -> Result<(u64, u64), String> {
    let c = std::ffi::CString::new(path).map_err(|_| "路径含 NUL".to_string())?;
    // libc 的 statvfs 结构体布局随平台变，零初始化后整体传入是惯例
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return Err(format!(
            "statvfs {path} 失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok((
        st.f_blocks * st.f_frsize as u64,
        st.f_bavail * st.f_frsize as u64,
    ))
}
