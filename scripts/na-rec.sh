#!/bin/bash
# na-rec.sh — 软件内实录一键入口（P2 显示真相，2026-09-08，配套
# KfmRecService.java + gate 通道十二）
#
#   bash scripts/na-rec.sh [毫秒,默认 8000]
#
# 链路:闸门(na-ssh.sh:9022 首选/8024 备援)写 rec-req-ms → 值守线程 → JNI → MainActivity →
# 系统授权弹窗(用户点「立即开始」,一次性) → MediaProjection 编码 →
# rec.mp4 落 files/usr/tmp → scp 拉回 /tmp/na-rec.mp4。
# 状态机(rec-status):await→recording→done;denied/timeout=弹窗没点;
# error=编码异常(看 message)。
set -euo pipefail

NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
DUR=${1:-8000}

# shellcheck source=scripts/lib/na-ssh.sh
source "$(dirname "$0")/lib/na-ssh.sh"

gate() { na_ssh "$1"; }

gate "echo $DUR > $NA_TMP/rec-req-ms; rm -f $NA_TMP/rec-status; rm -f $NA_TMP/rec.mp4"
echo "已触发(${DUR}ms)。请在手机授权弹窗点「立即开始」…"

for _ in $(seq 1 120); do
    sleep 0.5
    st=$(gate "cat $NA_TMP/rec-status 2>/dev/null" || true)
    case "$st" in
        recording)
            echo "…录制中"
            ;;
        done)
            na_scp_pull "$NA_TMP/rec.mp4" /tmp/na-rec.mp4 >/dev/null
            echo "✅ /tmp/na-rec.mp4"
            exit 0
            ;;
        denied|timeout)
            echo "❌ 授权弹窗没点（$st）——重跑本脚本会再弹一次"
            exit 1
            ;;
        error*)
            echo "❌ 编码异常: $st"
            exit 1
            ;;
    esac
done
echo "❌ 60 秒未完成——app 在前台吗?弹窗出来了吗?"
exit 1
