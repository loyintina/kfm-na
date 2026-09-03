//! ime_queue_spec.rs — IME 文字注入队列考题（A 档纯逻辑）
//!
//! 判卷维度：
//! - commitText 中文串顺序保持、排干即空、空 commit 入队为删选区指令（BAR-054）
//! - Inject 双形态：文本原样、键码存原始值（翻译在排干侧，见 keymap_spec）
//! - UTF-8 原样透传（CJK/emoji 混合不变形）
//!
//! 变异抽检：故意改坏答案（drain 不清队 / 未知键入队）本文件必须红。
//! 答案 src/ime_queue.rs；JNI 薄皮（ime_bridge.rs）判卷在真机。

use kfm_na::ime_queue::{ImeQueue, Inject};

// ---------- A 档：队列行为 ----------

#[test]
fn spec_队列_中文提交顺序保持() {
    let q = ImeQueue::new();
    q.push_commit("你好");
    q.push_commit("世界");
    q.push_commit("kfm");
    assert_eq!(
        q.drain(),
        vec![
            Inject::Text("你好".into()),
            Inject::Text("世界".into()),
            Inject::Text("kfm".into()),
        ]
    );
}

#[test]
fn spec_队列_排干即空() {
    let q = ImeQueue::new();
    q.push_commit("一次");
    assert_eq!(q.drain().len(), 1);
    assert!(q.drain().is_empty(), "排干后必须空——重复注入就是鬼打字");
}

#[test]
fn spec_bar054_空commit入队为删选区指令() {
    // BAR-054 定案：IME 工具栏「剪切」的删除半 = commitText("")——
    // Android 契约里 commit 文本替换当前选区，空串即删选区。
    // Java 侧 length>0 守卫 + 本队列空串丢弃曾把它双重静默吞掉
    // （第四刀全探针静默的真相：这道指令从没被观测过）。
    let q = ImeQueue::new();
    q.push_commit("");
    q.push_commit("落字");
    assert_eq!(
        q.drain(),
        vec![Inject::CommitEmpty, Inject::Text("落字".into())],
        "空 commit 是「有选区即删选区」的 IME 契约指令，必须入队，不许当垃圾丢"
    );
}

#[test]
fn spec_队列_utf8原样透传() {
    let q = ImeQueue::new();
    let mixed = "a你🦀好";
    q.push_commit(mixed);
    let out = q.drain();
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0],
        Inject::Text(mixed.into()),
        "注入通道不得动用户文字的一个字节"
    );
}

// ---------- A 档：键码入队（原始值，翻译见 keymap_spec） ----------

#[test]
fn spec_键码_原始键码入队() {
    // 键码→序列的翻译在排干侧（模式分岔），队列只存原始码：
    // 方向键进队列必须还是键码形态，不许提前翻成定死序列
    let q = ImeQueue::new();
    assert!(q.push_key_code(19), "方向键上必须可入队");
    assert!(q.push_key_code(66), "回车必须可入队");
    assert_eq!(q.drain(), vec![Inject::Key(19), Inject::Key(66)]);
}

#[test]
fn spec_键码_未知键吞掉() {
    let q = ImeQueue::new();
    assert!(!q.push_key_code(999), "未知键必须回报未消费");
    assert!(q.drain().is_empty(), "未知键绝不注入队列");
}

// ---------- A 档：全局实例冒烟（唯一碰全局的考题，防并行互踩） ----------

#[test]
fn spec_全局队列_冒烟() {
    let g = kfm_na::ime_queue::global();
    g.push_commit("全局冒烟一针");
    let out = g.drain();
    assert!(
        out.iter()
            .any(|i| matches!(i, Inject::Text(s) if s == "全局冒烟一针")),
        "全局实例 push→drain 闭环断了: {out:?}"
    );
}

// ---------- 组合态注入（2026-09-01 编辑对齐第 1 批） ----------

#[test]
fn spec_队列_组合态往返() {
    let q = ImeQueue::new();
    q.push_composing("nihao");
    q.push_composing(""); // 空串 = 组合清空(合法注入,与 Text 空串语义不同)
    q.push_composing_end();
    assert_eq!(
        q.drain(),
        vec![
            Inject::Composing("nihao".to_string()),
            Inject::Composing(String::new()),
            Inject::ComposingEnd,
        ],
        "组合态三连往返:文本/清空/结束,顺序保持"
    );
    assert!(q.drain().is_empty(), "排干即空");
}

// ---------- IME 上下文菜单动作（2026-09-02 曲线救国） ----------

#[test]
fn spec_队列_上下文菜单动作入队() {
    let q = ImeQueue::new();
    q.push_context_menu_action("selectAll");
    q.push_context_menu_action("copy");
    q.push_context_menu_action("paste");
    assert_eq!(
        q.drain(),
        vec![
            Inject::ContextMenuAction("selectAll".into()),
            Inject::ContextMenuAction("copy".into()),
            Inject::ContextMenuAction("paste".into()),
        ],
        "上下文菜单动作必须原样入队、顺序保持"
    );
}
