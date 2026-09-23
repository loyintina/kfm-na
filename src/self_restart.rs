//! self_restart.rs — 完全自重启（2026-09-23 用户立项：解析页服务段
//! [重启] 钮 + agent 远程触发，两路同核）。
//!
//! 机制：AlarmManager.setAndAllowWhileIdle 预约 1s 后拉起 MainActivity
//! （PendingIntent.getActivity，FLAG_CANCEL_CURRENT|FLAG_IMMUTABLE），
//! 随即 killProcess 自杀——预约押在系统侧，进程死了照样复活，超级
//! 省电下不依赖用户手动清后台（用户原话：「超级省电模式不能自己关
//! 后台的」）。纯 JNI 实现：Java 皮零改动，热更 .so 即可投递
//! （B 档胶水：对错是「系统让不让你活」，冒烟钉防退化）。
//!
//! 两路触发同核：
//! - UI：服务段 [重启] 钮两段确认——点一次武装 3s（文案翻「再点
//!   确认」），窗内再点执行；tmux 关闭跳框同款防误触意图，轻量实现
//! - agent：files/hatch/RESTART.request 旗标文件——about_to_wait
//!   节流 1s 探一回，见旗即删即重启（隧道活着时 na_ssh touch 即达）

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// 武装窗（两段确认）：3s 内再点执行，超时自动落回常态
pub const ARM_WINDOW_MS: u64 = 3000;
/// 旗标文件相对路径（files_dir 下）：hatch = 热更目录，重启与换装同门
pub const FLAG_REL: &str = "hatch/RESTART.request";
/// 旗标探节流：1s 一探（about_to_wait 每圈都调，exists 是 syscall）
const POLL_GAP_MS: u64 = 1000;

static FILES_DIR: OnceLock<PathBuf> = OnceLock::new();
static ARMED_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static LAST_POLL_MS: AtomicU64 = AtomicU64::new(0);

/// 壳启动时登记 files 目录（旗标路径的唯一来源；重复调幂等）
pub fn set_files_dir(dir: PathBuf) {
    let _ = FILES_DIR.set(dir);
}

/// 武装判定纯函数（A 档面——全局账只是它的壳）
pub fn armed_at(armed_until_ms: u64, now_ms: u64) -> bool {
    armed_until_ms != 0 && now_ms < armed_until_ms
}

/// 武装中？（0 = 未武装；过窗自动落回）
pub fn restart_armed(now_ms: u64) -> bool {
    armed_at(ARMED_UNTIL_MS.load(Ordering::Relaxed), now_ms)
}

/// 钮文案（涂装唯一源——涂装/命中吃同一枚，两处各写必漂移）
pub fn button_label(now_ms: u64) -> &'static str {
    if restart_armed(now_ms) {
        "再点确认"
    } else {
        "重启"
    }
}

/// 点钮裁决（A 档纯函数面）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapVerdict {
    /// 未武装 → 武装 3s，等第二击
    Arm,
    /// 武装窗内第二击 → 执行重启
    Execute,
}

/// 点钮裁决纯函数：（武装账， 当下时刻） → （新账， 裁决）
pub fn tap_verdict(armed_until_ms: u64, now_ms: u64) -> (u64, TapVerdict) {
    if armed_at(armed_until_ms, now_ms) {
        (0, TapVerdict::Execute)
    } else {
        (now_ms + ARM_WINDOW_MS, TapVerdict::Arm)
    }
}

pub fn tap(now_ms: u64) -> TapVerdict {
    let (next, v) = tap_verdict(ARMED_UNTIL_MS.load(Ordering::Relaxed), now_ms);
    ARMED_UNTIL_MS.store(next, Ordering::Relaxed);
    v
}

/// 节流旗标探（壳 about_to_wait 每圈调）：1s 一探，见旗即删 → true
/// （删在报 true 前——重启若失败旗标也不赖着，防自杀循环）
pub fn poll_flag(now_ms: u64) -> bool {
    if now_ms.saturating_sub(LAST_POLL_MS.load(Ordering::Relaxed)) < POLL_GAP_MS {
        return false;
    }
    LAST_POLL_MS.store(now_ms, Ordering::Relaxed);
    let Some(dir) = FILES_DIR.get() else {
        return false;
    };
    let flag = dir.join(FLAG_REL);
    if flag.exists() {
        let _ = std::fs::remove_file(&flag);
        return true;
    }
    false
}

/// 完全自重启（B 档 JNI 胶水）：闹钟预约 → 杀本进程。Err = 预约失败
/// （进程还活着，壳上报即可）；Ok 通常不返回（killProcess 先落地）。
#[cfg(target_os = "android")]
pub fn restart(app: &winit::platform::android::activity::AndroidApp) -> Result<(), String> {
    use jni::{JValue, JavaVM, jni_sig, jni_str, objects::JObject};
    // SAFETY: vm_as_ptr 是 android-activity 保证有效的 JavaVM 指针
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw = app.activity_as_ptr() as jni::sys::jobject;
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY: 全局引用归 android-activity 所有，from_raw 只包视图不接管
        let activity = unsafe { JObject::from_raw(env, raw) };
        // 本包启动 Intent = pm.getLaunchIntentForPackage(pkg)
        let pm = env
            .call_method(
                &activity,
                jni_str!("getPackageManager"),
                jni_sig!("()Landroid/content/pm/PackageManager;"),
                &[],
            )?
            .l()?;
        let pkg_obj = env
            .call_method(
                &activity,
                jni_str!("getPackageName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let pkg = env.cast_local::<jni::objects::JString>(pkg_obj)?;
        let intent = env
            .call_method(
                &pm,
                jni_str!("getLaunchIntentForPackage"),
                jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&pkg)],
            )?
            .l()?;
        // PendingIntent.getActivity(ctx, 0, intent,
        //   FLAG_CANCEL_CURRENT(0x08000000)|FLAG_IMMUTABLE(0x04000000))
        let pi = env
            .call_static_method(
                jni_str!("android/app/PendingIntent"),
                jni_str!("getActivity"),
                jni_sig!(
                    "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;"
                ),
                &[
                    JValue::Object(&activity),
                    JValue::Int(0),
                    JValue::Object(&intent),
                    JValue::Int(0x0C00_0000),
                ],
            )?
            .l()?;
        let svc = env.new_string("alarm")?;
        let am = env
            .call_method(
                &activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(&svc)],
            )?
            .l()?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // RTC_WAKEUP(0)，1s 后拉起；Doze 下也放行（AllowWhileIdle）
        env.call_method(
            &am,
            jni_str!("setAndAllowWhileIdle"),
            jni_sig!("(IJLandroid/app/PendingIntent;)V"),
            &[JValue::Int(0), JValue::Long(now_ms + 1000), JValue::Object(&pi)],
        )?;
        // 杀本进程：android.os.Process.killProcess(myPid())
        let pid = env
            .call_static_method(
                jni_str!("android/os/Process"),
                jni_str!("myPid"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        env.call_static_method(
            jni_str!("android/os/Process"),
            jni_str!("killProcess"),
            jni_sig!("(I)V"),
            &[JValue::Int(pid)],
        )?;
        Ok(())
    })
    .map_err(|e| format!("{e}"))
}

/// 宿主桩：纯逻辑（两段确认/旗标探）在宿主有考题，restart 是
/// Android 胶水——宿主调用只可能来自编译面，给了也执行不了
#[cfg(not(target_os = "android"))]
pub fn restart(_app: &()) -> Result<(), String> {
    Err("宿主无 Android 运行时".into())
}
