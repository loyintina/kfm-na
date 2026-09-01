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
}
