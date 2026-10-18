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
    // BAR-273 第二批（楼211 悬铃裁）：**成对带两维**——arch／什么机／哪一台／pid。
    assert_eq!(
        parts.len(),
        4,
        "两维成对：必须四段 arch/what/which/pid: {inner}"
    );
    let (arch, dev, which, pid) = (parts[0], parts[1], parts[2], parts[3]);
    assert!(!arch.is_empty(), "arch 不许为空");
    assert!(
        !dev.is_empty(),
        "设备位恒非空（退化链兜底不许留空）: {inner}"
    );
    // BAR-273 第二批（楼193 悬铃裁）：dev 段形态改为「值@来源档」。值仍只许
    // 字母数字与 -/_（进 grep 面与 JSON 文本），档名进白名单——`@` 只许出现
    // 一次，且必须是分隔符，不许出现在值里。
    let (dev_val, dev_tier) = {
        let mut it = dev.split('@');
        let v = it.next().unwrap_or("");
        let t = it.next().unwrap_or("");
        assert!(it.next().is_none(), "来源档分隔符 @ 只许一次: {dev}");
        (v, t)
    };
    assert!(
        dev_val
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
        "设备位值只许字母数字与 -/_（进 grep 面与 JSON 文本）: {dev}"
    );
    assert!(
        dev_val.chars().count() <= 12,
        "设备位值截 12 字符上限: {dev}"
    );
    assert!(
        ["model", "human", "env", "unknown"].contains(&dev_tier),
        "第一维（什么机）档名必须是 model/human/env/unknown 之一: {dev}"
    );
    // 第二维（哪一台）只有 devid 与 unknown 两档——它说的不是「什么机」
    let (which_val, which_tier) = {
        let mut it = which.split('@');
        let v = it.next().unwrap_or("");
        let t = it.next().unwrap_or("");
        assert!(it.next().is_none(), "第二维 @ 只许一次: {which}");
        (v, t)
    };
    assert!(
        !which_val.is_empty(),
        "第二维恒非空（两维成对，不许缺一维）: {inner}"
    );
    assert!(
        ["devid", "unknown"].contains(&which_tier),
        "第二维档名只能是 devid/unknown: {which}"
    );
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

#[test]
fn spec_bar273_来源档判别_纯函数全套() {
    // BAR-273 第二批（楼203／楼211）：五档枚举与「谁能跨机采信」是判卷第一道
    // 分辨闸，全走纯函数，A 档可判。变异抽检：把 tier_trusted 放宽到 devid、
    // 或把 human_source_ok 的与门改成或门 → 本钉必红。
    use kfm_na::report::{
        DEV_TIERS, TIER_DEVID, TIER_ENV, TIER_HUMAN, TIER_MODEL, TIER_UNKNOWN, env_pin_allowed,
        env_pin_provenance, human_source_ok, human_trusted, tier_of, tier_trusted_cross_machine,
        with_tier,
    };

    // ① 五档枚举齐全，且常量与枚举表同源
    assert_eq!(DEV_TIERS.len(), 5, "来源档必须五档");
    for t in [TIER_MODEL, TIER_HUMAN, TIER_DEVID, TIER_ENV, TIER_UNKNOWN] {
        assert!(DEV_TIERS.contains(&t), "枚举表缺档 {t}");
    }

    // ② 拼与拆（拆只认第一个 @；旧行无 @ ⇒ 空档＝无档）
    assert_eq!(with_tier("Neo11", TIER_MODEL), "Neo11@model");
    assert_eq!(tier_of("Neo11@model"), TIER_MODEL);
    assert_eq!(tier_of("Neo11"), "", "旧行无档必须解成空串，不许猜档");
    assert_eq!(tier_of("a@b@c"), "b@c", "只按第一个 @ 拆，后半原样交出");

    // ③ 只凭 tag 能跨机采信的只有 model（楼200／203）
    assert!(tier_trusted_cross_machine(TIER_MODEL), "model 可跨机");
    for t in [TIER_HUMAN, TIER_DEVID, TIER_ENV, TIER_UNKNOWN, ""] {
        assert!(!tier_trusted_cross_machine(t), "{t:?} 不许凭 tag 跨机采信");
    }

    // ④ human 出处三样：与关系，缺一不认（楼202／203）
    assert!(human_source_ok(
        "承影",
        "2026-10-19 06:40 +08:00",
        "dumpsys"
    ));
    assert!(!human_source_ok("", "2026-10-19 06:40 +08:00", "dumpsys"));
    assert!(!human_source_ok("承影", "  ", "dumpsys"));
    assert!(!human_source_ok("承影", "2026-10-19 06:40 +08:00", ""));
    assert!(human_trusted("承影", "t", "how"));
    assert!(!human_trusted("承影", "", "how"));

    // ⑤ 生产拒绝 env 改写身份（楼199）
    assert!(env_pin_allowed(true), "调试构建可用手钉");
    assert!(!env_pin_allowed(false), "生产构建必须拒绝手钉身份");

    // ⑥ 手钉留痕：非 env 档不产行；env 档必有行，且缺 WHY 时写明「未留痕」
    assert!(env_pin_provenance("Neo11@model").is_none());
    let line = env_pin_provenance("Neo11@env").expect("env 档必须留痕");
    assert!(line.contains("env-pin"), "留痕行要标档名: {line}");
    assert!(
        line.contains("留痕") && (line.contains("谁·为何")),
        "留痕行必须含谁/为何位: {line}"
    );
}
