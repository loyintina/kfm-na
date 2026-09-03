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
//! 2026-08-14 快捷键行改造：队列存原始 Inject（文本/键码），键码→序列的
//! 翻译挪到排干侧——方向键/End 的序列分普通/应用光标模式，模式位只有
//! 事件循环里的 Term 知道（keymap.rs 吃 app_cursor 参数）。
//!
//! B 档 JNI 薄皮在 ime_bridge.rs（cfg android），判卷 = 真机实拍
//! 「拼音打你好选词 → 终端出现你好」。

use std::collections::VecDeque;
use std::sync::Mutex;

/// 一注输入：commitText 落字（文本）、快捷键行键码（原始值，排干侧翻译）、
/// 或 IME 组合态（输入栏 preedit；2026-09-01 编辑对齐第 1 批）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inject {
    Text(String),
    Key(i32),
    /// 组合态文本（setComposingText；空串 = 组合清空）
    Composing(String),
    /// 组合结束（finishComposingText）
    ComposingEnd,
    /// 空 commitText（BAR-054）：IME 契约「commit 文本替换当前选区」，
    /// 空串 = 有选区即删选区——输入法工具栏「剪切」的删除半真身。
    /// 曾被判「空串不注入」丢掉：那对终端分支是对的（排干侧吞掉），
    /// 对输入栏分支是剪切的死刑——队列只存事实，消费侧按焦点分流
    CommitEmpty,
    /// IME 上下文菜单动作（performContextMenuAction；2026-09-02 曲线救国：
    /// 系统剪贴板被 ROM 锁死，输入法工具栏的复制/粘贴/全选/剪切走这里直抵状态核）
    ContextMenuAction(String),
}

/// 一次注入会话的 FIFO 队列。push 侧 = JNI 回调线程，drain 侧 = 事件循环
pub struct ImeQueue {
    q: Mutex<VecDeque<Inject>>,
}

impl ImeQueue {
    pub const fn new() -> Self {
        Self {
            q: Mutex::new(VecDeque::new()),
        }
    }

    /// commitText 落字入队。空串入队为 CommitEmpty（BAR-054：IME 剪切
    /// 删除半 = 空 commit 删选区，判卷 spec_bar054_空commit入队为删选区指令）
    pub fn push_commit(&self, text: &str) {
        let item = if text.is_empty() {
            Inject::CommitEmpty
        } else {
            Inject::Text(text.to_string())
        };
        // Mutex 中毒 = 之前持有方 panic 过——取回数据继续活，
        // 输入队列不为一次 panic 陪葬
        self.q
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(item);
    }

    /// 软键/快捷键行事件入队（原始键码，排干侧按当前光标模式翻序列）。
    /// 键的合法性不问模式（key_seq 两种模式同真值表），未知键吞掉 → false
    pub fn push_key_code(&self, code: i32) -> bool {
        if crate::keymap::key_seq(code, false).is_none() {
            return false;
        }
        self.q
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Inject::Key(code));
        true
    }

    /// 组合态文本入队（输入栏 preedit）。**空串注入合法** = 组合清空
    /// （与 push_commit 的空串语义不同：组合态的「无」是状态不是删选区）
    pub fn push_composing(&self, text: &str) {
        self.q
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Inject::Composing(text.to_string()));
    }

    /// 组合结束入队（finishComposingText）
    pub fn push_composing_end(&self) {
        self.q
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Inject::ComposingEnd);
    }

    /// IME 上下文菜单动作入队（performContextMenuAction）
    pub fn push_context_menu_action(&self, action: &str) {
        self.q
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Inject::ContextMenuAction(action.to_string()));
    }

    /// 事件循环侧排干：一次性取走全部待注入项（FIFO），队列归空
    pub fn drain(&self) -> Vec<Inject> {
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
