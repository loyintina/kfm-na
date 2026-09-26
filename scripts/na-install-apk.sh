#!/usr/bin/env bash
# na-install-apk.sh — na 自更新原语（BAR-162，2026-09-26）：投闸门让手机弹安装器。
#
#   bash scripts/na-install-apk.sh [APK路径]     # 缺省 target/release/apk/kfm-na.apk
#
# 链路（全程 na 自持，零 Termux）：
#   ① scp 进 na 私有目录 {files}/incoming/<名>（.new 再 mv，原子）
#   ② 投通道十五闸门 usr/tmp/install-apk-req（内容 = 上一步的路径）
#   ③ na 值守线程消费 → JNI 甩 MainActivity.installApkFromGate（UI 线程
#      ACTION_VIEW + grant 标志）→ 系统安装器；现行装机 APK 无该方法则走
#      引导腿（门线程直调 activity.startActivity）——两条腿判决都落
#      usr/tmp/install-status，本脚本读回打印。
#   ④ 用户在手机上点「安装」完成最后一步（普通 uid 无 INSTALL_PACKAGES）。
#
# 为什么不用 am start（本单缘起）：经 QUIC 反连桥在 na 沙箱里跑 am start
# （termux-am 壳，na uid）被 vivo 按进程态判 BAL 静默吞——连浏览器 VIEW 都
# 不弹、exit=0 无输出，而 pm list packages 照通（IPC 活）= 不是壳坏，是桥
# spawn 的 app_process 不是可见 Activity 的宿主进程；给「安装未知应用」全权限
# 无用（BAL 判进程态不判权限）。安装意图必须由 MainActivity 所在前台进程发起。
#
# 定位：这是 **na 自更新原语**——不是给某个包开的窄门，往后 agent 给自己/
# 工具链推包都走这条道（deploy-via-na.sh 同源同法）。
set -euo pipefail
cd "$(dirname "$0")/.."

# shellcheck source=scripts/lib/na-ssh.sh
source scripts/lib/na-ssh.sh

APK="${1:-target/release/apk/kfm-na.apk}"
[ -f "$APK" ] || { echo "❌ $APK 不存在，先打包（package-apk.sh）" >&2; exit 66; }
NAME="$(basename "$APK")"
case "$NAME" in
    *.apk) ;;
    *) echo "❌ 只递 .apk（收到 $NAME）" >&2; exit 64 ;;
esac
NA_FILES=/data/data/dev.kfm.na/files
INCOMING=$NA_FILES/incoming
TMP=$NA_FILES/usr/tmp

echo "=== [install 1/3] 送包进 na 私有目录（$NAME，$(du -h "$APK" | cut -f1)） ==="
na_ssh "mkdir -p $INCOMING"
# 大包过 QUIC 桥：先 .new 再 mv（半截包递给安装器 = 装出个坏包）
na_scp_push "$APK" "$INCOMING/$NAME.new"
na_ssh "mv $INCOMING/$NAME.new $INCOMING/$NAME && ls -la $INCOMING/$NAME"

echo "=== [install 2/3] 投闸门（通道十五 install-apk-req） ==="
na_ssh "mkdir -p $TMP && printf '%s\n' '$INCOMING/$NAME' > $TMP/install-apk-req.new && mv $TMP/install-apk-req.new $TMP/install-apk-req"

echo "=== [install 3/3] 读回判决（usr/tmp/install-status，等 3s） ==="
sleep 3
na_ssh "cat $TMP/install-status 2>/dev/null" || true
echo
echo "✅ 闸门已投。手机上点「安装」完成最后一步；没弹 = 看上面 install-status 那一行"
echo "   （ok leg=java = UI 线程正道；ok leg=rust-direct = 引导腿；NO_HANDLER/ERR = 当场钉层）"
