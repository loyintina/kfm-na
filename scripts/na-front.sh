#!/bin/bash
# na-front.sh — 拉 NA 到前台并确认就位（2026-09-11 用户拍板：agent 自拉
# 前台自测，测完 na-back.sh 退回 = 完成信号，用户不用守屏）
#
#   bash scripts/na-front.sh    # 拉前台，8 拍内确认 foreground=true
#
# 已知边界：屏幕熄灭时 vivo 后台活动拉起限制会挡住 am start（实踩两连
# 杀进程）——本脚本报红即「去亮屏」，别连环重试。
set -euo pipefail
cd "$(dirname "$0")/.."

ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=no localhost \
    "am start -n dev.kfm.na/.MainActivity" >/dev/null

for i in $(seq 1 8); do
    sleep 2
    if bash scripts/na-stats.sh 2>/dev/null | grep -q '^foreground=true'; then
        echo "✅ NA 前台就位（第 $i 拍）"
        exit 0
    fi
done
echo "❌ 拉前台失败：8 拍未就位——屏幕灭着？（vivo 后台拉起限制，亮屏后重试）" >&2
exit 1
