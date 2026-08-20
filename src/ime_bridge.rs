//! ime_bridge.rs — Java 皮 → Rust 的 IME 文字桥（B 档 JNI 薄皮）
//!
//! 对侧是 android/java/dev/kfm/na/KfmImeView.java 的两个 static native
//! 方法，由 KfmInputConnection 在 IME commitText / 软键事件时调用。
//! 本模块只做一件事：把参数转进 ime_queue。逻辑全在队列里（A 档考题
//! ime_queue_spec.rs），这里薄到没有可考的——判卷 = 真机实拍中文落字。
//!
//! 纪律：JNI 回调线程绝不 panic 跨 FFI 边界——with_env 捕获恐慌，
//! LogContextErrorAndDefault 落日志返回默认，丢一字好过崩一次。

use jni::EnvUnowned;
use jni::errors::LogContextErrorAndDefault;
use jni::objects::{JClass, JString};
use jni::sys::jint;
use std::sync::atomic::{AtomicU32, Ordering};

/// JNI 入口计数器（BAR-012③ 三轮诊断）：在任何可能失败的调用之前自增——
/// Java 侧 try/catch 把 UnsatisfiedLinkError 吞得无声无息，只有入口第一行
/// 的计数能证明 ART 到底有没有把 Java 调用绑进 Rust。心跳里读数：
/// 全 0 = Java→JNI 全灭（绑定失败）；>0 而 pushed=0 = 死在字符串转换
static COMMIT_ENTER: AtomicU32 = AtomicU32::new(0);
static COMMIT_PUSHED: AtomicU32 = AtomicU32::new(0);
static SENDKEY_ENTER: AtomicU32 = AtomicU32::new(0);
static IMELOG_ENTER: AtomicU32 = AtomicU32::new(0);

/// 给事件循环心跳读数：(commit 入口, commit 入队成功, 软键入口, 探针入口)
pub fn jni_counters() -> (u32, u32, u32, u32) {
    (
        COMMIT_ENTER.load(Ordering::Relaxed),
        COMMIT_PUSHED.load(Ordering::Relaxed),
        SENDKEY_ENTER.load(Ordering::Relaxed),
        IMELOG_ENTER.load(Ordering::Relaxed),
    )
}

/// dev.kfm.na.KfmImeView.nativeCommitText —— IME commitText 落字
/// （中文候选词、英文整串、粘贴都走这）。落字前过修饰键粘滞：
/// 状态在 input.modifiers 服务（input-ime 方案 A），JNI 线程经桥端点
/// （keybar::bridge_mods）取句柄，take 读走即清 = 一次性联动
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeCommitText(
    mut env: EnvUnowned,
    _class: JClass,
    text: JString,
) {
    COMMIT_ENTER.fetch_add(1, Ordering::Relaxed);
    env.with_env(|env| -> jni::errors::Result<()> {
        let s = text.try_to_string(env)?;
        let mods = crate::keybar::bridge_mods().map(|m| m.take()).unwrap_or(0);
        let s = if mods != 0 {
            crate::keymap::map_text(
                mods & crate::keybar::MOD_CTRL != 0,
                mods & crate::keybar::MOD_ALT != 0,
                mods & crate::keybar::MOD_SHIFT != 0,
                &s,
            )
        } else {
            s
        };
        crate::ime_queue::global().push_text(&s);
        COMMIT_PUSHED.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeCommitText".to_string());
}

/// dev.kfm.na.KfmImeView.nativeSendKey —— 软键事件（退格/回车）。
/// 接上 InputConnection 后软键盘删除走连接而非按键队列，Java 侧翻成
/// 键码送来这里存原始值，序列翻译在排干侧（keymap.rs）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeSendKey(
    _env: EnvUnowned,
    _class: JClass,
    code: jint,
) {
    SENDKEY_ENTER.fetch_add(1, Ordering::Relaxed);
    crate::ime_queue::global().push_key_code(code);
}

/// dev.kfm.na.KfmImeView.nativeImeLog —— Java 侧链路探针直送飞鸽传书。
/// IME 的生死在 IMM 与焦点之间（BAR-009 就死在 IMM 拒弹），Rust 侧看不见，
/// 让 Java 侧断点自己开口
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeImeLog(
    mut env: EnvUnowned,
    _class: JClass,
    msg: JString,
) {
    IMELOG_ENTER.fetch_add(1, Ordering::Relaxed);
    env.with_env(|env| -> jni::errors::Result<()> {
        let s = msg.try_to_string(env)?;
        crate::report::report("ime", &format!("[java] {s}"));
        Ok(())
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeImeLog".to_string());
}
