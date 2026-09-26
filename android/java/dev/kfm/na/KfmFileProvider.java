package dev.kfm.na;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileNotFoundException;
import java.util.List;

/**
 * KfmFileProvider — 自持安装/递交通道（2026-09-26 用户拍板：摆脱
 * Termux 依赖，「Termux 能做到的我们理论上也能」）。
 *
 * 把 na 私有目录里的文件（APK 等）包成 content:// + 一次性读授权递给
 * 系统组件（PackageInstaller 等）——不申请存储权限、不碰共享存储。
 * androidx FileProvider 用不了（Java 皮纯 SDK 编译，无 androidx 依赖），
 * 手写极简版：
 *   只认 content://dev.kfm.na.provider/apk/<文件名>
 *   根锁死在 {files}/incoming/——路径穿越（../、多级段）在规范化层拒掉；
 *   exported=false + grantUriPermissions=true：只有持一次性授权者能读。
 * 调用面：scripts/deploy-via-na.sh（am start VIEW content:// + grant 标志）。
 */
public class KfmFileProvider extends ContentProvider {

    private File root() {
        return new File(getContext().getFilesDir(), "incoming");
    }

    /** 只收 /apk/<名字>；名字单层、不许带分隔符和 ..——穿越在此死掉 */
    private File resolve(Uri uri) throws FileNotFoundException {
        List<String> seg = uri.getPathSegments();
        if (seg.size() != 2 || !"apk".equals(seg.get(0))) {
            throw new FileNotFoundException("只认 /apk/<名字>: " + uri);
        }
        String name = seg.get(1);
        if (name.isEmpty() || name.contains("..")) {
            throw new FileNotFoundException("非法文件名: " + uri);
        }
        File f = new File(root(), name);
        if (!f.isFile()) {
            throw new FileNotFoundException("不存在: " + f);
        }
        return f;
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) {
            throw new FileNotFoundException("只读通道，收到: " + mode);
        }
        return ParcelFileDescriptor.open(resolve(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
                        String[] selectionArgs, String sortOrder) {
        // PackageInstaller 会查文件名/大小做确认页展示
        try {
            File f = resolve(uri);
            MatrixCursor c = new MatrixCursor(new String[]{
                    OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE});
            c.addRow(new Object[]{f.getName(), f.length()});
            return c;
        } catch (FileNotFoundException e) {
            return null;
        }
    }

    @Override
    public String getType(Uri uri) {
        return "application/vnd.android.package-archive";
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        return 0;
    }
}
