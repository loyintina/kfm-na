#!/bin/bash
# PIN-pump-accept.sh — 考官:泵速率不得回潮 57k/s 空转(2026-08-27,
# 挂单①销案钉——WaitUntil 4ms 节拍案的回归闸,案无编号,行为钉形式)
#
# 判卷法:水位环最近两张快照的 pump_calls 差分 ÷ 间隔 = 速率。
# 健康基线 ~90/s(2026-08-27 热更后实测),病灶 57000/s——阈值 1000/s
# 取中间,只拦「回潮」不拦正常波动。环内不足两张(启动未满 1 分钟)
# = 数据不够,跳过不算挂。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-pump

hist=$(bash "$NA_ROOT/scripts/na-history.sh" 2>/dev/null) \
    || fail PIN-pump "水位环拉不到"
line=$(echo "$hist" | grep -c '^t=') || true
if [ "${line:-0}" -lt 2 ]; then
    echo "⏭ PIN-pump | 水位环不足两张快照(启动未满 1 分钟),跳过" >&2
    exit 77
fi
read -r t1 p1 t2 p2 < <(echo "$hist" | grep '^t=' | tail -2 | awk '
    { for (i=1; i<=NF; i++) {
        if ($i ~ /^t=/) { v=$i; sub(/^t=/, "", v); t = v }
        if ($i ~ /^pump=/) { v=$i; sub(/^pump=/, "", v); p = v }
      }
      printf "%s %s ", t, p }')
rate=$(( (p2 - p1) * 1000 / (t2 - t1) ))
if [ "$rate" -lt 1000 ]; then
    pass PIN-pump "泵速率 ${rate}/s < 1000/s(基线 ~90/s,病灶 57k/s)"
else
    fail PIN-pump "泵速率 ${rate}/s ≥ 1000/s——57k/s 空转病回潮?"
fi
