//! clipboard.rs — 系统剪贴板 + Toast 提示（B 档平台胶水，2026-08-21）
//!
//! 长按选择的复制落点：自绘世界没有 EditText，复制必须自己调
//! ClipboardManager。链路：
//!   activity.getSystemService("clipboard") → ClipboardManager
//!   → ClipData.newPlainText("kfm", text) → setPrimaryClip
//!   → Toast.makeText(activity, "已复制 N 字符", LENGTH_SHORT).show()
//!
//! B 档先例同 insets.rs：对错是「系统让不让你活」，判卷 = 真机实拍 +
//! [ime] 上报行（复制走 [ime] 通道——触摸交互诊断都在那），无 host 考题。

use jni::objects::JObject;
use jni::{JavaVM, jni_sig, jni_str};
use winit::platform::android::activity::AndroidApp;

/// 选中文字写系统剪贴板 + Toast 提示「已复制 N 字符」（N = 字符数非字节数）。
/// JNI 任一环失败只上报不 panic——复制失败不该炸终端
pub fn copy_and_toast(app: &AndroidApp, text: &str) {
    let n = text.chars().count();
    // SAFETY: vm_as_ptr 是 android-activity 保证有效的 JavaVM 指针
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY: 全局引用归 android-activity 所有，from_raw 只包视图不接管
        let activity = unsafe { JObject::from_raw(env, raw_activity) };
        let service_name = env.new_string("clipboard")?;
        let cm = env
            .call_method(
                &activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[jni::JValue::Object(&service_name)],
            )?
            .l()?;
        let label = env.new_string("kfm")?;
        let jtext = env.new_string(text)?;
        let clip = env
            .call_static_method(
                jni_str!("android/content/ClipData"),
                jni_str!("newPlainText"),
                jni_sig!(
                    "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
                ),
                &[jni::JValue::Object(&label), jni::JValue::Object(&jtext)],
            )?
            .l()?;
        env.call_method(
            &cm,
            jni_str!("setPrimaryClip"),
            jni_sig!("(Landroid/content/ClipData;)V"),
            &[jni::JValue::Object(&clip)],
        )?;
        // Toast.makeText(Context, CharSequence, int).show()——LENGTH_SHORT = 0
        let msg = env.new_string(format!("已复制 {n} 字符"))?;
        let toast = env
            .call_static_method(
                jni_str!("android/widget/Toast"),
                jni_str!("makeText"),
                jni_sig!(
                    "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;"
                ),
                &[
                    jni::JValue::Object(&activity),
                    jni::JValue::Object(&msg),
                    jni::JValue::Int(0),
                ],
            )?
            .l()?;
        env.call_method(&toast, jni_str!("show"), jni_sig!("()V"), &[])?;
        Ok(())
    });
    match result {
        Ok(()) => crate::report::report("ime", &format!("已复制 {n} 字符到系统剪贴板")),
        Err(e) => crate::report::report("ime", &format!("剪贴板 JNI 链路失败: {e:?}")),
    }
}
