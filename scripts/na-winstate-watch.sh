#!/bin/bash
# na-winstate-watch.sh — BAR-145 发病现场自动捕获（2026-09-26 用户拍板）
#
# 背景：用户遇 BAR-145（解析页 tmux 行点击漂移一行）后只能开 IME 打字
# 报告，而开 IME 强制重排 = 破坏现场（观测者效应）。但病灶是粘性的
# ——发作后持续到 IME 召唤才复位，发作期内任意时刻采样都有效。
# 所以不需要用户报信：服务器侧每 45s 投 window-state-req 闸门，
# 读回 winTop/insetTop/dm 带时间戳落账，发病几何自动落网。
#
# 判据（健康基线实测于 vc1790395273，2026-09-26）：winTop=0 insetTop=120
# dm=1260x2680。偏离即发病几何（挖孔/状态栏避让瞬态 desync 嫌疑，
# motion-java.log 已实录一次 winTop=135）。
# STALE：快照 uptime 没变 = 新 dump 没落地（app 卡死/闸门断），单独标记。
#
# 用法：bash scripts/na-winstate-watch.sh   # 前台或后台任务跑
# 产物：logs/winstate-watch.log（每轮一行，gitignored 建议）

set -u
cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/na-ssh.sh
source scripts/lib/na-ssh.sh
LOG=logs/winstate-watch.log
mkdir -p logs
PREV_UP=""
while :; do
  ts=$(date '+%m-%d %H:%M:%S')
  na_ssh "touch /data/data/dev.kfm.na/files/usr/tmp/window-state-req" 2>/dev/null
  sleep 3
  line=$(na_ssh "cat /data/data/dev.kfm.na/files/usr/tmp/window-state 2>/dev/null" 2>/dev/null | tail -1)
  if [ -z "$line" ]; then
    echo "$ts UNREACHABLE" >> "$LOG"
  else
    up=${line%% *}
    if [ "$up" = "$PREV_UP" ]; then
      echo "$ts STALE $line" >> "$LOG"
    else
      case "$line" in
        *"winTop=0 insetTop=120"*) mark=OK ;;
        *)                       mark="⚠SICK" ;;
      esac
      echo "$ts $mark $line" >> "$LOG"
      PREV_UP=$up
    fi
  fi
  sleep 45
done
