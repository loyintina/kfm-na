//! lockprobe_spec — 锁探针（MAIN0192 楼596／601 定性、楼654-666 契约）
//!
//! 判卷维度：三分类纯逻辑、令牌唯一性、报行字段同形（含因果锚与单调钟）、
//! 自锁路径**故意不写 end**（这是判据，不是漏写）。

/// 这几枚共享全局状态（HOLDERS 槽 + LOG_FILE），必须串行跑
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

use kfm_na::lockprobe::{
    Anchors, Verdict, classify, clear_holder, enter, holder_of, init_log, log_path_read,
    prefetch_token, set_holder,
};

#[test]
fn spec_lockprobe_三分类真值表() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // 自锁的唯一判据是「命中持有者是自己」；holder 为 0 是「有人持着没登记」
    assert_eq!(classify(7, 7), Verdict::SelfDeadlock);
    assert_eq!(classify(7, 8), Verdict::Contend);
    assert_eq!(classify(0, 8), Verdict::Unowned);
    // 自己令牌为 0 的退化情形也不许误判成自锁
    assert_eq!(classify(0, 0), Verdict::Unowned);
}

#[test]
fn spec_lockprobe_令牌唯一且非零() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let a = prefetch_token();
    let b = prefetch_token();
    assert!(a > 0 && b > 0, "令牌从 1 起，0 保留给「无主」");
    assert_ne!(a, b, "令牌必须唯一（tid 会复用，不能用）");
}

#[test]
fn spec_lockprobe_报行字段同形且自锁无end() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("lockprobe-{}", std::process::id()));
    let path = dir.join("lockprobe.log");
    let _ = std::fs::remove_dir_all(&dir);
    init_log(&path);

    let arc = "test-arc";
    clear_holder(arc);

    // ① 正常路径：进临界区=登记, 出域=写 end
    {
        let tok = prefetch_token();
        let mut g = enter(
            "site-normal",
            arc,
            tok,
            Anchors {
                web_on_top: true,
                ai_snap: false,
            },
            42,
        );
        g.acquired();
        assert_eq!(holder_of(arc), tok, "acquired 后 holder 必须是本令牌");
    }
    assert_eq!(holder_of(arc), 0, "出域必须清 holder");

    // ② 自锁路径：先把自己的令牌写成 holder，再 enter ⇒ 报 self，且 Drop 不写 end
    let tok = prefetch_token();
    set_holder(arc, tok);
    {
        let _g = enter(
            "site-deadlock",
            arc,
            tok,
            Anchors {
                web_on_top: true,
                ai_snap: true,
            },
            43,
        );
        // 故意不让它 acquired——自锁时也走不到那里
    }

    let lines = log_path_read(&path);
    let joined = lines.join("\n");
    assert!(joined.contains("verdict=self"), "自锁必须报 self: {joined}");
    assert!(
        joined.contains("verdict=end"),
        "正常路径必须写 end: {joined}"
    );
    // 字段同形（楼646：结构可比）
    for f in [
        "probe=lock",
        "mode=",
        "mono_ms=",
        "site=",
        "arc=",
        "tok=",
        "holder=",
        "verdict=",
        "web_on_top=",
        "ai_snap=",
    ] {
        assert!(joined.contains(f), "报行缺字段 {f}: {joined}");
    }
    // 楼703：契约两项必须在（begin_ms／frame），不许用 mono_ms 顶替 begin_ms
    assert!(
        joined.contains("begin_ms="),
        "缺 begin_ms（契约项）: {joined}"
    );
    assert!(joined.contains("frame=43"), "缺帧号（契约项）: {joined}");
    // 因果锚（楼665）：自锁那行的锚必须带 web_on_top=1 / ai_snap=1
    let dead = lines
        .iter()
        .find(|l| l.contains("site-deadlock"))
        .expect("自锁行必须在");
    assert!(
        dead.contains("web_on_top=1") && dead.contains("ai_snap=1"),
        "锚没带上: {dead}"
    );
    // 自锁行**不得**带 end（楼653：缺 end 就是判据）
    let ends = lines.iter().filter(|l| l.contains("verdict=end")).count();
    assert_eq!(ends, 1, "只有正常路径那一行才有 end");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spec_lockprobe_init与invalid两类行可构造() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // 楼727／728：对表此前只比 probe=lock 行；init 与 invalid 两类也要能构造、可核。
    // ① init：init_log 后必须有一行 probe=lock mode=init schema=v4（三态判读第一态）
    // ② invalid：frame==0 即整行无效，落 probe=invalid reason=frame（缺字段名要说清）
    let dir = std::env::temp_dir().join(format!("lockprobe-ii-{}", std::process::id()));
    let path = dir.join("lockprobe.log");
    let _ = std::fs::remove_dir_all(&dir);
    init_log(&path);

    let arc2 = "test-arc-ii";
    clear_holder(arc2);
    let tok = prefetch_token();
    {
        // frame=0 ⇒ 故意构造无效行；守卫出来即 ended，additionally 不写 end
        let _g = enter("site-invalid", arc2, tok, Anchors::default(), 0);
    }

    let lines = log_path_read(&path);
    let init = lines
        .iter()
        .find(|l| l.contains("mode=init"))
        .expect("init 行必须可构造");
    assert!(
        init.contains("schema=v4"),
        "init 行必须带 schema 版本: {init}"
    );
    let inv = lines
        .iter()
        .find(|l| l.contains("probe=invalid"))
        .expect("invalid 行必须可构造");
    assert!(
        inv.contains("reason=frame"),
        "invalid 行必须给缺失字段名: {inv}"
    );
    // 无效行不得冒充有效证据：不许出现 site-invalid 的 lock 行
    assert!(
        !lines
            .iter()
            .any(|l| l.contains("site-invalid") && l.contains("probe=lock")),
        "frame 缺时不许出 probe=lock 行（整行无效）"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spec_lockprobe_校验逻辑可独立调用() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // 楼735：真实现里 tok／begin_ms 由内部生成，「缺字段」不可达；但夹具要能造出来，
    // 前提是**校验逻辑可独立调用**——所以 validate 是 pub 纯函数。
    use kfm_na::lockprobe::validate;
    assert_eq!(validate(1, 1), Ok(()));
    assert_eq!(validate(0, 1), Err("begin_ms"), "缺 begin_ms 要能判出来");
    assert_eq!(validate(1, 0), Err("frame"), "缺 frame 要能判出来");
    // 两个都缺时按字段顺序先报 begin_ms（确定性，便于对表）
    assert_eq!(validate(0, 0), Err("begin_ms"));
}

#[test]
fn spec_lockprobe_真实现产出的行按类型对表() {
    // 楼750／751／752：正对照**不是**「全 pass」，而是「真原文 ＋ 按类型的期望表」——
    // invalid 行本身在格式层合法（自带 schema+reason），期望也是 ok；该判 missing_field
    // 的是「缺字段的 lock 行」，那不可能由真实现产出。
    // 样本留档：scripts/check/lockprobe-samples-real.txt（输入也跟着结果一起留）。
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let checker = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/check/check-lockprobe-line.py"
    );
    let samples = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/check/lockprobe-samples-real.txt"
    );
    let out = std::process::Command::new("python3")
        .arg(checker)
        .arg("--fixture")
        .arg(samples)
        .output()
        .expect("必须能起 python3");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "真原文按类型期望表必须全过：{stdout}");
}
