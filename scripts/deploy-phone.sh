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

VERSION_CODE=$(grep -m1 '^VERSION_CODE=' scripts/package-apk.sh | cut -d= -f2)
NAME="kfm-na-$VERSION_CODE.apk"

PHONE_PICKUP=~/w/项目/kfm-na          # 用户固定取包点（2026-08-15 用户指定）：
                                      # 安装器没弹/找不到包时来这里拿

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
