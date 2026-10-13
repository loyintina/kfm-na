package dev.kfm.na;

import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.GestureDescription;
import android.graphics.Path;
import android.os.Bundle;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;

import java.util.List;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/**
 * BAR-246 无障碍服务薄壳（MAIN0149 用户拍板「让 na 拥有自己的无障碍权限」）。
 *
 * 形态铁律（承影 NA0175 楼2）：本类名一次定死、永不再改——无障碍授权按
 * 包名＋服务类名记账，类名不变 = 热更（只换 .so）碰不到授权面。本类只持
 * 服务实例与原子原语，全部命令解析/编排/日志逻辑在 native 侧（src/a11y.rs）。
 *
 * v1 不监听事件流（onAccessibilityEvent 空实现）——要「手」不要「眼」，
 * 系统负担最小。
 */
public class KfmA11yService extends AccessibilityService {

    private static KfmA11yService sInst;
    private static final Object LOCK = new Object();

    static {
        // na_loader 是加载壳（manifest lib_name）；幂等，进程内已载则零成本
        try {
            System.loadLibrary("na_loader");
        } catch (Throwable ignore) {
        }
    }

    /** 服务绑定/解绑通报 native（防护同 KfmImeView：UnsatisfiedLinkError 吞掉不炸服务） */
    private static native void nativeA11yUp(boolean up);

    @Override
    protected void onServiceConnected() {
        sInst = this;
        try {
            nativeA11yUp(true);
        } catch (Throwable ignore) {
        }
    }

    @Override
    public boolean onUnbind(android.content.Intent intent) {
        sInst = null;
        try {
            nativeA11yUp(false);
        } catch (Throwable ignore) {
        }
        return super.onUnbind(intent);
    }

    @Override
    public void onDestroy() {
        sInst = null;
        super.onDestroy();
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent event) {
        // v1 不消费事件流（空实现——见类头注释）
    }

    @Override
    public void onInterrupt() {
    }

    // ---------- native 委托的原子原语（全部 static，native 侧按名调） ----------

    /** 状态一行：up=服务实例在/root=根节点可拿 */
    public static String a11yStatus() {
        synchronized (LOCK) {
            if (sInst == null) {
                return "up=0 root=0";
            }
            AccessibilityNodeInfo root = null;
            try {
                root = sInst.getRootInActiveWindow();
                return "up=1 root=" + (root != null ? 1 : 0);
            } finally {
                if (root != null) {
                    root.recycle();
                }
            }
        }
    }

    /** 窗口节点树文本（缩进树；截断 256KB／2000 节点双帽，防巨型树撑爆） */
    public static String a11yDumpTree() {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            AccessibilityNodeInfo root = sInst.getRootInActiveWindow();
            if (root == null) {
                return "ERR no-root（无活动窗口或窗口拒读）";
            }
            StringBuilder sb = new StringBuilder();
            int[] count = {0};
            dumpNode(root, 0, sb, count);
            String s = sb.toString();
            if (s.length() > 256 * 1024) {
                s = s.substring(0, 256 * 1024) + "\n…[截断 256KB]";
            }
            return s;
        }
    }

    private static void dumpNode(AccessibilityNodeInfo n, int depth, StringBuilder sb, int[] count) {
        if (n == null || count[0] >= 2000) {
            return;
        }
        count[0]++;
        for (int i = 0; i < depth; i++) {
            sb.append("  ");
        }
        String cls = n.getClassName() == null ? "?" : n.getClassName().toString();
        int dot = cls.lastIndexOf('.');
        sb.append(dot >= 0 ? cls.substring(dot + 1) : cls);
        CharSequence text = n.getText();
        CharSequence desc = n.getContentDescription();
        String id = n.getViewIdResourceName();
        if (text != null) {
            sb.append(" text='").append(text.toString().replace('\n', ' ')).append('\'');
        }
        if (desc != null) {
            sb.append(" desc='").append(desc.toString().replace('\n', ' ')).append('\'');
        }
        if (id != null) {
            sb.append(" id='").append(id).append('\'');
        }
        android.graphics.Rect r = new android.graphics.Rect();
        n.getBoundsInScreen(r);
        sb.append(" [").append(r.left).append(',').append(r.top).append(',')
                .append(r.right).append(',').append(r.bottom).append(']');
        if (n.isClickable()) {
            sb.append(" c");
        }
        if (n.isScrollable()) {
            sb.append(" s");
        }
        if (n.isEditable()) {
            sb.append(" e");
        }
        sb.append('\n');
        int nc = n.getChildCount();
        for (int i = 0; i < nc; i++) {
            AccessibilityNodeInfo c = n.getChild(i);
            if (c != null) {
                dumpNode(c, depth + 1, sb, count);
                c.recycle();
            }
        }
    }

    /** 按 text/contentDescription 命中节点，点其最近可点祖先 */
    public static String a11yClickText(String target) {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            AccessibilityNodeInfo root = sInst.getRootInActiveWindow();
            if (root == null) {
                return "ERR no-root";
            }
            List<AccessibilityNodeInfo> hits = root.findAccessibilityNodeInfosByText(target);
            root.recycle();
            if (hits == null || hits.isEmpty()) {
                return "ERR not-found: " + target;
            }
            for (AccessibilityNodeInfo hit : hits) {
                AccessibilityNodeInfo clickable = clickableSelfOrAncestor(hit);
                if (clickable != null) {
                    boolean ok = clickable.performAction(AccessibilityNodeInfo.ACTION_CLICK);
                    CharSequence t = hit.getText();
                    String what = t != null ? t.toString() : target;
                    if (clickable != hit) {
                        clickable.recycle();
                    }
                    if (ok) {
                        recycleAll(hits);
                        return "ok clicked '" + what + "'";
                    }
                }
            }
            recycleAll(hits);
            return "ERR no-clickable-ancestor: " + target;
        }
    }

    /** 按完整资源 id（包名:id/名）命中节点，点其最近可点祖先 */
    public static String a11yClickId(String viewId) {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            AccessibilityNodeInfo root = sInst.getRootInActiveWindow();
            if (root == null) {
                return "ERR no-root";
            }
            List<AccessibilityNodeInfo> hits = root.findAccessibilityNodeInfosByViewId(viewId);
            root.recycle();
            if (hits == null || hits.isEmpty()) {
                return "ERR not-found: " + viewId;
            }
            for (AccessibilityNodeInfo hit : hits) {
                AccessibilityNodeInfo clickable = clickableSelfOrAncestor(hit);
                if (clickable != null) {
                    boolean ok = clickable.performAction(AccessibilityNodeInfo.ACTION_CLICK);
                    if (clickable != hit) {
                        clickable.recycle();
                    }
                    if (ok) {
                        recycleAll(hits);
                        return "ok clicked id=" + viewId;
                    }
                }
            }
            recycleAll(hits);
            return "ERR no-clickable-ancestor: " + viewId;
        }
    }

    /** 坐标点（dispatchGesture；2s 等结果回执） */
    public static String a11yClickPoint(final int x, final int y) {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            Path path = new Path();
            path.moveTo(x, y);
            GestureDescription gesture = new GestureDescription.Builder()
                    .addStroke(new GestureDescription.StrokeDescription(path, 0, 50))
                    .build();
            final CountDownLatch latch = new CountDownLatch(1);
            final AtomicReference<Boolean> done = new AtomicReference<>(false);
            boolean dispatched = sInst.dispatchGesture(gesture, new GestureResultCallback() {
                @Override
                public void onCompleted(GestureDescription g) {
                    done.set(true);
                    latch.countDown();
                }

                @Override
                public void onCancelled(GestureDescription g) {
                    latch.countDown();
                }
            }, null);
            if (!dispatched) {
                return "ERR gesture-rejected";
            }
            try {
                latch.await(2, TimeUnit.SECONDS);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
            return done.get() ? "ok point " + x + "," + y : "ERR gesture-timeout/cancelled";
        }
    }

    /** 第一个可滚节点前滚/回滚 */
    public static String a11yScroll(boolean forward) {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            AccessibilityNodeInfo root = sInst.getRootInActiveWindow();
            if (root == null) {
                return "ERR no-root";
            }
            AccessibilityNodeInfo sc = findFirstScrollable(root);
            root.recycle();
            if (sc == null) {
                return "ERR no-scrollable";
            }
            boolean ok = sc.performAction(forward
                    ? AccessibilityNodeInfo.ACTION_SCROLL_FORWARD
                    : AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD);
            sc.recycle();
            return ok ? "ok scroll " + (forward ? "fwd" : "back") : "ERR scroll-refused";
        }
    }

    /** 焦点（或首个可编辑）节点 ACTION_SET_TEXT */
    public static String a11ySetText(String text) {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            AccessibilityNodeInfo root = sInst.getRootInActiveWindow();
            if (root == null) {
                return "ERR no-root";
            }
            AccessibilityNodeInfo target = sInst.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
            if (target == null || !target.isEditable()) {
                if (target != null) {
                    target.recycle();
                }
                target = findFirstEditable(root);
            }
            root.recycle();
            if (target == null) {
                return "ERR no-editable";
            }
            Bundle args = new Bundle();
            args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text);
            boolean ok = target.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args);
            target.recycle();
            return ok ? "ok set-text" : "ERR set-text-refused";
        }
    }

    /** 全局动作（sel 序号与 Rust GlobalAction::sel 同表：0 back/1 home/2 recents/3 notifications/4 quick-settings） */
    public static String a11yGlobal(int sel) {
        synchronized (LOCK) {
            if (sInst == null) {
                return "ERR no-service";
            }
            int action;
            String name;
            switch (sel) {
                case 0:
                    action = GLOBAL_ACTION_BACK;
                    name = "back";
                    break;
                case 1:
                    action = GLOBAL_ACTION_HOME;
                    name = "home";
                    break;
                case 2:
                    action = GLOBAL_ACTION_RECENTS;
                    name = "recents";
                    break;
                case 3:
                    action = GLOBAL_ACTION_NOTIFICATIONS;
                    name = "notifications";
                    break;
                case 4:
                    action = GLOBAL_ACTION_QUICK_SETTINGS;
                    name = "quick-settings";
                    break;
                default:
                    return "ERR bad-global-sel: " + sel;
            }
            boolean ok = sInst.performGlobalAction(action);
            return ok ? "ok global " + name : "ERR global-refused " + name;
        }
    }

    // ---------- 内部小件 ----------

    /** 自身可点则取自身，否则逐级向上找最近可点祖先（列表条目文本格常不可点） */
    private static AccessibilityNodeInfo clickableSelfOrAncestor(AccessibilityNodeInfo n) {
        AccessibilityNodeInfo cur = n;
        while (cur != null) {
            if (cur.isClickable()) {
                return cur;
            }
            AccessibilityNodeInfo parent = cur.getParent();
            if (cur != n) {
                cur.recycle();
            }
            cur = parent;
        }
        return null;
    }

    private static AccessibilityNodeInfo findFirstScrollable(AccessibilityNodeInfo n) {
        if (n == null) {
            return null;
        }
        if (n.isScrollable()) {
            return n;
        }
        int nc = n.getChildCount();
        for (int i = 0; i < nc; i++) {
            AccessibilityNodeInfo c = n.getChild(i);
            if (c != null) {
                AccessibilityNodeInfo r = findFirstScrollable(c);
                if (r != null) {
                    if (c != r) {
                        c.recycle();
                    }
                    return r;
                }
                c.recycle();
            }
        }
        return null;
    }

    private static AccessibilityNodeInfo findFirstEditable(AccessibilityNodeInfo n) {
        if (n == null) {
            return null;
        }
        if (n.isEditable()) {
            return n;
        }
        int nc = n.getChildCount();
        for (int i = 0; i < nc; i++) {
            AccessibilityNodeInfo c = n.getChild(i);
            if (c != null) {
                AccessibilityNodeInfo r = findFirstEditable(c);
                if (r != null) {
                    if (c != r) {
                        c.recycle();
                    }
                    return r;
                }
                c.recycle();
            }
        }
        return null;
    }

    private static void recycleAll(List<AccessibilityNodeInfo> nodes) {
        for (AccessibilityNodeInfo n : nodes) {
            if (n != null) {
                n.recycle();
            }
        }
    }
}
