//! insets.rs — JNI 直取真实软键盘高度（BAR-006 正道，2026-08-13 实拍定案）
//!
//! 为什么必须走 JNI：
//! - winit 0.30 Android 的 Ime::Enabled/Disabled 在本机（OriginOS）从未触发
//!   （全日志零条），估计式避让成了死代码；
//! - cargo-apk 0.10 / ndk-build 的 Activity 字段表没有 windowSoftInputMode，
//!   Manifest 正道被构建工具封死；
//! - android-activity 0.6 无 insets API。
//!
//! 剩下的唯一活路：JNI 直调 WindowInsets。
//! 链路：Activity.getWindow().getDecorView().getRootWindowInsets()
//!   → WindowInsets.Type.ime() → isVisible(type) / getInsets(type).bottom
//!
//! B 档平台胶水：对错是「系统让不让你活」，判卷 = 真机实拍 + [ime] 上报行。
//!
//! 插件化（input-ime 设计页，2026-08-16）：`ImeInsets` trait 跨平台常开
//! （考题喂假实现），JNI 胶水与 `JniInsets` 薄壳 cfg(android)。形态 =
//! 共享实例直挂（Sync），不是工厂——判别准则见规格书 v1.2 §4.2。

/// 键盘 inset 轮询间隔（BAR-065：500→100ms——间隔就是栏带跟键盘的
/// 感知延迟本身；轮询只在事件循环醒着时跑，100ms 成本可忽略）。
/// 家在 insets 而非 android_app：后者 cfg(android) 考题够不着
pub const IME_POLL_MS: u64 = 100;

/// 键盘来源服务（服务键 `dyn ImeInsets`，共享实例直挂）。
/// 生产 = JniInsets（JNI 直调 WindowInsets）；考题 = 假实现。
pub trait ImeInsets: Send + Sync {
    /// 真实 IME 底部 inset（px）。未弹 Some(0)；查询失败 None（调用方维持旧值）
    fn ime_bottom_px(&self) -> Option<u32>;
    /// 强制弹出软键盘（BAR-012：SHOW_FORCED 无视 IMM 拒弹策略）
    fn force_show(&self);
    /// 强制收起软键盘（期 0④：点非输入区 = 失焦收键盘；
    /// hideSoftInputFromWindow(decorView.windowToken, 0)）
    fn force_hide(&self);
}

#[cfg(target_os = "android")]
pub use imp::{JniInsets, force_hide_keyboard, force_show_keyboard, query_ime_bottom};

#[cfg(target_os = "android")]
mod imp {
    use jni::objects::JObject;
    use jni::{JavaVM, jni_sig, jni_str};
    use winit::platform::android::activity::AndroidApp;

    use super::ImeInsets;

    /// JNI 键盘来源薄壳（构造注入 AndroidApp 句柄——运行时对象不走配置表，
    /// 评审裁决 4）
    pub struct JniInsets {
        app: AndroidApp,
    }

    impl JniInsets {
        pub fn new(app: AndroidApp) -> Self {
            JniInsets { app }
        }
    }

    impl ImeInsets for JniInsets {
        fn ime_bottom_px(&self) -> Option<u32> {
            query_ime_bottom(&self.app)
        }
        fn force_show(&self) {
            force_show_keyboard(&self.app)
        }
        fn force_hide(&self) {
            force_hide_keyboard(&self.app)
        }
    }

    /// 强制弹出软键盘（BAR-012）：winit 的 set_ime_allowed 走 SHOW_IMPLICIT，
    /// 用户手动收过键盘后 IMM 按策略拒弹（实拍：关掉再点就召唤不出）。
    /// SHOW_FORCED = 用户强制召唤，无视该策略。
    ///
    /// 二轮诊断（实拍 Some(false) 后）：强弹目标从 decorView 换成
    /// **当前焦点 View 本身**，并把「焦点是谁 + IMM 认不认它（isActive）」
    /// 一并报回——showSoftInput 返回 false 几乎只有一个意思：IMM 没有可用
    /// 输入目标（served view）。每次触摸都报（用户点几下就几行），
    /// 三个数直接区分焦点丢失 vs IMM 拒认。
    pub fn force_show_keyboard(app: &AndroidApp) {
        // SAFETY: 同 query_ime_bottom
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        let result = vm.attach_current_thread(|env| -> jni::errors::Result<String> {
            // SAFETY: 同 query_ime_bottom
            let activity = unsafe { JObject::from_raw(env, raw_activity) };
            let focus = env
                .call_method(
                    &activity,
                    jni_str!("getCurrentFocus"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            if focus.is_null() {
                return Ok("焦点为空——showSoftInput 无目标可打".to_string());
            }
            let cls = env
                .call_method(
                    &focus,
                    jni_str!("getClass"),
                    jni_sig!("()Ljava/lang/Class;"),
                    &[],
                )?
                .l()?;
            let name_obj = env
                .call_method(
                    cls,
                    jni_str!("getSimpleName"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let name_jstring = env.cast_local::<jni::objects::JString>(name_obj)?;
            let name = name_jstring.try_to_string(env)?;
            let service_name = env.new_string("input_method")?;
            let imm = env
                .call_method(
                    &activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[jni::JValue::Object(&service_name)],
                )?
                .l()?;
            let active = env
                .call_method(
                    &imm,
                    jni_str!("isActive"),
                    jni_sig!("(Landroid/view/View;)Z"),
                    &[jni::JValue::Object(&focus)],
                )?
                .z()?;
            const SHOW_FORCED: i32 = 2;
            let shown = env
                .call_method(
                    imm,
                    jni_str!("showSoftInput"),
                    jni_sig!("(Landroid/view/View;I)Z"),
                    &[jni::JValue::Object(&focus), jni::JValue::Int(SHOW_FORCED)],
                )?
                .z()?;
            Ok(format!("焦点={name} isActive={active} 强弹={shown}"))
        });
        match result {
            Ok(msg) => crate::report::report("ime", &format!("强弹诊断: {msg}")),
            Err(_) => crate::report::report("ime", "强弹诊断: JNI 链路失败"),
        }
    }

    /// 强制收起软键盘（期 0④：点 AI 面板等非输入区 = 失焦收键盘）。
    /// hideSoftInputFromWindow 要的是 windowToken——decorView 恒在，
    /// 不依赖当前焦点（焦点那套在 force_show 里是诊断必需，收键盘不用）
    pub fn force_hide_keyboard(app: &AndroidApp) {
        // SAFETY: 同 query_ime_bottom
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let raw_activity = app.activity_as_ptr() as jni::sys::jobject;
        let result = vm.attach_current_thread(|env| -> jni::errors::Result<String> {
            // SAFETY: 同 query_ime_bottom
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
            let token = env
                .call_method(
                    &decor,
                    jni_str!("getWindowToken"),
                    jni_sig!("()Landroid/os/IBinder;"),
                    &[],
                )?
                .l()?;
            let service_name = env.new_string("input_method")?;
            let imm = env
                .call_method(
                    &activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[jni::JValue::Object(&service_name)],
                )?
                .l()?;
            let hidden = env
                .call_method(
                    imm,
                    jni_str!("hideSoftInputFromWindow"),
                    jni_sig!("(Landroid/os/IBinder;I)Z"),
                    &[jni::JValue::Object(&token), jni::JValue::Int(0)],
                )?
                .z()?;
            Ok(format!("收键盘={hidden}"))
        });
        match result {
            Ok(msg) => crate::report::report("ime", &msg),
            Err(_) => crate::report::report("ime", "收键盘: JNI 链路失败"),
        }
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
}
