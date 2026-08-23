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
    private static final String CHANNEL_ID = "kfm_na_keepalive";
    private static final int NOTIF_ID = 1;
    private PowerManager.WakeLock mWakeLock;

    @Override
    public void onCreate() {
        super.onCreate();
        Notification.Builder b;
        if (Build.VERSION.SDK_INT >= 26) {
            NotificationManager nm = getSystemService(NotificationManager.class);
            nm.createNotificationChannel(new NotificationChannel(
                    CHANNEL_ID, "KFM-NA 保活", NotificationManager.IMPORTANCE_MIN));
            b = new Notification.Builder(this, CHANNEL_ID);
        } else {
            b = new Notification.Builder(this);
        }
        Notification n = b.setContentTitle("KFM-NA 运行中")
                .setContentText("保活前台服务：退后台不冻结（BAR-029）")
                .setSmallIcon(android.R.drawable.stat_notify_more)
                .setOngoing(true)
                .build();
        startForeground(NOTIF_ID, n);
        PowerManager pm = (PowerManager) getSystemService(POWER_SERVICE);
        mWakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "kfm_na:keepalive");
        mWakeLock.acquire();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        return START_STICKY; // 被回收后系统尽力拉起
    }

    @Override
    public void onDestroy() {
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
