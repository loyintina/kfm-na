//! na-loader — APK 里焊死的极小加载器（热更新壳，2026-08-26 与用户定）
//!
//! 为什么存在：主库 libkfm_na.so 此前被框架直接加载，改代码必须重打
//! APK、过安装器、手动点装。现在 manifest 指向本加载器（libna_loader.so，
//! 设计目标是永远不变），它在 ANativeActivity_onCreate 里做唯一一件事：
//!
//!   1. 私有目录有热更核心 {files}/hot/libkfm_na.so → dlopen 它;
//!   2. 没有或加载失败 → 回落 dlopen 包内捆绑的 libkfm_na.so（裸 soname,
//!      框架的命名空间搜索路径含 APK lib 目录，extractNativeLibs=false
//!      直映射也找得到）;
//!   3. 把选择落档 {files}/usr/tmp/loader-pick（跑的是谁必须可查）；
//!   4. dlsym 真 ANativeActivity_onCreate 转发，本壳退场。
//!
//! 热更链路：手机编出 libkfm_na.so → scp 进沙箱 hot/ → 重启 App 生效。
//! 不重打 APK、不过安装器、不碰 versionCode。
//!
//! 这也是插件宿主胚胎：壳与核心之间只隔一层 dlopen + 固定入口符号，
//! 未来核心用同一机制加载功能插件 .so（cordis-na 插件运行时的原生面）。

/// 热更核心在私有目录的位置（相对 internal_data_path）
pub const HOT_CORE_REL: &str = "hot/libkfm_na.so";
/// 包内捆绑核心的 soname（回落路径，靠框架命名空间解析）
pub const BUNDLED_CORE_SONAME: &str = "libkfm_na.so";
/// 选择落档（相对 internal_data_path，闸门目录 = usr/tmp）
pub const PICK_REC_REL: &str = "usr/tmp/loader-pick";

/// 热更核心全路径（纯函数，钉拼接格式）
pub fn hot_core_path(internal_data_path: &str) -> String {
    format!(
        "{}/{HOT_CORE_REL}",
        internal_data_path.trim_end_matches('/')
    )
}

/// 选择落档路径（纯函数）
pub fn pick_rec_path(internal_data_path: &str) -> String {
    format!(
        "{}/{PICK_REC_REL}",
        internal_data_path.trim_end_matches('/')
    )
}

/// 加载选择（纯函数，钉回落顺序：热更优先，失败/缺失才回落捆绑）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePick {
    Hot,
    Bundled,
}

pub fn pick_core(hot_exists: bool, hot_dlopen_ok: bool) -> CorePick {
    if hot_exists && hot_dlopen_ok {
        CorePick::Hot
    } else {
        CorePick::Bundled
    }
}

/// 落档行格式（纯函数，钉死：一行一案，跑的是谁一读便知）
pub fn pick_line(unix_secs: u64, pick: CorePick, detail: &str) -> String {
    let tag = match pick {
        CorePick::Hot => "hot",
        CorePick::Bundled => "bundled",
    };
    format!("unix={unix_secs} pick={tag} {detail}")
}

#[cfg(target_os = "android")]
mod imp {
    use super::*;
    use std::ffi::{CString, c_char, c_void};

    type OnCreate = unsafe extern "C" fn(*mut ndk_sys::ANativeActivity, *mut c_void, usize);

    fn now_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// 加载选择 + 落档 + 取入口符号。失败返回 None（调用方只能让活动死）
    fn load_core(internal: &str) -> Option<(CorePick, OnCreate)> {
        let hot = hot_core_path(internal);
        let hot_exists = std::path::Path::new(&hot).exists();
        let mut hot_ok = false;
        let mut handle: *mut c_void = std::ptr::null_mut();

        if hot_exists {
            let c = CString::new(hot.as_str()).ok()?;
            handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW) };
            hot_ok = !handle.is_null();
        }
        let pick = pick_core(hot_exists, hot_ok);
        if pick == CorePick::Bundled {
            let c = CString::new(BUNDLED_CORE_SONAME).ok()?;
            handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW) };
        }
        // 落档:bundled 时记回落原因(热更不存在/加载失败),排查不用猜
        let detail = match pick {
            CorePick::Hot => format!("path={hot}"),
            CorePick::Bundled if !hot_exists => "why=无热更核心".into(),
            CorePick::Bundled => "why=热更核心加载失败,已回落".into(),
        };
        let line = pick_line(now_unix(), pick, &detail);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pick_rec_path(internal))
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
        if handle.is_null() {
            return None; // 捆绑核心都加载失败 = 包坏了,无药可救
        }
        let sym = unsafe { libc::dlsym(handle, c"ANativeActivity_onCreate".as_ptr()) };
        if sym.is_null() {
            return None;
        }
        Some((pick, unsafe {
            std::mem::transmute::<*mut c_void, OnCreate>(sym)
        }))
    }

    /// 框架入口（manifest android.app.lib_name="na_loader"）：选核心、
    /// 落档、转发。本壳不做任何业务，做完即退场
    #[unsafe(no_mangle)]
    pub extern "C" fn ANativeActivity_onCreate(
        activity: *mut ndk_sys::ANativeActivity,
        saved_state: *mut c_void,
        saved_state_size: usize,
    ) {
        let internal = unsafe {
            let p = (*activity).internalDataPath;
            if p.is_null() {
                return; // 私有目录都拿不到,无法继续
            }
            std::ffi::CStr::from_ptr(p as *const c_char)
                .to_string_lossy()
                .into_owned()
        };
        if let Some((_, entry)) = load_core(&internal) {
            unsafe { entry(activity, saved_state, saved_state_size) };
        }
        // 核心全灭:不转发 = 活动自然终结,loader-pick 里留有遗言
    }
}
