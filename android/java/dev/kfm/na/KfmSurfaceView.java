package dev.kfm.na;

import android.content.Context;
import android.view.SurfaceView;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;

/**
 * 带 IME 入口的 SurfaceView——中文输入的命脉。
 *
 * Android 的中文输入不是按键事件，是输入法通过 InputConnection.commitText
 * 整串塞字（Termux/SDL 同款认知）。本 View 覆写 onCreateInputConnection
 * 给出 KfmInputConnection，落字经 JNI 直推 Rust 侧 ime_queue。
 */
public class KfmSurfaceView extends SurfaceView {
    public KfmSurfaceView(Context context) {
        super(context);
        // 必须可聚焦，InputMethodManager 才会把本 View 当输入目标
        setFocusable(true);
        setFocusableInTouchMode(true);
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        // TYPE_CLASS_TEXT：让输入法进组词模式（拼音候选）；
        // NO_EXTRACT_UI：横屏/小屏不弹全屏输入界面；
        // ACTION_NONE：回车不当「完成」键，走按键事件进终端
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT;
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_ACTION_NONE;
        return new KfmInputConnection(this);
    }

    // JNI 对侧：src/ime_bridge.rs（落字/软键 → ime_queue → 事件循环排干）
    static native void nativeCommitText(String text);

    static native void nativeSendKey(int keyCode);
}
