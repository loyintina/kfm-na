package dev.kfm.na;

import android.app.NativeActivity;
import android.content.Intent;
import android.media.projection.MediaProjectionManager;
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
    private static MainActivity sInstance;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        sInstance = this;
        // targetSdk 28 域降级（exec 探针放行）的副作用：系统按「旧应用」把窗口
        // 压到状态栏下面（实拍 16777514：终端不再满屏）。运行时调用不受
        // targetSdk 门控——decorFitsSystemWindows(false) 把内容铺回状态栏下，
        // 刘海区允许进（短边），几何归 Rust 侧 MARGIN_TOP 管（圆角屏下探）
        getWindow().setDecorFitsSystemWindows(false);
        android.view.WindowManager.LayoutParams lp = getWindow().getAttributes();
        lp.layoutInDisplayCutoutMode =
                android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        getWindow().setAttributes(lp);
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
        // BAR-029：前台服务保活——退后台不被 cached-app 冻结器冻住
        // （sshd 冬眠、8024 闸门断流的治本；常驻通知是 Android 硬规矩）
        startForegroundService(new android.content.Intent(this, KfmKeepAliveService.class));
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

    // ---- 软件内实录（P2，2026-09-08）：gate hook 的 Java 着陆点。
    // Android 14+ 授权必须先行（旧序 FGS 在前 = SecurityException 闪退）：
    // hook → 弹授权 → 同意才起 FGS（extras 带授权）→ 服务建投影 ----
    private static int sRecMs;

    /** 真实显示刷新率(Hz)——Rust fx 帧预算的单源（BAR-077，2026-09-10
     * 拍板：动画节拍跟 vsync 走；120Hz 屏写死 16ms=60fps 硬钳=「落下
     * 拖影」真凶，vsync 账本实测 110-120Hz 定罪）。0=查询失败（Rust
     * 侧维持旧预算）。每次 resumed 一问，系统设置切 60/120 档跟手。 */
    public float displayRefreshHz() {
        try {
            android.view.Display d = getWindowManager().getDefaultDisplay();
            if (d == null) {
                return 0f;
            }
            return d.getMode().getRefreshRate();
        } catch (Throwable t) {
            return 0f;
        }
    }

    /** 原生 gate 线程经 JNI 调（hook 注册在 android_app）——甩 UI 线程 */
    public void startRecordingFromGate(final int ms) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                sRecMs = ms;
                MediaProjectionManager mpm = (MediaProjectionManager)
                        getSystemService(MEDIA_PROJECTION_SERVICE);
                startActivityForResult(mpm.createScreenCaptureIntent(), 7001);
            }
        });
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode == 7001) {
            if (resultCode == android.app.Activity.RESULT_OK) {
                android.content.Intent svc = new android.content.Intent(this, KfmRecService.class);
                svc.putExtra("resultCode", resultCode);
                svc.putExtra("data", data);
                svc.putExtra("durationMs", sRecMs);
                startForegroundService(svc);
            } else {
                status("denied");
            }
            return;
        }
        super.onActivityResult(requestCode, resultCode, data);
    }

    private void status(String s) {
        try {
            java.io.File f = new java.io.File(getFilesDir(), "usr/tmp/rec-status");
            f.getParentFile().mkdirs();
            java.io.FileWriter w = new java.io.FileWriter(f, false);
            w.write(s);
            w.close();
        } catch (Exception ignored) {
        }
    }

    // ---- 浏览器卡尖刺（SPKE-web，2026-09-12，用户拍板立项）：gate hook 的
    // Java 着陆点。判卷双轨：web-status 状态文件（建成/异常栈）+ 真屏截图。
    // 第一钉已定罪（2026-09-12 实拍）：WebView 建成不崩（targetSdk28 兼容✓）
    // 但塞进 content FrameLayout 不可见——与 BAR-017 键行同死法：原生
    // busy-loop 每帧盖掉同窗 View。第二钉走独立窗口：WindowManager.addView
    // + TYPE_APPLICATION_PANEL（自带 surface 合成于主窗之上，原生重绘够不
    // 着；挂 activity token 免 SYSTEM_ALERT_WINDOW 权限）。
    // url="close" 收起（removeView + destroy）。----
    private android.webkit.WebView mWeb;

    /** 原生 gate 线程经 JNI 调（与 startRecordingFromGate 同槽注册） */
    public void startWebViewFromGate(final String url) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    android.view.WindowManager wm = getWindowManager();
                    if ("close".equals(url)) {
                        if (mWeb != null) {
                            wm.removeView(mWeb);
                            mWeb.destroy();
                            mWeb = null;
                        }
                        webStatus("closed");
                        return;
                    }
                    if (mWeb == null) {
                        android.webkit.WebView wv = new android.webkit.WebView(MainActivity.this);
                        android.webkit.WebSettings s = wv.getSettings();
                        s.setJavaScriptEnabled(true);
                        s.setDomStorageEnabled(true);
                        wv.setWebViewClient(new android.webkit.WebViewClient());
                        // 尖刺定版几何：整宽 × 屏高 3/5，顶部靠泊——刻意不全屏，
                        // 好让截图一次判两事（上方 WebView 活没活 + 下方原生
                        // 终端画面是否还在正常重绘）
                        int h = getResources().getDisplayMetrics().heightPixels * 3 / 5;
                        android.view.WindowManager.LayoutParams lp =
                                new android.view.WindowManager.LayoutParams(
                                        android.view.WindowManager.LayoutParams.MATCH_PARENT,
                                        h,
                                        android.view.WindowManager.LayoutParams.TYPE_APPLICATION_PANEL,
                                        0,
                                        android.graphics.PixelFormat.TRANSLUCENT);
                        lp.token = getWindow().getDecorView().getWindowToken();
                        lp.gravity = android.view.Gravity.TOP;
                        wm.addView(wv, lp);
                        mWeb = wv;
                        webStatus("added-panel h=" + h + " token=" + (lp.token != null));
                    }
                    mWeb.loadUrl(url);
                    webStatus("loading " + url);
                } catch (Throwable t) {
                    // targetSdk28 兼容性/缺 WebView 提供者/token 空等死法都在此落证
                    java.io.StringWriter sw = new java.io.StringWriter();
                    t.printStackTrace(new java.io.PrintWriter(sw));
                    webStatus("CRASH " + sw.toString());
                }
            }
        });
    }

    private void webStatus(String s) {
        try {
            java.io.File f = new java.io.File(getFilesDir(), "usr/tmp/web-status");
            f.getParentFile().mkdirs();
            java.io.FileWriter w = new java.io.FileWriter(f, false);
            w.write(s);
            w.close();
        } catch (Exception ignored) {
        }
    }
}
