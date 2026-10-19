#!/bin/bash
# check-hot-core-vc.sh — 推核前闸（MAIN0192 楼418 清和裁）
#
# 为什么需要：热核只换 .so、不换壳。核里的 JNI 面一改，旧 APK 调不到对应符号/布局
# ⇒ 一点终端调软键盘即进程死（11 机 2026-10-19 实证：核自报 vc1792312241，机上
# APK 是 1792229184）。所以「核内嵌 vc」与「机上 APK versionCode」必须一致，
# 不一致即拒推——机器核，不靠人记。
#
# 用法：
#   bash scripts/check/check-hot-core-vc.sh <libkfm_na.so 路径> <APK versionCode>
# 取 versionCode（设备侧，只读）：
#   dumpsys package dev.kfm.na | grep -m1 versionCode
#
# 判档（rc）：
#   0 PASS  核内嵌 vc 恰好一个且与 APK versionCode 相等
#   1 FAIL  两者不等（列出两值）
#   2 FAIL  核里没有 vc 戳（无戳＝无从核，拒推）
#   3 FAIL  核里出现多个 vc 戳（构建混乱，拒推）
#   4 用法错
set -u

so=${1:-}
apk_vc=${2:-}
if [ -z "$so" ] || [ -z "$apk_vc" ]; then
  echo "用法: check-hot-core-vc.sh <libkfm_na.so> <APK versionCode>" >&2
  exit 4
fi
if [ ! -f "$so" ]; then
  echo "FAIL：找不到核文件 $so" >&2
  exit 2
fi
case "$apk_vc" in
  ''|*[!0-9]*)
    echo "FAIL：APK versionCode 必须是纯数字，实得「$apk_vc」——取法：dumpsys package dev.kfm.na | grep -m1 versionCode" >&2
    exit 4
    ;;
esac

# 从二进制里抽 vc 戳（不依赖 strings/binutils）
cores=$(grep -aoE 'vc[0-9]{6,}' "$so" | sed 's/^vc//' | sort -u)

if [ -z "$cores" ]; then
  echo "FAIL：核里没有 vc 戳——无戳不推（无戳＝手机端无从核，配不配套只能等崩）"
  exit 2
fi
n=$(printf '%s\n' "$cores" | wc -l | tr -d ' ')
if [ "$n" -ne 1 ]; then
  echo "FAIL：核里出现 $n 个不同 vc 戳（构建混乱，拒推）：$(echo "$cores" | paste -sd, -)"
  exit 3
fi

if [ "$cores" = "$apk_vc" ]; then
  echo "PASS：核 vc＝$cores，APK versionCode＝$apk_vc（配套）"
  exit 0
fi
echo "FAIL：核 vc＝$cores ≠ APK versionCode＝$apk_vc——推上去就是「核比壳新」，一点即崩（楼414 实证）"
exit 1
