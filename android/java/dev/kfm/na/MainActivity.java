package dev.kfm.na;

import android.app.NativeActivity;
import android.os.Bundle;
import android.widget.FrameLayout;

/**
 * KFM-NA 主 Activity——唯一职责：在原生内容之上叠一格 1px 的 IME 焦点占位
 * View（KfmImeView），让中文输入法的 commitText 有处可投。
 *
 * 为什么不能替换内容 View（BAR-008 实拍教训）：NativeActivity 的
 * NativeContentView 与窗口 surface（takeSurface）的回调时序是原生渲染的
 * 命脉——把内容 View 换成自带 SurfaceView 后，原生层绑到不可见的 surface，
 * 画面全黑，只有切后台的间隙能瞥见真终端（2026-08-13 实拍）。
 * 原生渲染路径一行不动，IME 走焦点正交注入：input queue 被 NativeActivity
 * 整窗接管（按键/触摸直达原生层，与 View 焦点无关），焦点给谁只决定
 * InputMethodManager 用谁的 InputConnection。
 */
public class MainActivity extends NativeActivity {
    private KfmImeView mIme;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // BAR-012：占位 View 是文本编辑器且持焦点，IMM 进场会自动弹键盘
        // （实拍：启动完成即弹，用户没点任何东西）——STATE_HIDDEN 压住，
        // 键盘只能由触摸主动召唤（touch → JNI SHOW_FORCED）
        getWindow().setSoftInputMode(
                android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_HIDDEN
                        | android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        FrameLayout root = findViewById(android.R.id.content);
        KfmImeView ime = new KfmImeView(this);
        ime.setLayoutParams(new FrameLayout.LayoutParams(1, 1));
        root.addView(ime);
        ime.requestFocus();
        mIme = ime;
        // 探针延迟 3s 发：onCreate 时 Rust 侧 report flusher 可能还没起
        // （enqueue 直接丢），delay 后通道必然就绪
        ime.postDelayed(() -> KfmImeView.imeLog("IME 占位已叠加, focus=" + ime.isFocused()), 3000);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // 焦点重请求（BAR-012③ 嫌疑）：onCreate 里 requestFocus 时窗口还没拿到
        // 焦点，请求可能落空——窗口真拿到焦点时再请求一次，IMM 才有输入目标
        if (hasFocus && mIme != null) {
            mIme.requestFocus();
        }
    }
}
