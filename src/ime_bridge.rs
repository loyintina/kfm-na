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

/// dev.kfm.na.KfmImeView.nativeCommitText —— IME commitText 落字
/// （中文候选词、英文整串、粘贴都走这）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeCommitText(
    mut env: EnvUnowned,
    _class: JClass,
    text: JString,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let s = text.try_to_string(env)?;
        crate::ime_queue::global().push_text(&s);
        Ok(())
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeCommitText".to_string());
}

/// dev.kfm.na.KfmImeView.nativeSendKey —— 软键事件（退格/回车）。
/// 接上 InputConnection 后软键盘删除走连接而非按键队列，Java 侧翻成
/// 键码送来这里，映射表在 ime_queue::key_code_to_bytes
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeSendKey(
    _env: EnvUnowned,
    _class: JClass,
    code: jint,
) {
    crate::ime_queue::global().push_key_code(code);
}
