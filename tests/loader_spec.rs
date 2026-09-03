//! na-loader 考题(A 档)——热更新壳的路径/回落/落档格式纯函数
//!
//! 契约(2026-08-26 与用户定):
//! ①热更核心固定住 {files}/hot/libkfm_na.so;
//! ②热更优先,不存在或 dlopen 失败才回落包内捆绑核心;
//! ③每次选择落档 loader-pick,一行一案(跑的是谁必须可查)。

use na_loader::{CorePick, hot_core_path, pick_core, pick_line, pick_rec_path};

#[test]
fn spec_loader_热更核心路径拼接() {
    assert_eq!(
        hot_core_path("/data/data/dev.kfm.na/files"),
        "/data/data/dev.kfm.na/files/hot/libkfm_na.so"
    );
    // 尾斜杠不双写
    assert_eq!(
        hot_core_path("/data/data/dev.kfm.na/files/"),
        "/data/data/dev.kfm.na/files/hot/libkfm_na.so"
    );
    assert_eq!(
        pick_rec_path("/data/data/dev.kfm.na/files"),
        "/data/data/dev.kfm.na/files/usr/tmp/loader-pick"
    );
}

#[test]
fn spec_loader_回落顺序钉() {
    // 热更在且载得动 = 用热更
    assert_eq!(pick_core(true, true), CorePick::Hot);
    // 热更不存在 / 加载失败 = 回落捆绑(两种病都回落,不白屏)
    assert_eq!(pick_core(false, false), CorePick::Bundled);
    assert_eq!(pick_core(true, false), CorePick::Bundled);
    assert_eq!(pick_core(false, true), CorePick::Bundled);
}

#[test]
fn spec_loader_落档行格式() {
    assert_eq!(
        pick_line(1787714059, CorePick::Hot, "path=/x/hot/libkfm_na.so"),
        "unix=1787714059 pick=hot path=/x/hot/libkfm_na.so"
    );
    assert_eq!(
        pick_line(1, CorePick::Bundled, "why=无热更核心"),
        "unix=1 pick=bundled why=无热更核心"
    );
}

// BAR-054 2026-09-03 装机实拍：新增 JNI 方法 nativeSelectedText（Java +
// 核心 ime_bridge 都加了），唯独忘加 na-loader 转发表——Java 按 BAR-039
// 焊死绑 na_loader 的符号，转发表没有 = UnsatisfiedLinkError 被 KfmImeView
// 的 try/catch 静默吞成 null，IME 剪切第一环 getSelectedText 恒答 null，
// 全链静默哑火。机械钉：KfmImeView.java 声明的每个 static native 方法，
// na-loader 转发表与核心 ime_bridge 都必须有同名符号——新加 JNI 方法
// 三处（Java/核心/loader）少一处，本钉当场红，不再靠装机实拍才发现。
#[test]
fn spec_bar054_jni方法三方齐备() {
    let java = include_str!("../android/java/dev/kfm/na/KfmImeView.java");
    let loader = include_str!("../crates/na-loader/src/lib.rs");
    let core = include_str!("../src/ime_bridge.rs");
    let mut names = Vec::new();
    for seg in java.split("static native ").skip(1) {
        let head = seg.split('(').next().unwrap_or("");
        let Some(name) = head.split_whitespace().last() else {
            continue;
        };
        if !name.is_empty() {
            names.push(name);
        }
    }
    assert!(
        names.len() >= 7,
        "解析保底：KfmImeView 至少 7 个 native 方法（实测 {names:?}）"
    );
    for name in &names {
        // 必须匹配真实函数定义行，不许裸子串——符号改名/注释提及都会
        // 留下旧串骗过 contains（2026-09-03 变异抽检实录：DISABLED 后缀
        // 变体照样含子串，钉被架空）
        let def = format!("fn Java_dev_kfm_na_KfmImeView_{name}(");
        assert!(
            loader.contains(&def),
            "na-loader 转发表缺 {def}（BAR-039：Java 焊死绑 na_loader，缺它=静默吞）"
        );
        // 核心侧容忍 jni 0.22 生命期泛型（fn name<'local>( 形态）
        let def_generic = format!("fn Java_dev_kfm_na_KfmImeView_{name}<");
        assert!(
            core.contains(&def) || core.contains(&def_generic),
            "核心 ime_bridge 缺 {def} 实现"
        );
    }
}
