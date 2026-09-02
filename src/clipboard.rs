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

use jni::objects::{JObject, JString};
use jni::{JavaVM, jni_sig, jni_str};
use winit::platform::android::activity::AndroidApp;

/// 选中文字写系统剪贴板 + Toast 提示「已复制 N 字符」（N = 字符数非字节数）。
/// JNI 任一环节失败只上报不 panic——复制失败不该炸终端
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
        // Toast 提示（BAR-046）：makeText 必须在带 Looper 的线程调用，
        // 而 gate 值守线程没有 Looper——实测抛 NPE「Can't toast on a thread
        // that has not called Looper.prepare()」。当前先移除 Toast，只保
        // 剪贴板写入；后续如需要 Toast 提示，应通过 Activity.runOnUiThread
        // 或 Handler 移到主线程再调。
        Ok(())
    });
    match result {
        Ok(()) => crate::report::report("ime", &format!("已复制 {n} 字符到系统剪贴板")),
        Err(e) => crate::report::report("ime", &format!("剪贴板 JNI 链路失败: {e:?}")),
    }
}

/// 读系统剪贴板文本。剪贴板为空/非文本/无权限/ABI 失败都返回 None，不 panic。
pub fn get_clipboard_text(app: &AndroidApp) -> Option<String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
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
        // clip = cm.getPrimaryClip()
        let clip = env
            .call_method(
                &cm,
                jni_str!("getPrimaryClip"),
                jni_sig!("()Landroid/content/ClipData;"),
                &[],
            )?
            .l()?;
        if clip.is_null() {
            return Ok(None);
        }
        // item = clip.getItemAt(0)
        let item = env
            .call_method(
                &clip,
                jni_str!("getItemAt"),
                jni_sig!("(I)Landroid/content/ClipData$Item;"),
                &[jni::JValue::Int(0)],
            )?
            .l()?;
        if item.is_null() {
            return Ok(None);
        }
        // text = item.getText()
        let text_obj = env
            .call_method(
                &item,
                jni_str!("getText"),
                jni_sig!("()Ljava/lang/CharSequence;"),
                &[],
            )?
            .l()?;
        if text_obj.is_null() {
            return Ok(None);
        }
        // SAFETY: getText() 返回 java.lang.CharSequence，成功调用后这里按 String 读。
        // 实际读之前先 is_null() 过滤；into_raw + from_raw 只转移局部引用所有权。
        let raw = text_obj.into_raw();
        let jstr = unsafe { JString::from_raw(env, raw) };
        let s = jstr.try_to_string(env)?;
        Ok(Some(s))
    });
    match result {
        Ok(Some(s)) => {
            let n = s.chars().count();
            crate::report::report("ime", &format!("从系统剪贴板粘贴 {n} 字符"));
            Some(s)
        }
        Ok(None) => {
            crate::report::report("ime", "剪贴板为空或非文本");
            None
        }
        Err(e) => {
            crate::report::report("ime", &format!("读剪贴板 JNI 链路失败: {e:?}"));
            None
        }
    }
}
