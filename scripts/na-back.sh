#!/bin/bash
# na-back.sh — 送 NA 回后台并确认（2026-09-11 用户拍板的工作流收尾：
# 用户看到 NA 退下 = agent 测试做完，可以做别的事）
#
#   bash scripts/na-back.sh     # 回后台，8 拍内确认 foreground=false
#
# 实踩教训：input keyevent KEYCODE_HOME 会被 NA 当终端按键吃掉（终端
# 的 HOME/END 语义），必须走 launcher intent 回桌面。
set -euo pipefail
cd "$(dirname "$0")/.."

source scripts/lib/gate-lib.sh
if [[ $NA_TRANSPORT == adb ]]; then
    "$NA_ADB" -s "$NA_ADB_SERIAL" shell \
        "am start -a android.intent.action.MAIN -c android.intent.category.HOME" >/dev/null
else
    ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=no localhost \
        "am start -a android.intent.action.MAIN -c android.intent.category.HOME" >/dev/null
fi

for i in $(seq 1 8); do
    sleep 2
    if bash scripts/na-stats.sh 2>/dev/null | grep -q '^foreground=false'; then
        echo "✅ NA 已回后台（第 $i 拍）——测试完毕信号"
        exit 0
    fi
done
echo "❌ 回后台失败：8 拍未退（可能已被系统回收——查 pidof）" >&2
exit 1
