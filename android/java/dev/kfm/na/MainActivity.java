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
        // BAR-145 治本臂：焦点重获 = IME 召收之外的窗口重协商时点，重贴
        // 沉浸式标志（见下）
        if (hasFocus) {
            reapplyImmersive();
        }
    }

    /** BAR-145 治本臂（2026-09-26）：唤醒/焦点变化的窗口重协商时点**重贴**
     *  沉浸式标志——病灶是输入侧窗顶被错记到状态栏之下（捕获器实测 winTop=135，
     *  显示侧全屏 0），重贴迫使系统重算窗口 frame，输入/显示两侧快照同源
     *  （沉浸式 desync 的标准修法）。点火时点 = 复现钥匙直指的 resume。
     *  安全性：本应用 surface 恒为物理整屏（1260x2800），重贴不改变可视
     *  尺寸；全过程 try 兜底，皮肤不许把壳带着一起死。 */
    private void reapplyImmersive() {
        try {
            getWindow().setDecorFitsSystemWindows(false);
            getWindow().getDecorView().setSystemUiVisibility(
                    android.view.View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                            | android.view.View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                            | android.view.View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                            | android.view.View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                            | android.view.View.SYSTEM_UI_FLAG_FULLSCREEN
                            | android.view.View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        } catch (Throwable ignored) {
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

    // ---- 全局页（BAR-260／NA0189 楼1，白露 MAIN0175 楼23）：gate hook 的
    // Java 着陆点。判卷双轨：web-status 状态文件（建成/异常栈）+ 真屏截图。
    // 承载走独立窗口：WindowManager.addView + TYPE_APPLICATION_PANEL
    // （自带 surface 合成于主窗之上，原生重绘够不着；挂 activity token 免
    // SYSTEM_ALERT_WINDOW 权限）。几何 = 整页：宽 MATCH_PARENT、高 = 屏高
    // − 顶部原生条高、纵向偏移 = 条高（让出顶部那一条给 Rust 原生条）。
    // url="close" 收起（removeView + destroy）。----
    private android.webkit.WebView mWeb;

    /** 原生 gate 线程经 JNI 调（与 startRecordingFromGate 同槽注册） */
    /** 条高单源（BAR-260 段①，NA0189 楼6 判据①）：Java 只报**密度**这个
     * 平台事实，条高由 Rust 侧 web_page::bar_px 算一次并随桥传回——两端
     * 不许各算一份。 */
    public float webDensity() {
        return getResources().getDisplayMetrics().density;
    }

    /** 全局页逐帧位移（BAR-260 段②，NA0189 楼6 判据②）：位移的**单源在
     * Rust**（seam 采样→bar_offset_px 取整），本端只照数摆窗、不自算；Rust
     * 侧值不变不会调进来（防抖在 gate::web_offset_sync）。 */
    public void webOffset(final int dx) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    if (mWeb == null) {
                        return;
                    }
                    android.view.ViewGroup.LayoutParams gp = mWeb.getLayoutParams();
                    if (!(gp instanceof android.view.WindowManager.LayoutParams)) {
                        return;
                    }
                    android.view.WindowManager.LayoutParams lp =
                            (android.view.WindowManager.LayoutParams) gp;
                    if (lp.x == dx) {
                        return;
                    }
                    lp.x = dx;
                    lp.gravity = android.view.Gravity.TOP | android.view.Gravity.LEFT;
                    getWindowManager().updateViewLayout(mWeb, lp);
                } catch (Throwable t) {
                    java.io.StringWriter sw = new java.io.StringWriter();
                    t.printStackTrace(new java.io.PrintWriter(sw));
                    webStatus("offset-fail dx=" + dx + " " + sw.toString());
                }
            }
        });
    }

    /** 平台占用带·上（NA0189 楼22/23 硬判据）：状态栏高——**一律从
     * WindowInsets 取，禁止硬编码机型数值**（135 这类数只作判卷对照，
     * 不许进代码）。取不到时返回 0（宁可保守也不猜）。 */
    public int webInsetTop() {
        try {
            android.view.WindowInsets wi = getWindow().getDecorView().getRootWindowInsets();
            if (wi == null) {
                return 0;
            }
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                return wi.getInsets(android.view.WindowInsets.Type.statusBars()).top;
            }
            return wi.getSystemWindowInsetTop();
        } catch (Throwable t) {
            webStatus("inset-top-fail " + t);
            return 0;
        }
    }

    /** 平台占用带·右（同上）：右缘系统手势条宽——取 systemGestures 与
     * mandatorySystemGestures 的较大者，再与 systemWindowInset.right 取大。 */
    public int webInsetRight() {
        try {
            android.view.WindowInsets wi = getWindow().getDecorView().getRootWindowInsets();
            if (wi == null) {
                return 0;
            }
            int r = 0;
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                r = Math.max(
                        wi.getInsets(android.view.WindowInsets.Type.systemGestures()).right,
                        wi.getInsets(android.view.WindowInsets.Type.mandatorySystemGestures()).right);
            } else if (android.os.Build.VERSION.SDK_INT >= 29) {
                r = wi.getSystemGestureInsets().right;
            }
            return Math.max(r, wi.getSystemWindowInsetRight());
        } catch (Throwable t) {
            webStatus("inset-right-fail " + t);
            return 0;
        }
    }

    public void startWebViewFromGate(final String url, final int barPx, final int yOffPx) {
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
                        // 整页几何：条高**取 Rust 传回的那一份**（单源，见
                        // web_page::bar_px）；本端不再自算 44dp×density——
                        // 条高让给 Rust 原生条，页体从条下缘铺到屏底
                        android.util.DisplayMetrics dm = getResources().getDisplayMetrics();
                        int bar = Math.max(1, barPx);
                        int h = Math.max(1, dm.heightPixels - bar);
                        android.view.WindowManager.LayoutParams lp =
                                new android.view.WindowManager.LayoutParams(
                                        android.view.WindowManager.LayoutParams.MATCH_PARENT,
                                        h,
                                        android.view.WindowManager.LayoutParams.TYPE_APPLICATION_PANEL,
                                        0,
                                        android.graphics.PixelFormat.TRANSLUCENT);
                        // BAR-260 出口红修复（NA0189 楼19–20 白露硬判据）：
                        // 页体窗**不许弹掉条区的触摸**——FLAG_NOT_TOUCH_MODAL
                        // 让窗外的条带触摸回到 na（原生条在真实窗口层序上
                        // 不受本窗遮盖：本窗顶从 bar 起，条带内无窗压着）；
                        // FLAG_NOT_FOCUSABLE 让返回键不被本窗吃掉（页面困死
                        // 的第二条出路）。**不是扩热区**——热区与绘制仍同一份
                        // （web_page::close_rect / bar_offset_px），这里只改
                        // 窗口的触摸/焦点归属。
                        lp.flags = android.view.WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                                | android.view.WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE;
                        lp.token = getWindow().getDecorView().getWindowToken();
                        lp.gravity = android.view.Gravity.TOP;
                        // yOffPx = 状态栏高 + 条高（Rust 算好传回，本端不做布局算术）
                        lp.y = Math.max(0, yOffPx);
                        wm.addView(wv, lp);
                        mWeb = wv;
                        // 判卷取证（dumpsys 对照用）：条高/页体高/纵向偏移/标志
                        // 判卷取证（清和楼23「可用带要给实测边界数」）：inset 两条
                        // 读数＋条带几何一并落，实测数从这里抄，代码里不写死
                        webStatus("added-page bar=" + bar + " yOff=" + lp.y + " h=" + h
                                + " insetTop=" + webInsetTop() + " insetRight=" + webInsetRight()
                                + " flags=0x" + Integer.toHexString(lp.flags)
                                + " token=" + (lp.token != null));
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

    // ---- 自更新原语（BAR-162，2026-09-26）：gate 通道十五 install-apk-req
    // 的 Java 着陆点。缘起：桥 shell 路已证死（研究线实跑）——经 QUIC 反连桥
    // 在 na 沙箱里 am start（termux-am 壳，na uid）被 vivo 按进程态判 BAL
    // 静默吞，连浏览器 VIEW 都不弹、exit=0 无输出，而 pm list packages 照通
    // （IPC 活）= 不是壳坏，是桥 spawn 的 app_process 不是可见 Activity 的
    // 宿主进程；给「安装未知应用」全权限也无用（BAL 判进程态不判权限）。
    // 故意图改由 MainActivity（前台进程）自己发起：门线程 → JNI → 本方法
    // （UI 线程）→ 系统安装器。
    // URI 由 Rust 侧先落定进 {files}/incoming/ 再拼（KfmFileProvider 的根
    // 就锁在那儿，见 src/install.rs），本方法只管「从 Activity 上下文递出去」
    // 这一段。现行装机 APK 没有本方法时，Rust 侧走引导腿（门线程直调
    // activity.startActivity）——自更新原语的零引导义。
    /** 判决落 usr/tmp/install-status + motion-java.log 时序（判卷一条账）。
     *  参数 = content://dev.kfm.na.provider/apk/&lt;名&gt; */
    public void installApkFromGate(final String uri) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                String verdict;
                try {
                    android.content.Intent it = new android.content.Intent(
                            android.content.Intent.ACTION_VIEW);
                    it.setDataAndType(android.net.Uri.parse(uri),
                            "application/vnd.android.package-archive");
                    it.addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION
                            | android.content.Intent.FLAG_ACTIVITY_NEW_TASK);
                    startActivity(it);
                    verdict = "ok leg=java uri=" + uri;
                } catch (android.content.ActivityNotFoundException e) {
                    verdict = "NO_HANDLER leg=java uri=" + uri + "（无接收者）";
                } catch (Throwable t) {
                    verdict = "ERR leg=java uri=" + uri + " —— " + t;
                }
                appendMotion("install-apk " + verdict);
                installStatus(verdict);
            }
        });
    }

    /** 自更新原语判决落账（快照覆盖写；与 web-status/rec-status 同族，
     *  单独一个文件免得跟录音/浏览器的账混流） */
    private void installStatus(String s) {
        try {
            java.io.File f = new java.io.File(getFilesDir(), "usr/tmp/install-status");
            f.getParentFile().mkdirs();
            java.io.FileWriter w = new java.io.FileWriter(f, false);
            w.write(s + "\n");
            w.close();
        } catch (Exception ignored) {
        }
    }

    private void appendMotion(String line) {
        try {
            java.io.File f = new java.io.File(getFilesDir(), "usr/tmp/motion-java.log");
            f.getParentFile().mkdirs();
            java.io.FileWriter w = new java.io.FileWriter(f, true);
            w.write(android.os.SystemClock.uptimeMillis() + " " + line + "\n");
            w.close();
        } catch (Exception ignored) {
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        reapplyImmersive();
    }
}
