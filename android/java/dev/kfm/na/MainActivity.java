package dev.kfm.na;

import android.app.NativeActivity;
import android.os.Bundle;

/**
 * KFM-NA 主 Activity——唯一职责：把内容 View 换成自带 IME 连接的
 * KfmSurfaceView。
 *
 * 为什么必须换：NativeActivity 自带的 NativeContentView 没有
 * InputConnection，中文输入法的 commitText 无处可投（中文死结根源，
 * 2026-08-13 实锤 winit native-activity 后端零 Ime 事件代码）。
 * surface 回调仍指回本 Activity（NativeActivity 公开实现
 * SurfaceHolder.Callback2），原生层（winit/softbuffer）无感。
 */
public class MainActivity extends NativeActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        KfmSurfaceView view = new KfmSurfaceView(this);
        view.getHolder().addCallback(this);
        setContentView(view);
        view.requestFocus();
    }
}
