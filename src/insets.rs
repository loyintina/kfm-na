//! insets.rs — JNI 直取真实软键盘高度（BAR-006 正道，2026-08-13 实拍定案）
//!
//! 为什么必须走 JNI：
//! - winit 0.30 Android 的 Ime::Enabled/Disabled 在本机（OriginOS）从未触发
//!   （全日志零条），估计式避让成了死代码；
//! - cargo-apk 0.10 / ndk-build 的 Activity 字段表没有 windowSoftInputMode，
//!   Manifest 正道被构建工具封死；
//! - android-activity 0.6 无 insets API。
//! 剩下的唯一活路：JNI 直调 WindowInsets。
//! 链路：Activity.getWindow().getDecorView().getRootWindowInsets()
//!   → WindowInsets.Type.ime() → isVisible(type) / getInsets(type).bottom
//!
//! B 档平台胶水：对错是「系统让不让你活」，判卷 = 真机实拍 + [ime] 上报行。

use jni::objects::JObject;
use jni::{JavaVM, jni_sig, jni_str};
use winit::platform::android::activity::AndroidApp;

/// 强制弹出软键盘（BAR-012）：winit 的 set_ime_allowed 走 SHOW_IMPLICIT，
/// 用户手动收过键盘后 IMM 按策略拒弹（实拍：关掉再点就召唤不出）。
/// SHOW_FORCED = 用户强制召唤，无视该策略
pub fn force_show_keyboard(app: &AndroidApp) {
    // SAFETY: 同 query_ime_bottom
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    let _ = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY: 同 query_ime_bottom
        let activity = unsafe { JObject::from_raw(env, raw_activity) };
        let service_name = env.new_string("input_method")?;
        let imm = env
            .call_method(
                &activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[jni::JValue::Object(&service_name)],
            )?
            .l()?;
        let window = env
            .call_method(
                &activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        let decor = env
            .call_method(
                window,
                jni_str!("getDecorView"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        const SHOW_FORCED: i32 = 2;
        env.call_method(
            imm,
            jni_str!("showSoftInput"),
            jni_sig!("(Landroid/view/View;I)Z"),
            &[jni::JValue::Object(&decor), jni::JValue::Int(SHOW_FORCED)],
        )?;
        Ok(())
    });
}

/// 查询一次真实 IME 底部 inset（px；NativeActivity 全屏窗与帧缓冲同坐标系）。
/// 键盘未弹 → Some(0)；弹出 → Some(高度)；查询失败（JNI 异常/无 insets）→ None
/// （调用方维持旧值，不抖动）。
pub fn query_ime_bottom(app: &AndroidApp) -> Option<u32> {
    // SAFETY: vm_as_ptr 是 android-activity 保证有效的 JavaVM 指针
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
    vm.attach_current_thread(|env| -> jni::errors::Result<Option<u32>> {
        // SAFETY: android-activity 的全局引用，归它所有、随它存活；
        // from_raw 只包一层视图，不接管所有权、绝不 delete
        let activity = unsafe { JObject::from_raw(env, raw_activity) };
        let window = env
            .call_method(
                &activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        let decor = env
            .call_method(
                window,
                jni_str!("getDecorView"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        let insets = env
            .call_method(
                decor,
                jni_str!("getRootWindowInsets"),
                jni_sig!("()Landroid/view/WindowInsets;"),
                &[],
            )?
            .l()?;
        if insets.is_null() {
            return Ok(None);
        }
        // WindowInsets.Type.ime() 静态方法（API 30+，本机 OriginOS 35 够用）
        let ime_type = env
            .call_static_method(
                jni_str!("android/view/WindowInsets$Type"),
                jni_str!("ime"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let visible = env
            .call_method(
                &insets,
                jni_str!("isVisible"),
                jni_sig!("(I)Z"),
                &[jni::JValue::Int(ime_type)],
            )?
            .z()?;
        if !visible {
            return Ok(Some(0));
        }
        let ins = env
            .call_method(
                insets,
                jni_str!("getInsets"),
                jni_sig!("(I)Landroid/graphics/Insets;"),
                &[jni::JValue::Int(ime_type)],
            )?
            .l()?;
        let bottom = env.get_field(ins, jni_str!("bottom"), jni_sig!("I"))?.i()?;
        Ok(Some(bottom.max(0) as u32))
    })
    .ok()
    .flatten()
}
