//! install_spec.rs — na 自更新原语考题（A 档：文件/URI 半边纯函数）
//!
//! 契约（BAR-162，2026-09-26 定；缘起见 src/install.rs 头）：
//! ①名字净化与 KfmFileProvider.resolve **同口径**——取末段、拒空/含 `..`/
//!   非 .apk；不合规在进门就拒，不去白弹一次安装器；
//! ②URI 形态唯一合法：`content://dev.kfm.na.provider/apk/<单层名>`；
//! ③落定 = 进 `{files}/incoming/`（provider 根），已在里面就原地用（免
//!   42MB 白拷）；**先 .new 再改名**——半截包递给安装器 = 装出个坏包；
//! ④判决落 install-status 一行（谁跑谁写：Java 腿 Java 写，引导腿 Rust 写）。
//!
//! 变异抽检方向：名字放行 `..`（穿越进 provider）/ 不取末段（URI 多一层必被
//! 拒）/ 落定不原子（去掉 .new）/ 已在 incoming 也照拷（白拷 42MB）/ 放行
//! 非 .apk——本文件必须红。

use kfm_na::install::{
    APK_MIME, INCOMING_DIR, PROVIDER_AUTHORITY, Staged, apk_name, apk_uri, stage_apk, write_status,
};

#[test]
fn spec_bar162_安装包名净化与provider同口径() {
    assert_eq!(
        apk_name("/data/data/dev.kfm.na/files/incoming/kfm-na-1790414909.apk"),
        Some("kfm-na-1790414909.apk".to_string()),
        "取末段"
    );
    assert_eq!(apk_name("kfm-na.apk"), Some("kfm-na.apk".to_string()));
    assert_eq!(
        apk_name("/x/kfm-na.APK"),
        Some("kfm-na.APK".to_string()),
        "扩展名大小写不敏感"
    );
    // provider resolve 允许的只有单层名；含 .. 的一律拒（穿越）
    assert_eq!(
        apk_name("/x/../y.apk"),
        Some("y.apk".to_string()),
        "末段合法即合法"
    );
    assert_eq!(apk_name("eviltraversal..apk"), None, "名字带 .. 必拒");
    assert_eq!(apk_name(".."), None);
    assert_eq!(apk_name(""), None);
    assert_eq!(apk_name("/x/"), None, "尾斜杠 = 空名");
    assert_eq!(apk_name("/x/payload.zip"), None, "只递 .apk");
    assert_eq!(apk_name("/x/a\0b.apk"), None, "NUL 必拒");
}

#[test]
fn spec_bar162_递交uri形态唯一() {
    let u = apk_uri("kfm-na-1.apk");
    assert_eq!(u, "content://dev.kfm.na.provider/apk/kfm-na-1.apk");
    assert!(u.contains(PROVIDER_AUTHORITY), "authority 与 manifest 同源");
    assert_eq!(
        APK_MIME, "application/vnd.android.package-archive",
        "MIME 与 provider.getType 同源"
    );
}

#[test]
fn spec_bar162_落定进incoming_原子且不白拷() {
    let td = tempfile::tempdir().unwrap();
    let data = td.path().to_str().unwrap();
    // 源在别处（usr/tmp 那种）：拷进 incoming/
    let src_dir = td.path().join("usr-tmp");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("kfm-na-9.apk");
    std::fs::write(&src, b"FAKE-APK-BYTES").unwrap();
    let st: Staged = stage_apk(data, src.to_str().unwrap()).unwrap();
    assert_eq!(
        st.path,
        td.path()
            .join(INCOMING_DIR)
            .join("kfm-na-9.apk")
            .to_str()
            .unwrap(),
        "落地必在 provider 根下"
    );
    assert_eq!(st.uri, apk_uri("kfm-na-9.apk"));
    assert_eq!(st.bytes, 14, "字节数照实报（安装器要核对）");
    assert_eq!(std::fs::read(&st.path).unwrap(), b"FAKE-APK-BYTES");
    assert!(
        std::fs::read(&src).unwrap() == b"FAKE-APK-BYTES",
        "源不动（只拷不改）"
    );
    let leftovers: Vec<_> = std::fs::read_dir(td.path().join(INCOMING_DIR))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".new"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "{leftovers:?} 半截 .new 不许留场（递交前落定必须原子）"
    );
    // 源已在 incoming 里：原地用，不再拷一份（免 42MB 白拷）
    let before = std::fs::metadata(&st.path).unwrap().len();
    let again = stage_apk(data, &st.path).unwrap();
    assert_eq!(again.path, st.path, "原地用");
    assert_eq!(std::fs::metadata(&again.path).unwrap().len(), before);
    let entries: Vec<_> = std::fs::read_dir(td.path().join(INCOMING_DIR))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries.len(), 1, "不许拷出第二份：{entries:?}");
}

#[test]
fn spec_bar162_落定坏输入必拒() {
    let td = tempfile::tempdir().unwrap();
    let data = td.path().to_str().unwrap();
    // 名字不合规：进门就拒（不去白弹安装器）
    let bad = td.path().join("payload.zip");
    std::fs::write(&bad, b"x").unwrap();
    assert!(
        stage_apk(data, bad.to_str().unwrap()).is_err(),
        "非 .apk 必拒"
    );
    // 源不存在
    assert!(stage_apk(data, "/no/such/kfm-na.apk").is_err());
    // 源是目录
    assert!(
        stage_apk(data, td.path().to_str().unwrap()).is_err(),
        "目录不是包"
    );
}

#[test]
fn spec_bar162_判决落账_覆盖写一行() {
    let td = tempfile::tempdir().unwrap();
    let d = td.path().to_str().unwrap();
    write_status(d, "ok leg=java uri=content://x").unwrap();
    let f = td.path().join("install-status");
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "ok leg=java uri=content://x\n",
        "一行一判决（判卷直接 cat）"
    );
    // 覆盖写：后一次判决不追加（否则读回来分不清哪次算数）
    write_status(d, "NO_HANDLER leg=java uri=content://x").unwrap();
    let got = std::fs::read_to_string(&f).unwrap();
    assert_eq!(got.lines().count(), 1);
    assert!(got.starts_with("NO_HANDLER"));
}

#[test]
fn spec_bar162_两腿接线源码守卫() {
    // 主臂：Java 皮 UI 线程正道（ACTION_VIEW + grant 标志）；
    // 引导腿：门线程直调 activity.startActivity（现行装机 APK 没有新方法
    // 时的零引导路）。摘任一条 = 自更新原语残废（或断零点引导）。
    let java = include_str!("../android/java/dev/kfm/na/MainActivity.java");
    assert!(
        java.contains("public void installApkFromGate"),
        "install-apk-req 的 Java 着陆点被摘（自更新原语主臂）"
    );
    // 只切方法体本身（到下一个成员为止）——切到文件尾会把别的方法的
    // runOnUiThread、以及 installStatus 的**定义**一起圈进来，守卫形同虚设
    let body = java
        .split("public void installApkFromGate")
        .nth(1)
        .and_then(|s| s.split("private void installStatus").next())
        .expect("installApkFromGate 定义被摘");
    for need in [
        "runOnUiThread",
        "Intent.ACTION_VIEW",
        "FLAG_GRANT_READ_URI_PERMISSION",
        "startActivity(it)",
        "installStatus(verdict)",
    ] {
        assert!(
            body.contains(need),
            "Java 腿缺 {need}（递不出去或判卷无账）"
        );
    }
    let app = include_str!("../src/android_app.rs");
    assert!(
        app.contains("crate::gate::register_install_hook"),
        "通道十五钩子没注册（闸门投了没人接）"
    );
    assert!(
        app.contains("fn install_intent_direct") && app.contains("install_intent_direct(&vm4"),
        "引导腿被摘或没接上（现行装机 APK 上没有新方法时 = 零引导断）"
    );
    assert!(
        app.contains("exception_clear"),
        "引导腿前必须清挂起的 Java 异常（残留异常会毒后续每次 JNI 调用）"
    );
    assert!(app.contains("installApkFromGate"), "主臂 JNI 调用点被摘");
    let gate = include_str!("../src/gate.rs");
    assert!(
        gate.contains("install_req_check(DUMP_DIR)"),
        "值守循环没消费 install-apk-req（投了没人看）"
    );
    let script = include_str!("../scripts/na-install-apk.sh");
    assert!(
        script.contains("install-apk-req"),
        "推包脚本与闸门文件名脱钩"
    );
    // 落定原子性：先 .new 再 rename（半截包递给安装器 = 装出个坏包，同 hot/
    // 与 BAR-150 的第三个落点）。行为面测不了「中途半截」，故源码守卫钉这两件
    // 必须成对出现——去掉 .new 直接写终名即破约。
    // 判据必须是**代码形态**（注释里也写着 .new，拿 .new 当判据 = 变异2 逃逸）
    let pure = include_str!("../src/install.rs");
    assert!(
        pure.contains("incoming.join(format!(\"{name}.new\"))")
            && pure.contains("std::fs::rename(&tmp, &dst)"),
        "递交前落定不许退化非原子（.new 落临时 + rename 落定，成对）"
    );
}
