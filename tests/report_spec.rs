//! 报表考题（A 档）：设备/实例标识（BAR-134 立 · BAR-235 加设备维）——
//! field-reports.log 是多设备混流，行尾 [arch/dev/pid] 是判读第一分道闸。
//! 纯逻辑钉死格式与取值，变异抽检：摘掉 stamp／去设备位／消毒放行空格
//! 必须咬。

use kfm_na::report::{parse_prop_key, sanitize_dev, stamp_msg};

#[test]
fn spec_bar235_报表行_挂设备维三段() {
    // 1. 原消息完整保留在前缀
    let out = stamp_msg("隧道 Up");
    assert!(out.starts_with("隧道 Up "), "原消息必须在前: {out}");
    // 2. 行尾是 [arch/dev/pid] 形态：方括号 + 两斜杠 + 数字 pid
    let tail = out.strip_prefix("隧道 Up ").unwrap();
    assert!(
        tail.starts_with('[') && tail.ends_with(']'),
        "标识必须方括号包裹: {tail}"
    );
    let inner = &tail[1..tail.len() - 1];
    let parts: Vec<&str> = inner.split('/').collect();
    assert_eq!(parts.len(), 3, "BAR-235 后必须三段 arch/dev/pid: {inner}");
    let (arch, dev, pid) = (parts[0], parts[1], parts[2]);
    assert!(!arch.is_empty(), "arch 不许为空");
    assert!(
        !dev.is_empty(),
        "设备位恒非空（退化链兜底不许留空）: {inner}"
    );
    assert!(
        dev.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
        "设备位只许字母数字与 -/_（进 grep 面与 JSON 文本）: {dev}"
    );
    assert!(dev.chars().count() <= 12, "设备位截 12 字符上限: {dev}");
    assert!(
        !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()),
        "pid 必须是数字: {pid}"
    );
    // 3. pid 必须是本进程真 pid（多实例分道靠它）
    assert_eq!(pid, std::process::id().to_string());
    // 4. arch 必须等于编译目标（同 arch 多机分道靠设备位）
    assert_eq!(arch, std::env::consts::ARCH);
    // 5. 空消息也挂（心跳类短行不豁免）
    let out2 = stamp_msg("");
    assert!(out2.starts_with('['), "空消息标识直接打头: {out2}");
}

#[test]
fn spec_bar235_双机分道_设备位可区分() {
    // 本案病灶（2026-10-07）：两台同 arch 真机混流后与「单机双实例」同形。
    // 钉：同一 pid 段、不同设备位 ⇒ 标识串必须不同（分道闸成立）
    let a = sanitize_dev("Neo11");
    let b = sanitize_dev("iQOO Neo 9");
    assert_ne!(a, b, "不同机型消毒后必须仍可区分: {a} vs {b}");
    assert_eq!(a, "Neo11");
    assert_eq!(b, "iQOONeo9", "空白剥掉、字母数字留存");
    let tag_a = format!("[aarch64/{a}/1234]");
    let tag_b = format!("[aarch64/{b}/1234]");
    assert_ne!(tag_a, tag_b, "同 pid 异机必须两串不同");
}

#[test]
fn spec_bar235_机型解析_只认目标键与剜行内注释() {
    let props = "# ro.product.model=COMMENTED\nro.product.other=x\nro.product.model=Neo11 \nro.product.model=后一条\n";
    assert_eq!(
        parse_prop_key(props, "ro.product.model").as_deref(),
        Some("Neo11"),
        "注释与其它键必须跳过，值去空白"
    );
    // 缺键 / 空值 / 畸形行一律 None（调用方继续退化链）
    assert_eq!(
        parse_prop_key("ro.product.model=\n", "ro.product.model"),
        None
    );
    assert_eq!(parse_prop_key("nothing here", "ro.product.model"), None);
    assert_eq!(parse_prop_key("no equals sign", "ro.product.model"), None);
    assert_eq!(parse_prop_key("", "ro.product.model"), None);
    // 行内注释必须剜掉（否则机型串会吞进 "Neo11vivo" 这类尾巴）
    assert_eq!(
        parse_prop_key("ro.product.model=Neo11 # vivo\n", "ro.product.model").as_deref(),
        Some("Neo11")
    );
    // 值全被注释吃掉 = 未命中（继续退化链）
    assert_eq!(
        parse_prop_key("ro.product.model= # only comment\n", "ro.product.model"),
        None
    );
}

#[test]
fn spec_bar235_设备位消毒_去空白标点并截断() {
    assert_eq!(sanitize_dev("Neo 11"), "Neo11");
    assert_eq!(
        sanitize_dev("iQOO Neo11 (V2332A)"),
        "iQOONeo11V23",
        "标点剥掉且截 12"
    );
    assert_eq!(sanitize_dev("   "), "", "全空白 = 不可用（调用方降档）");
    assert_eq!(sanitize_dev("!!!"), "", "全标点 = 不可用");
    assert_eq!(sanitize_dev("a-b_c9"), "a-b_c9", "-/_ 属合法字符");
    // 中文机型名不被剥光（is_alphanumeric 对 CJK 为真），但空白剥掉
    assert_eq!(sanitize_dev("华为 Mate 60"), "华为Mate60");
}

#[test]
fn spec_bar134_标识_转义后仍存活() {
    // 落盘链：msg 先进 escape_json 再进 JSON——标识是纯 ASCII，必须原样穿过
    let msg = stamp_msg("quote\"and\\slash");
    let body = format!(
        "{{\"stage\":\"{}\",\"msg\":\"{}\"}}",
        kfm_na::report::escape_json("stage"),
        kfm_na::report::escape_json(&msg)
    );
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let got = v["msg"].as_str().unwrap();
    assert!(
        got.starts_with("quote\"and\\slash ["),
        "转义不得伤消息体: {got}"
    );
    assert!(got.ends_with(']'), "标识必须在行尾: {got}");
}

#[test]
fn spec_bar139_报表走自持隧道() {
    // BAR-139：报表必须走 na 自持隧道（9021），不许回 8021（Termux 代维，
    // 一冻结报表即断供——2026-09-23 断更 26 分钟实录）
    use kfm_na::report::{HOST_HEADER, PATH, SERVER_ADDR};
    assert_eq!(SERVER_ADDR.port(), 9021, "报表口必须是自持隧道 9021");
    assert!(
        SERVER_ADDR.ip().is_loopback(),
        "必须打本机回环: {SERVER_ADDR}"
    );
    assert!(HOST_HEADER.contains("9021"), "Host 头随端口: {HOST_HEADER}");
    assert!(PATH.ends_with("/api/na-report"), "路径不许漂: {PATH}");
}
