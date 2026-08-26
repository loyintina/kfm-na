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
