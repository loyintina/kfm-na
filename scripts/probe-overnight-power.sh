#!/bin/bash
# probe-overnight-power.sh — 过夜电耗画像采集(2026-08-28,电耗专题开局)
#
#   nohup bash scripts/probe-overnight-power.sh [分钟] [间隔秒] >/dev/null 2>&1 &
#
# 默认 540 分钟 × 每 300s 一拍 ≈ 108 行。双源对账:
#   电池侧(8022):percentage / current(µA,负=放电) / status / 温度
#   na 侧(8024):cpu_jiffies / rss_kb / uptime / fg / pump_calls / deaths
# 判读:相邻拍 cpu_jiffies 差分=na 的 CPU 速率;current 均值=整机放电
# 电流;GAP 行=8024 失联窗口(Doze 冻结?本身即数据)。结尾给 drain 摘要。
# 已知边界:充电状态整夜无效(status=CHARGING 电流读数无意义,照记)。
set -uo pipefail

MINUTES=${1:-540}
IV=${2:-300}
LOG=/root/kfm-na/overnight-power-$(date +%m%d).log
END=$(( $(date +%s) + MINUTES * 60 ))

echo "# 过夜电耗采集 $(date '+%F %T') 起,${MINUTES}min×${IV}s | ts | 电量% 电流µA 状态 温度 | uptime fg cpu_jiffies rss_kb pump deaths |" >> "$LOG"

battery() {
    ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=6 -o StrictHostKeyChecking=no localhost \
        'termux-battery-status 2>/dev/null' \
        | python3 -c "import json,sys
try:
    d=json.load(sys.stdin)
    print(d.get('percentage','?'), d.get('current','?'), d.get('status','?'), d.get('temperature','?'))
except Exception:
    print('? ? ? ?')"
}

nastats() {
    bash "$(dirname "$0")/na-stats.sh" 2>/dev/null | grep -E '^(uptime|foreground|cpu_jiffies|rss_kb|pump_calls|session_deaths)=' \
        | tr '\n' ' ' | sed 's/=$//'
}

while [ "$(date +%s)" -lt "$END" ]; do
    ts=$(date '+%F %T')
    b=$(battery)
    ns=$(nastats)
    if echo "$ns" | grep -q '^uptime='; then
        echo "$ts | $b | $ns" >> "$LOG"
    else
        echo "$ts | $b | GAP(8024 失联——冻结/挂起窗口,本身即数据)" >> "$LOG"
    fi
    sleep "$IV"
done

echo "=== 采集结束 $(date '+%F %T') ===" >> "$LOG"
echo "== drain 摘要 ==" >> "$LOG"
awk '/\| [0-9]+ \|/ {
    for (i=1;i<=NF;i++) {
        if ($i ~ /^cpu_jiffies=/) { split($i,a,"="); jif[n++]=a[2] }
    }
} END {
    if (n>1) printf "na CPU 速率均值: %.1f jiffies/拍(共 %d 拍)\n", (jif[n-1]-jif[0])/(n-1), n
    else print "有效拍不足"
}' "$LOG" >> "$LOG"
echo "完成,账本: $LOG"
