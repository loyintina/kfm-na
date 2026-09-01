#!/usr/bin/env bash
# deploy-phone.sh — 一键送包到手机 Termux 并调起系统安装器（BAR-011 红线照旧：
# versionCode 由 package-apk.sh 管，本脚本只送已打好的包或先打包再送）
#
# 链路：服务器 → ssh 隧道 localhost:8022 → 手机 Termux → scp 到共享存储 →
#       am start 调起安装器（用户在手机上点「安装」完成最后一步）
#
# 为什么不静默装：Termux 普通 uid 无 INSTALL_PACKAGES 权限（SELinux +
# PackageManager 双重拦截），root 之前最后一下确认省不掉。
# 为什么放共享存储：system_server 读不到 Termux 私有目录
# （/data/data/com.termux/...），/storage/emulated/0 可读。
#
# 用法：
#   bash scripts/deploy-phone.sh           # 送当前 target/release/apk/kfm-na.apk
#   bash scripts/deploy-phone.sh --build   # 先跑 package-apk.sh 再送
set -euo pipefail
cd "$(dirname "$0")/.."

SSH_PORT=8022
SSH_HOST=localhost
PHONE_SHARED=/storage/emulated/0          # 手机共享存储根（termux-storage 已挂）
PHONE_TMP=/data/data/com.termux/files/home/downloads  # 私有目录暂存（留档，私有目录装不了）

if [ "${1:-}" = "--build" ]; then
    bash scripts/package-apk.sh
fi

APK=target/release/apk/kfm-na.apk
[ -f "$APK" ] || { echo "❌ $APK 不存在，先打包（或用 --build）"; exit 1; }

VERSION_CODE=$(cat build/version-code.current 2>/dev/null)
[ -n "$VERSION_CODE" ] || { echo "❌ build/version-code.current 不存在，先打包"; exit 1; }
NAME="kfm-na-$VERSION_CODE.apk"

PHONE_PICKUP=/data/data/com.termux/files/home/w/项目/kfm-na  # 用户固定取包点
                                      #（2026-08-15 用户指定）。必须写绝对路径：
                                      # ~/ 会在本地 shell 展开成 /root 再送到手机
                                      #（BAR-019），安装器没弹/找不到包时来这里拿

if [ -d /data/data/com.termux ]; then
    # 手机上本地跑（档位 2 自举）：包就在本机，直接拷共享存储调安装器
    echo "=== [deploy] 手机本地模式：$NAME ==="
    cp "$APK" "$PHONE_SHARED/$NAME"
    mkdir -p "$PHONE_PICKUP" && cp "$APK" "$PHONE_PICKUP/$NAME"
    am start -a android.intent.action.VIEW \
        -d "file://$PHONE_SHARED/$NAME" \
        -t application/vnd.android.package-archive
    echo "=== [deploy] ✅ 安装器已调起：点「安装」（$NAME）==="
    echo "    备用取包点：$PHONE_PICKUP/$NAME"
    exit 0
fi

SSH="ssh -p $SSH_PORT -o BatchMode=yes -o ConnectTimeout=8 $SSH_HOST"

echo "=== [deploy 1/3] 送包到手机（$NAME） ==="
scp -P $SSH_PORT -o BatchMode=yes "$APK" "$SSH_HOST:$PHONE_TMP/$NAME"

echo "=== [deploy 2/3] 拷进共享存储（安装器要读）+ 固定取包点 ==="
$SSH "cp $PHONE_TMP/$NAME $PHONE_SHARED/$NAME && \
    mkdir -p $PHONE_PICKUP && cp $PHONE_TMP/$NAME $PHONE_PICKUP/$NAME && \
    ls -la $PHONE_SHARED/$NAME"

echo "=== [deploy 3/3] 调起安装器 ==="
$SSH "am start -a android.intent.action.VIEW \
    -d file://$PHONE_SHARED/$NAME \
    -t application/vnd.android.package-archive"

echo "=== [deploy] ✅ 安装器已调起：手机上点「安装」（$NAME） ==="
echo "    备用取包点：$PHONE_PICKUP/$NAME"

# 2026-08-31 排障实锤：na-loader 优先 dlopen {files}/hot/libkfm_na.so，
# hot 里躺着旧热核 = 新 APK 白装（新功能全被盖住）。装包前清一次；
# 8024 不可达（na 没在跑）就给手动路径（docs/active/热更新.md §坑）。
echo "=== [deploy 收尾] 清 hot/ 旧热核（防盖住新包） ==="
if [ -f /root/.ssh/na_probe_key ] && ssh -p 8024 -i /root/.ssh/na_probe_key \
    -o BatchMode=yes -o ConnectTimeout=4 -o StrictHostKeyChecking=no \
    localhost 'rm -f /data/data/dev.kfm.na/files/hot/libkfm_na.so' 2>/dev/null; then
    echo "    ✅ hot 旧核已清，新包启动即新核"
else
    echo "    ⚠️ 8024 不可达（na 没在跑？）：启动新包后跑"
    echo "       bash scripts/na-push-so.sh 把同版新核推进 hot/ 盖住旧核，"
    echo "       或经 8024 手动 rm hot/libkfm_na.so 再重启"
fi

# 清旧包(2026-09-01):取包点只留最新 2 个——旧包误装=「已装更高版本」迷惑弹
if [ -d "$DEST_DIR" ]; then
    ls -t "$DEST_DIR"/kfm-na-*.apk 2>/dev/null | tail -n +3 | xargs -r rm -f
fi
