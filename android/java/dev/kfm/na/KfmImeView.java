package dev.kfm.na;

import android.content.Context;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;

/**
 * 1px IME 焦点占位 View——中文输入的命脉。
 *
 * 不可见、不绘制、不碰 surface（BAR-008：碰 surface 就是动原生渲染的命）。
 * 唯一职责：持焦点、在输入法询问时给出 KfmInputConnection。
 * Android 的中文输入不是按键事件，是输入法通过 InputConnection.commitText
 * 整串塞字（Termux/SDL 同款认知），落字经 JNI 直推 Rust 侧 ime_queue。
 *
 * BAR-009：onCheckIsTextEditor 必须覆写为 true——View 默认 false，
 * InputMethodManager.showSoftInput 检查输入目标不是文本编辑器就拒绝弹键盘
 * （SDL 的 DummyEdit 同款覆写）。没有这条，占位 View 一拿焦点键盘就哑。
 */
public class KfmImeView extends View {
    public KfmImeView(Context context) {
        super(context);
        // 必须可聚焦，InputMethodManager 才会把本 View 当输入目标
        setFocusable(true);
        setFocusableInTouchMode(true);
    }

    // JNI 探针统一入口：探针是诊断工具，绝不杀死 Activity——JNI 符号缺失
    // （dex/so 版本错配，BAR-011）等一切 Throwable 都吞掉降级
    static void imeLog(String msg) {
        try {
            nativeImeLog(msg);
        } catch (Throwable t) {
            // 吞：丢一行探针好过崩一次
        }
    }

    // 落字/软键同样走防护入口（KfmInputConnection 专用）：BAR-011 契约——
    // Java 侧任何 JNI 调用都不许裸奔，符号缺失 = 输入哑火，不许 = 闪退
    static void commitText(String text) {
        try {
            nativeCommitText(text);
        } catch (Throwable t) {
            // 吞
        }
    }

    static void sendKey(int keyCode) {
        try {
            nativeSendKey(keyCode);
        } catch (Throwable t) {
            // 吞
        }
    }

    @Override
    public boolean onCheckIsTextEditor() {
        return true; // BAR-009：声明「我是文本编辑器」，IMM 才肯弹键盘
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        // TYPE_CLASS_TEXT：让输入法进组词模式（拼音候选）；
        // NO_EXTRACT_UI：横屏/小屏不弹全屏输入界面；
        // ACTION_NONE：回车不当「完成」键，走按键事件进终端
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT;
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_ACTION_NONE;
        imeLog("IMM 询问 InputConnection——已给出");
        return new KfmInputConnection(this);
    }

    @Override
    protected void onFocusChanged(boolean gainFocus, int direction, android.graphics.Rect previouslyFocusedRect) {
        super.onFocusChanged(gainFocus, direction, previouslyFocusedRect);
        imeLog("IME 占位焦点变化: " + (gainFocus ? "拿到" : "丢了"));
    }

    // JNI 对侧：src/ime_bridge.rs（落字/软键 → ime_queue → 事件循环排干）
    static native void nativeCommitText(String text);

    static native void nativeSendKey(int keyCode);

    // 链路探针：Java 侧断点直送飞鸽传书（B 档平台胶水，判卷 = 上报行）
    static native void nativeImeLog(String msg);
}
