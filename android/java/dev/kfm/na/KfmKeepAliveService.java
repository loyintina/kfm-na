package dev.kfm.na;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.Intent;
import android.os.Build;
import android.os.IBinder;
import android.os.PowerManager;

/**
 * BAR-029 保活前台服务（Termux 同款路子）。
 *
 * 病灶：na 退后台被 Android cached-app 冻结器整进程冻住——sshd 冬眠
 * （TCP 握手由内核 backlog 完成但 banner 发不出，8024 闸门断流）、
 * tmux/会话全僵。进程一提为「前台服务」重要性，冻结器就不动它。
 *
 * 代价：常驻通知一条（Android 硬规矩，IMPORTANCE_MIN 低打扰渠道）。
 * 配套 WAKE_LOCK：息屏/Doze 下保住 CPU（Termux 同款 partial wake lock）。
 */
public class KfmKeepAliveService extends Service {
    /** 心跳间隔（BAR-260 出口重修第四件，NA0189 楼52/53）：判活要判**可观察
     * 进展**，不判进程在不在（MAIN0182 第四通则）。每 30s 一条：通知文本带
     * 心跳序号 + logcat 落一行带时刻——死前最后一条即死亡时刻证据。 */
    private static final long HB_MS = 30000L;
    private static final String TAG = "kfm_na_keepalive";
    private final android.os.Handler mH = new android.os.Handler(android.os.Looper.getMainLooper());
    private int mHb = 0;
    private volatile boolean mStop = false;
    private final Runnable mBeat = new Runnable() {
        @Override
        public void run() {
            if (mStop) {
                return;
            }
            mHb++;
            long now = System.currentTimeMillis();
            android.util.Log.i(TAG, "hb=" + mHb + " at=" + now + " fgs=alive");
            try {
                Notification n = buildNotification("心跳 " + mHb + " · " + now);
                NotificationManager nm = getSystemService(NotificationManager.class);
                if (nm != null) {
                    nm.notify(NOTIF_ID, n); // 同 id 覆盖：心跳可见且不留多条
                }
            } catch (Throwable t) {
                android.util.Log.e(TAG, "hb-notify-fail hb=" + mHb + " " + t);
            }
            mH.postDelayed(this, HB_MS);
        }
    };
    private static final String CHANNEL_ID = "kfm_na_keepalive";
    private static final int NOTIF_ID = 1;
    private PowerManager.WakeLock mWakeLock;

    @Override
    public void onCreate() {
        super.onCreate();
        // 兜底（NA0189 楼53 清和批准）：建渠道/startForeground/拿 WakeLock 任一
        // 步抛，整条 FGS 会静默没起来——异常必须落痕，不许静默
        try {
            onCreateInner();
        } catch (Throwable t) {
            android.util.Log.e(TAG, "fgs-boot-fail " + t);
            try {
                stopSelf();
            } catch (Throwable ignored) {
            }
            return;
        }
        mH.postDelayed(mBeat, HB_MS);
    }

    private void onCreateInner() {
        if (Build.VERSION.SDK_INT >= 26) {
            NotificationManager nm = getSystemService(NotificationManager.class);
            if (nm == null) {
                throw new IllegalStateException("NotificationManager 取空");
            }
            nm.createNotificationChannel(new NotificationChannel(
                    CHANNEL_ID, "KFM-NA 保活", NotificationManager.IMPORTANCE_MIN));
        }
        Notification n = buildNotification("保活前台服务：退后台不冻结（BAR-029）");
        startForeground(NOTIF_ID, n);
        PowerManager pm = (PowerManager) getSystemService(POWER_SERVICE);
        mWakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "kfm_na:keepalive");
        mWakeLock.acquire();
    }

    /** 共同一条通知构造（首帧与心跳同源，不各写一份） */
    private Notification buildNotification(String text) {
        Notification.Builder b;
        if (Build.VERSION.SDK_INT >= 26) {
            b = new Notification.Builder(this, CHANNEL_ID);
        } else {
            b = new Notification.Builder(this);
        }
        return b.setContentTitle("KFM-NA 运行中")
                .setContentText(text)
                .setSmallIcon(android.R.drawable.stat_notify_more)
                .setOngoing(true)
                .build();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        return START_STICKY; // 被回收后系统尽力拉起
    }

    @Override
    public void onDestroy() {
        mStop = true;
        mH.removeCallbacksAndMessages(null);
        android.util.Log.i(TAG, "hb-stop at=" + System.currentTimeMillis() + " lastHb=" + mHb);
        if (mWakeLock != null && mWakeLock.isHeld()) {
            mWakeLock.release();
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
