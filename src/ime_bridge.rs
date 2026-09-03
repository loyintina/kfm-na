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
        crate::ime_queue::global().push_commit(&s);
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

/// dev.kfm.na.KfmImeView.nativeComposingText —— IME 组合态文本
/// （setComposingText：拼音预编辑，随打随变）。空串 = 组合清空。
/// 消费侧按焦点分流：输入栏聚焦 → preedit 上栏；终端 → 沿革吞掉
/// （BAR-012 终端语义不变）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeComposingText(
    mut env: EnvUnowned,
    _class: JClass,
    text: JString,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let s = text.try_to_string(env)?;
        crate::ime_queue::global().push_composing(&s);
        Ok(())
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeComposingText".to_string());
}

/// dev.kfm.na.KfmImeView.nativeFinishComposing —— 组合结束
/// （finishComposingText：候选词已定，组合区收账）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeFinishComposing(
    _env: EnvUnowned,
    _class: JClass,
) {
    crate::ime_queue::global().push_composing_end();
}

/// dev.kfm.na.KfmImeView.nativeContextMenuAction —— IME 上下文菜单动作
/// （performContextMenuAction；2026-09-02 曲线救国：系统剪贴板被 ROM 锁死，
/// 输入法工具栏的复制/剪切/粘贴/全选直送状态核，不走系统剪贴板）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeContextMenuAction(
    mut env: EnvUnowned,
    _class: JClass,
    action: JString,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let s = action.try_to_string(env)?;
        crate::ime_queue::global().push_context_menu_action(&s);
        Ok(())
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeContextMenuAction".to_string());
}

/// dev.kfm.na.KfmImeView.nativeSelectedText —— 当前选区文本直答（BAR-054）：
/// 输入法按「剪切」前先 getSelectedText() 探选区，答空它就不发 cut。
/// 无选区/栏未登记/出错 = null（JString::default()；哑火返回无选区，
/// 同 Java 侧 try/catch 契约）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeSelectedText<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let sel = crate::gate::input_bar_handle().and_then(|bar| bar.selected_text());
        match sel {
            Some(text) => Ok(env.new_string(text)?),
            None => Ok(JString::default()),
        }
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeSelectedText".to_string())
}

/// dev.kfm.na.KfmImeView.nativeTextBeforeCursor —— 光标前 n 字（BAR-054：
/// IME 内部删除/替换逻辑靠 getTextBeforeCursor 算范围，答空它删个寂寞）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeTextBeforeCursor<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass,
    n: jint,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let t = crate::gate::input_bar_handle()
            .map(|bar| bar.text_before_cursor(n.max(0) as usize))
            .unwrap_or_default();
        env.new_string(t)
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeTextBeforeCursor".to_string())
}

/// dev.kfm.na.KfmImeView.nativeTextAfterCursor —— 光标后 n 字（BAR-054）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeTextAfterCursor<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass,
    n: jint,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let t = crate::gate::input_bar_handle()
            .map(|bar| bar.text_after_cursor(n.max(0) as usize))
            .unwrap_or_default();
        env.new_string(t)
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeTextAfterCursor".to_string())
}

/// dev.kfm.na.KfmImeView.nativeSetSelection —— IME setSelection 直设（BAR-054）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeSetSelection(
    _env: EnvUnowned,
    _class: JClass,
    start: jint,
    end: jint,
) {
    if let Some(bar) = crate::gate::input_bar_handle() {
        bar.set_caret_or_selection(start.max(0) as usize, end.max(0) as usize);
    }
}

/// dev.kfm.na.KfmImeView.nativeReplaceText —— IME replaceText 直改（BAR-054：
/// 剪切删除半若走 replaceText(start,end,"") 即此形态）
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_kfm_na_KfmImeView_nativeReplaceText(
    mut env: EnvUnowned,
    _class: JClass,
    start: jint,
    end: jint,
    text: JString,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let t = text.try_to_string(env)?;
        if let Some(bar) = crate::gate::input_bar_handle() {
            bar.replace_range(start.max(0) as usize, end.max(0) as usize, &t);
        }
        Ok(())
    })
    .resolve_with::<LogContextErrorAndDefault, _>(|| "in nativeReplaceText".to_string());
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
