package dev.kfm.na;

import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.InputConnection;

/**
 * IME 连接：commitText 落字 → JNI 直推 Rust。
 *
 * 接上本连接后软键盘的删除/回车走连接回调而非按键队列——不覆写
 * deleteSurroundingText / sendKeyEvent 的话，开中文输入法后退格直接哑火。
 * 尖刺 v1 不做组词预览（preedit）：候选栏由输入法自绘，落字才进终端。
 */
final class KfmInputConnection extends BaseInputConnection {
    KfmInputConnection(View target) {
        super(target, true);
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        if (text != null && text.length() > 0) {
            KfmSurfaceView.nativeCommitText(text.toString());
        }
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
        // 软退格：翻成 KEYCODE_DEL 送原生侧映射（ime_queue::key_code_to_bytes）
        int n = Math.min(beforeLength, 64);
        for (int i = 0; i < n; i++) {
            KfmSurfaceView.nativeSendKey(KeyEvent.KEYCODE_DEL);
        }
        return true;
    }

    @Override
    public boolean sendKeyEvent(KeyEvent event) {
        KfmSurfaceView.nativeSendKey(event.getKeyCode());
        return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        // 组词预览尖刺期不上屏，但必须收——不收的话部分输入法直接罢工
        return true;
    }

    @Override
    public boolean finishComposingText() {
        return true;
    }

    @Override
    public boolean performEditorAction(int actionCode) {
        return true;
    }
}
