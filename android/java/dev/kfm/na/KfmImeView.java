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
    static {
        // BAR-012③ 三轮：libkfm_na.so 由框架 NativeActivity 加载，挂在框架
        // classloader 名下——本类（应用 classloader）做 JNI 懒解析时在自己的
        // native 库清单里找不到符号，UnsatisfiedLinkError 被下面的 try/catch
        // 静默吞掉（实拍：键盘能弹但三年无一字进 Rust，探针全灭）。
        // 对已加载的库再 loadLibrary 是幂等的，副作用正是把它登记进应用
        // classloader 的库清单——懒解析立刻能命中。
        //
        // BAR-039(2026-08-26):loadLibrary 目标焊死为 na_loader——热更核心
        // (hot/ 绝对路径 dlopen)与包内捆绑 libkfm_na.so 是两个实例,IME 若
        // 绑到捆绑副本,commit 进它的静态队列,运行核心永远抽不到(装机实拍
        // commit 计数恒 0)。na_loader 导出同名 JNI 符号原样转发当前核心,
        // 热更换核 Java 无感。
        try {
            System.loadLibrary("na_loader");
        } catch (Throwable t) {
            // 吞：登记失败也绝不杀死 Activity（BAR-011 契约）
        }
    }

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

    // 组合态文本(setComposingText;输入栏 preedit,2026-09-01 编辑对齐)。
    // 消费侧按焦点分流:输入栏聚焦→上栏,终端→沿革吞掉。空串=清组合
    static void composingText(String text) {
        try {
            nativeComposingText(text);
        } catch (Throwable t) {
            // 吞:旧 .so 无此符号(BAR-011),组合预览消失好过崩
        }
    }

    static void finishComposing() {
        try {
            nativeFinishComposing();
        } catch (Throwable t) {
            // 吞
        }
    }

    // IME 上下文菜单动作（performContextMenuAction；2026-09-02 曲线救国：
    // 系统剪贴板被 ROM 锁死，输入法工具栏命令直送 Rust 状态核）
    static void contextMenuAction(String action) {
        try {
            nativeContextMenuAction(action);
        } catch (Throwable t) {
            // 吞：旧 .so 无此符号(BAR-011)，菜单哑火好过崩
        }
    }

    // 当前选区文本直问 Rust 状态核（BAR-054：输入法按「剪切」前先
    // getSelectedText() 探选区，默认实现不认我们的状态核恒返回空，
    // 输入法判「无物可剪」根本不发 cut 事件——工具栏剪切哑火根源）
    static String selectedText() {
        try {
            return nativeSelectedText();
        } catch (Throwable t) {
            return null; // 吞：旧 .so 无此符号，哑火返回无选区
        }
    }

    // 以下四个同属 BAR-054 防护入口（BAR-011 契约：JNI 调用不裸奔）
    static String textBeforeCursor(int n) {
        try {
            String t = nativeTextBeforeCursor(n);
            return t == null ? "" : t;
        } catch (Throwable t) {
            return "";
        }
    }

    static String textAfterCursor(int n) {
        try {
            String t = nativeTextAfterCursor(n);
            return t == null ? "" : t;
        } catch (Throwable t) {
            return "";
        }
    }

    static void setSel(int start, int end) {
        try {
            nativeSetSelection(start, end);
        } catch (Throwable t) {
            // 吞
        }
    }

    static void replaceText(int start, int end, String text) {
        try {
            nativeReplaceText(start, end, text);
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
        // TYPE_CLASS_TEXT + VISIBLE_PASSWORD（BAR-012，Termux 同款）：
        // 禁自动纠错/联想——否则 Gboard 对英文也开组词，字母走
        // setComposingText 攒词不 commit，终端永远不见字。
        // 中文组词（拼音候选）不受影响，选词仍走 commitText。
        // NO_EXTRACT_UI：横屏/小屏不弹全屏输入界面；
        // ACTION_NONE：回车不当「完成」键，走按键事件进终端
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT | EditorInfo.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
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
    static native void nativeComposingText(String text);
    static native void nativeFinishComposing();
    static native void nativeContextMenuAction(String action);

    // BAR-054：选区文本直问状态核（对侧 src/ime_bridge.rs），无选区/未登记 = null
    static native String nativeSelectedText();

    // BAR-054 续：IME 删除/替换/直设路径——光标前后查询 + setSelection + replaceText
    static native String nativeTextBeforeCursor(int n);

    static native String nativeTextAfterCursor(int n);

    static native void nativeSetSelection(int start, int end);

    static native void nativeReplaceText(int start, int end, String text);

    // 链路探针：Java 侧断点直送飞鸽传书（B 档平台胶水，判卷 = 上报行）
    static native void nativeImeLog(String msg);
}
