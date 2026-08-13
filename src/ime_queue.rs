//! ime_queue.rs — IME 文字注入队列（A 档：host 可测核心）
//!
//! 中文输入链路（2026-08-13 定案，SDL/GameActivity 同款）：
//!   Java KfmInputConnection.commitText → JNI nativeCommitText
//!     → 本队列 push → 事件循环 about_to_wait 排干 → TermCmd::Input
//!
//! 为什么需要它：Android 的中文输入不是按键，是输入法通过 Java 层
//! InputConnection.commitText 整串塞字。NativeActivity 没有这套 Java
//! 接口（中文死结根源），winit native-activity 后端零 Ime 事件代码
//! （2026-08-13 实锤），Java 皮 + 本队列是平台层唯一活路。
//! push 侧跑在 JNI 回调线程，drain 侧跑在事件循环线程——跨线程过桥，
//! 所以是 Mutex 队列而不是直接调 winit。
//!
//! B 档 JNI 薄皮在 ime_bridge.rs（cfg android），判卷 = 真机实拍
//! 「拼音打你好选词 → 终端出现你好」。

use std::collections::VecDeque;
use std::sync::Mutex;

/// Android KeyEvent 键码（android.view.KeyEvent）
pub const KEYCODE_ENTER: i32 = 66;
pub const KEYCODE_DEL: i32 = 67;

/// 软键 → 终端输入字节。只翻尖刺期键盘映射已支持的键
/// （与 android_app::handle_key 同表），未知键 → None（吞掉，不注入垃圾）
pub fn key_code_to_bytes(code: i32) -> Option<&'static str> {
    match code {
        KEYCODE_ENTER => Some("\r"),
        KEYCODE_DEL => Some("\x7f"),
        _ => None,
    }
}

/// 一次注入会话的 FIFO 队列。push 侧 = JNI 回调线程，drain 侧 = 事件循环
pub struct ImeQueue {
    q: Mutex<VecDeque<String>>,
}

impl ImeQueue {
    pub const fn new() -> Self {
        Self {
            q: Mutex::new(VecDeque::new()),
        }
    }

    /// commitText 落字入队；空串不注入（判卷 spec_队列_空串不注入）
    pub fn push_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        // Mutex 中毒 = 之前持有方 panic 过——取回数据继续活，
        // 输入队列不为一次 panic 陪葬
        self.q
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(text.to_string());
    }

    /// 软键事件（退格/回车）；已消费 → true，未知键吞掉 → false
    pub fn push_key_code(&self, code: i32) -> bool {
        match key_code_to_bytes(code) {
            Some(bytes) => {
                self.push_text(bytes);
                true
            }
            None => false,
        }
    }

    /// 事件循环侧排干：一次性取走全部待注入文字（FIFO），队列归空
    pub fn drain(&self) -> Vec<String> {
        let mut q = self.q.lock().unwrap_or_else(|e| e.into_inner());
        q.drain(..).collect()
    }
}

impl Default for ImeQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局实例：单 Activity 单事件循环，JNI 回调与 android_app 共享这一格
static GLOBAL: ImeQueue = ImeQueue::new();

pub fn global() -> &'static ImeQueue {
    &GLOBAL
}
