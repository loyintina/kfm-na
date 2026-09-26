#!/usr/bin/env bash
# deploy-via-na.sh — 自持安装通道部署 APK（2026-09-26，用户拍板摆脱 Termux）
#
# 链路（全程 na 自持，零 Termux 依赖）：
#   服务器 → QUIC 反连桥（9022 → na sshd）→ scp 进 na 私有目录
#   {files}/incoming/ → 投闸门 usr/tmp/install-apk-req → na 值守线程 JNI 甩
#   MainActivity.installApkFromGate → UI 线程 ACTION_VIEW
#   content://dev.kfm.na.provider/apk/<名> + FLAG_GRANT_READ_URI_PERMISSION
#   → KfmFileProvider 开只读流递给系统安装器（exported=false +
#   grantUriPermissions=true，无存储权限、不碰共享存储）。
#   2026-09-26 定罪改道：原先第二步走桥 shell 的 am start（termux-am 壳，na
#   uid），被 vivo 按进程态判 BAL **静默吞**（连浏览器 VIEW 都不弹、exit=0
#   无输出，而 pm list packages 照通 = IPC 活、壳没坏；桥 spawn 的
#   app_process 不是可见 Activity 的宿主进程，给「安装未知应用」全权限无用）。
#   安装意图只能由 MainActivity 所在**前台进程**发起——改投闸门（通道十五，
#   与 na-install-apk.sh 同源同法；那条是通用口，这条是 APK 打包全流程）。
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
TMP=/data/data/dev.kfm.na/files/usr/tmp

echo "=== [deploy-na 1/3] 送包进 na 私有目录（$NAME，$(du -h "$APK" | cut -f1)） ==="
na_ssh "mkdir -p $INCOMING"
# 大包装载经 QUIC 桥——先推 .new 再 mv 原子改名（防半读，同 hot/ 协议）
na_scp_push "$APK" "$INCOMING/$NAME.new"
na_ssh "mv $INCOMING/$NAME.new $INCOMING/$NAME && ls -la $INCOMING/$NAME"

echo "=== [deploy-na 2/3] 投闸门调起安装器（通道十五，Activity 上下文发起） ==="
na_ssh "printf '%s\n' '$INCOMING/$NAME' > $TMP/install-apk-req.new && \
    mv $TMP/install-apk-req.new $TMP/install-apk-req"
sleep 3
echo "--- 判决（usr/tmp/install-status；ok leg=java=UI 线程正道 / ok leg=rust-direct=引导腿）---"
na_ssh "cat $TMP/install-status 2>/dev/null || echo '（无判决——闸门没被消费？核是不是装着我改过的版本）'"

echo "=== [deploy-na 3/3] ✅ 安装器已调起：手机上点「安装」（$NAME） ==="
echo "    包留在 na 私有目录 $INCOMING/$NAME（安装器读完即可删）"
echo "    注意：本脚本不清 hot/ 热核——loader 优先 hot（dlopen 成功即用），"
echo "    hot 核比包内新 = 装完跑的还是热核；要验包内核先手动 rm hot/libkfm_na.so"
