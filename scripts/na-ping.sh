#!/bin/bash
# na-ping.sh — loop 看门狗随查(2026-08-25,配套 gate.rs watch_loop)
#
#   bash scripts/na-ping.sh     问一次:alive beat_age=Nms / stall / 未起跳
#
# 重绘泵是忙轮询(about_to_wait 每圈盖戳),龄期 >3000ms = 循环卡死/冬眠。
# 被动档案在闸门目录 loop-stall.log(只在卡死/复活迁移时写);
# panic 档案在 panic.log(追加制,一行一案)。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"

gate_touch ping-req   # BAR-233：结果一次性取走，无需预清
ok=""
for _ in $(seq 1 30); do
    out=$(gate_result ping-res 2>/dev/null) && { ok=1; break; }
    sleep 0.3
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到应答——值守线程活着吗?" >&2
    exit 1
fi
echo "$out"
