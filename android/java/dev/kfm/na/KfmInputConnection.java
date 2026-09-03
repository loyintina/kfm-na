package dev.kfm.na;

import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;

/**
 * IME 连接：commitText 落字 → JNI 直推 Rust。
 *
 * 接上本连接后软键盘的删除/回车走连接回调而非按键队列——不覆写
 * deleteSurroundingText / sendKeyEvent 的话，开中文输入法后退格直接哑火。
 * sendKeyEvent 的可打印键翻字符走 commitText 通道（部分输入法英文字母也
 * 走这里，不翻就丢字）。组词预览（preedit）：setComposingText/
 * finishComposingText 原样转发 native——消费侧按焦点分流（输入栏聚焦
 * → 拼音上栏；终端 → 沿革吞掉，BAR-012 语义不变）。2026-09-01 编辑对齐。
 */
final class KfmInputConnection extends BaseInputConnection {
    KfmInputConnection(View target) {
        super(target, true);
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        if (text != null && text.length() > 0) {
            KfmImeView.commitText(text.toString());
        }
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
        // 软退格：翻成 KEYCODE_DEL 送原生侧映射（ime_queue::key_code_to_bytes）
        // BAR-054 探针：IME 剪切的删除半走这里——记下调用与当时选区状态
        String sel = KfmImeView.selectedText();
        KfmImeView.imeLog("deleteSurroundingText(" + beforeLength + "," + afterLength
                + ") 选区=" + (sel == null ? "null" : sel.length() + "字"));
        int n = Math.min(beforeLength, 64);
        for (int i = 0; i < n; i++) {
            KfmImeView.sendKey(KeyEvent.KEYCODE_DEL);
        }
        return true;
    }

    @Override
    public boolean sendKeyEvent(KeyEvent event) {
        if (event.getAction() == KeyEvent.ACTION_DOWN) {
            int code = event.getKeyCode();
            if (code == KeyEvent.KEYCODE_DEL || code == KeyEvent.KEYCODE_ENTER) {
                KfmImeView.sendKey(code);
            } else {
                // 可打印键（字母/数字/符号）翻字符走落字通道
                int ch = event.getUnicodeChar();
                if (ch != 0) {
                    KfmImeView.commitText(new String(Character.toChars(ch)));
                }
            }
        }
        return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        // 组词预览原样转发（消费侧按焦点分流）；必须收——不收部分输入法罢工
        if (text != null) {
            KfmImeView.composingText(text.toString());
        }
        return true;
    }

    @Override
    public boolean finishComposingText() {
        KfmImeView.finishComposing();
        return true;
    }

    @Override
    public boolean performEditorAction(int actionCode) {
        return true;
    }

    @Override
    public CharSequence getSelectedText(int flags) {
        // BAR-054：输入法工具栏按「剪切」前先探选区，默认实现不认
        // Rust 状态核恒返回空 → 输入法判「无物可剪」不发 cut 事件。
        // 直问状态核拿真实选区；无选区返回 null（契约：无选区 = null）
        CharSequence sel = KfmImeView.selectedText();
        // 探针：剪切全链路第一环——IME 到底来不来问、问到什么
        KfmImeView.imeLog("getSelectedText 探询 → "
                + (sel == null ? "null" : sel.length() + "字"));
        return sel;
    }

    @Override
    public boolean performContextMenuAction(int id) {
        // 输入法工具栏的复制/剪切/粘贴/全选（2026-09-02 曲线救国：
        // 系统剪贴板被 ROM 锁死，这些命令直送 Rust 状态核，不走系统剪贴板）
        String action = null;
        if (id == android.R.id.selectAll) action = "selectAll";
        else if (id == android.R.id.cut) action = "cut";
        else if (id == android.R.id.copy) action = "copy";
        else if (id == android.R.id.paste) action = "paste";
        if (action != null) {
            KfmImeView.contextMenuAction(action);
            return true;
        }
        return super.performContextMenuAction(id);
    }
}
