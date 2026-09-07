package dev.kfm.na;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.hardware.display.DisplayManager;
import android.hardware.display.VirtualDisplay;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaFormat;
import android.media.MediaMuxer;
import android.media.projection.MediaProjection;
import android.media.projection.MediaProjectionManager;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.IBinder;
import android.view.Surface;
import java.io.File;
import java.io.FileWriter;
import java.nio.ByteBuffer;

/**
 * KfmRecService — 软件内实录（P2 显示真相，2026-09-08）。
 *
 * 链路：gate rec-req-ms 文件 → 原生 hook → MainActivity.startRecordingFromGate
 * → startForegroundService(本服务) → startForeground(mediaProjection 类型)
 * → 系统授权弹窗（用户点一次，安卓规矩绕不开）→ onActivityResult 回投
 * → MediaProjection + MediaCodec Surface 编码 + MediaMuxer → rec.mp4 落
 * files/usr/tmp（8024 scp 直达）。
 *
 * API 36 纪律：投影会话必须挂在 mediaProjection 类型的前台服务上
 * （Android 14+ 强制，SecurityException）；token 一次性——授权回调里
 * 立刻建 VirtualDisplay。状态写 rec-status（await/denied/timeout/
 * recording/done/error），服务器侧轮询判定。
 */
public class KfmRecService extends Service {
    private static final String CH = "kfm-rec";
    private static final int NOTIF_ID = 7002;

    private static KfmRecService sInstance;

    private MediaProjection mProjection;
    private VirtualDisplay mDisplay;
    private MediaCodec mCodec;
    private MediaMuxer mMuxer;
    private Surface mInputSurface;
    private HandlerThread mThread;
    private Handler mHandler;
    private volatile boolean mEosSent;
    private int mTrack = -1;

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        sInstance = this;
        startAsForeground();
        // Android 14+ 唯一合法顺序（2026-09-08 闪退案根因）：用户先授权，
        // 服务后 startForeground(mediaProjection)，再建投影——反序即
        // SecurityException 闪退（旧版把 FGS 放授权前，一触即崩）
        try {
            int resultCode = intent.getIntExtra("resultCode", -1);
            Intent data = intent.getParcelableExtra("data");
            MediaProjectionManager mpm = (MediaProjectionManager)
                    getSystemService(Context.MEDIA_PROJECTION_SERVICE);
            MediaProjection projection = mpm.getMediaProjection(resultCode, data);
            begin(projection, intent.getIntExtra("durationMs", 8000));
        } catch (Exception e) {
            status("error:" + e.getClass().getSimpleName());
            finishEncode();
        }
        return START_NOT_STICKY;
    }

    /** 授权成功后 begin：建编码器/投影/复用器，durationMs 后收尾 */
    public void begin(MediaProjection projection, int durationMs) {
        mProjection = projection;
        File out = new File(getFilesDir(), "usr/tmp/rec.mp4");
        out.getParentFile().mkdirs();
        try {
            android.util.DisplayMetrics dm = getResources().getDisplayMetrics();
            int w = Math.max(2, (dm.widthPixels / 2) * 2);
            int h = Math.max(2, (dm.heightPixels / 2) * 2);

            MediaFormat fmt = MediaFormat.createVideoFormat(
                    MediaFormat.MIMETYPE_VIDEO_AVC, w, h);
            fmt.setInteger(MediaFormat.KEY_COLOR_FORMAT,
                    MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface);
            fmt.setInteger(MediaFormat.KEY_BIT_RATE, 12_000_000);
            fmt.setInteger(MediaFormat.KEY_FRAME_RATE, 60);
            fmt.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1);
            mCodec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC);
            mCodec.configure(fmt, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE);
            mInputSurface = mCodec.createInputSurface();
            mMuxer = new MediaMuxer(out.getAbsolutePath(),
                    MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4);

            // 用户从系统投屏角标手动停 → 立刻收尾（token 一次性纪律）
            mProjection.registerCallback(new MediaProjection.Callback() {
                @Override
                public void onStop() {
                    requestStop();
                }
            }, mHandler);

            mDisplay = mProjection.createVirtualDisplay("kfm-rec", w, h, dm.densityDpi,
                    DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                    mInputSurface, null, mHandler);

            // 编码泵：FORMAT_CHANGED → 注册轨并起 muxer；样本照写；
            // EOS → 收尾（Surface 输入 = onInputBufferAvailable 永不回调）
            mCodec.setCallback(new MediaCodec.Callback() {
                @Override
                public void onInputBufferAvailable(MediaCodec codec, int index) {
                }

                @Override
                public void onOutputBufferAvailable(MediaCodec codec,
                        int index, MediaCodec.BufferInfo info) {
                    try {
                        ByteBuffer buf = codec.getOutputBuffer(index);
                        if (mMuxer != null && mTrack >= 0
                                && (info.flags & MediaCodec.BUFFER_FLAG_CODEC_CONFIG) == 0) {
                            mMuxer.writeSampleData(mTrack, buf, info);
                        }
                        codec.releaseOutputBuffer(index, false);
                        if ((info.flags & MediaCodec.BUFFER_FLAG_END_OF_STREAM) != 0) {
                            finishEncode();
                        }
                    } catch (Exception e) {
                        status("error:" + e.getClass().getSimpleName());
                        finishEncode();
                    }
                }

                @Override
                public void onOutputFormatChanged(MediaCodec codec, MediaFormat format) {
                    try {
                        mTrack = mMuxer.addTrack(format);
                        mMuxer.start();
                    } catch (Exception e) {
                        status("error:" + e.getClass().getSimpleName());
                        finishEncode();
                    }
                }

                @Override
                public void onError(MediaCodec codec, MediaCodec.CodecException e) {
                    status("error:" + e.getClass().getSimpleName());
                    finishEncode();
                }
            }, mHandler);
            mCodec.start();
            status("recording");
            mHandler.postDelayed(new Runnable() {
                @Override
                public void run() {
                    requestStop();
                }
            }, durationMs);
        } catch (Exception e) {
            status("error:" + e.getClass().getSimpleName());
            finishEncode();
        }
    }

    /** 收编码：EOS 进流，泵侧吐完即 finishEncode（时长到/手动停都走这） */
    private void requestStop() {
        try {
            if (mCodec != null && !mEosSent) {
                mEosSent = true;
                mCodec.signalEndOfInputStream();
            }
        } catch (Exception ignored) {
        }
    }

    /** 全量拆台（幂等——任何异常路径都收得干净） */
    private void finishEncode() {
        try {
            if (mCodec != null) {
                mCodec.stop();
                mCodec.release();
                mCodec = null;
            }
        } catch (Exception ignored) {
        }
        try {
            if (mMuxer != null) {
                mMuxer.stop();
                mMuxer.release();
                mMuxer = null;
            }
        } catch (Exception ignored) {
        }
        try {
            if (mDisplay != null) {
                mDisplay.release();
                mDisplay = null;
            }
        } catch (Exception ignored) {
        }
        try {
            if (mInputSurface != null) {
                mInputSurface.release();
                mInputSurface = null;
            }
        } catch (Exception ignored) {
        }
        try {
            if (mProjection != null) {
                mProjection.stop();
                mProjection = null;
            }
        } catch (Exception ignored) {
        }
        status("done");
        stopSelf();
    }

    private void startAsForeground() {
        NotificationManager nm = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (android.os.Build.VERSION.SDK_INT >= 26) {
            NotificationChannel ch = new NotificationChannel(CH, "KFM 实录",
                    NotificationManager.IMPORTANCE_LOW);
            nm.createNotificationChannel(ch);
        }
        Notification n = new Notification.Builder(this, CH)
                .setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle("KFM-NA 实录中")
                .build();
        if (android.os.Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIF_ID, n,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION);
        } else {
            startForeground(NOTIF_ID, n);
        }
        mThread = new HandlerThread("kfm-rec");
        mThread.start();
        mHandler = new Handler(mThread.getLooper());
    }

    private void status(String s) {
        try {
            File f = new File(getFilesDir(), "usr/tmp/rec-status");
            f.getParentFile().mkdirs();
            FileWriter w = new FileWriter(f, false);
            w.write(s);
            w.close();
        } catch (Exception ignored) {
        }
    }

    @Override
    public void onDestroy() {
        if (sInstance == this) {
            sInstance = null;
        }
        finishEncode();
        if (mThread != null) {
            mThread.quitSafely();
            mThread = null;
        }
        super.onDestroy();
    }
}
