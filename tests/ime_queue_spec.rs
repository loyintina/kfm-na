//! ime_queue_spec.rs — IME 文字注入队列考题（A 档纯逻辑）
//!
//! 判卷维度：
//! - commitText 中文串顺序保持、排干即空、空串不注入
//! - 软键码映射（ENTER/DEL → 终端字节，未知键吞掉）
//! - UTF-8 原样透传（CJK/emoji 混合不变形）
//!
//! 变异抽检：故意改坏答案（drain 不清队 / DEL 映射成 '\x08'）本文件必须红。
//! 答案 src/ime_queue.rs；JNI 薄皮（ime_bridge.rs）判卷在真机。

use kfm_na::ime_queue::{ImeQueue, KEYCODE_DEL, KEYCODE_ENTER, key_code_to_bytes};

// ---------- A 档：队列行为 ----------

#[test]
fn spec_队列_中文提交顺序保持() {
    let q = ImeQueue::new();
    q.push_text("你好");
    q.push_text("世界");
    q.push_text("kfm");
    assert_eq!(q.drain(), vec!["你好", "世界", "kfm"]);
}

#[test]
fn spec_队列_排干即空() {
    let q = ImeQueue::new();
    q.push_text("一次");
    assert_eq!(q.drain().len(), 1);
    assert!(q.drain().is_empty(), "排干后必须空——重复注入就是鬼打字");
}

#[test]
fn spec_队列_空串不注入() {
    let q = ImeQueue::new();
    q.push_text("");
    assert!(q.drain().is_empty());
}

#[test]
fn spec_队列_utf8原样透传() {
    let q = ImeQueue::new();
    let mixed = "a你🦀好";
    q.push_text(mixed);
    let out = q.drain();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], mixed, "注入通道不得动用户文字的一个字节");
    assert_eq!(out[0].as_bytes(), mixed.as_bytes());
}

// ---------- A 档：软键码映射 ----------

#[test]
fn spec_键码_回车删除映射() {
    // 与 android_app::handle_key 的键盘映射同表——软/硬键盘行为一致
    assert_eq!(key_code_to_bytes(KEYCODE_ENTER), Some("\r"));
    assert_eq!(key_code_to_bytes(KEYCODE_DEL), Some("\x7f"));
}

#[test]
fn spec_键码_未知键吞掉() {
    assert_eq!(key_code_to_bytes(999), None);
    let q = ImeQueue::new();
    assert!(!q.push_key_code(999), "未知键必须回报未消费");
    assert!(q.drain().is_empty(), "未知键绝不注入队列");
    assert!(q.push_key_code(KEYCODE_DEL));
    assert_eq!(q.drain(), vec!["\x7f"]);
}

// ---------- A 档：全局实例冒烟（唯一碰全局的考题，防并行互踩） ----------

#[test]
fn spec_全局队列_冒烟() {
    let g = kfm_na::ime_queue::global();
    g.push_text("全局冒烟一针");
    let out = g.drain();
    assert!(
        out.iter().any(|s| s == "全局冒烟一针"),
        "全局实例 push→drain 闭环断了: {out:?}"
    );
}
