#!/usr/bin/env bash
# deploy-via-na.sh — 自持安装通道部署 APK（2026-09-26，用户拍板摆脱 Termux）
#
# 链路（全程 na 自持，零 Termux 依赖）：
#   服务器 → QUIC 反连桥（9022 → na sshd）→ scp 进 na 私有目录
#   {files}/incoming/ → am start VIEW content://dev.kfm.na.provider/apk/<名>
#   + --grant-read-uri-permission → KfmFileProvider 开只读流递给系统安装器
#   （exported=false + grantUriPermissions=true，无存储权限、不碰共享存储）
#
# 与 deploy-phone.sh 的分工：那条走 Termux（8022 + /sdcard + file://），
# 这条走 na 自己（QUIC 桥 + 私有目录 + FileProvider）。na 活着就能装。
# 最后一步「安装」按钮照旧用户在手机上点（普通 uid 无 INSTALL_PACKAGES）。
#
# 用法：bash scripts/deploy-via-na.sh [APK路径]（缺省 target/release/apk/kfm-na.apk）
set -euo pipefail
cd "$(dirname "$0")/.."

# shellcheck source=scripts/lib/na-ssh.sh
source scripts/lib/na-ssh.sh

APK="${1:-target/release/apk/kfm-na.apk}"
[ -f "$APK" ] || { echo "❌ $APK 不存在，先打包（package-apk.sh）" >&2; exit 66; }
NAME="$(basename "$APK")"
INCOMING=/data/data/dev.kfm.na/files/incoming

echo "=== [deploy-na 1/3] 送包进 na 私有目录（$NAME，$(du -h "$APK" | cut -f1)） ==="
na_ssh "mkdir -p $INCOMING"
# 大包装载经 QUIC 桥——先推 .new 再 mv 原子改名（防半读，同 hot/ 协议）
na_scp_push "$APK" "$INCOMING/$NAME.new"
na_ssh "mv $INCOMING/$NAME.new $INCOMING/$NAME && ls -la $INCOMING/$NAME"

echo "=== [deploy-na 2/3] 调起安装器（FileProvider 一次性授权） ==="
na_ssh "am start -a android.intent.action.VIEW \
    -d content://dev.kfm.na.provider/apk/$NAME \
    -t application/vnd.android.package-archive \
    --grant-read-uri-permission"

echo "=== [deploy-na 3/3] ✅ 安装器已调起：手机上点「安装」（$NAME） ==="
echo "    包留在 na 私有目录 $INCOMING/$NAME（安装器读完即可删）"
echo "    注意：本脚本不清 hot/ 热核——loader 优先 hot（dlopen 成功即用），"
echo "    hot 核比包内新 = 装完跑的还是热核；要验包内核先手动 rm hot/libkfm_na.so"
