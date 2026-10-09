#!/bin/bash
# na-stats.sh — 运行时统计随查(2026-08-26,配套 gate.rs stats_answer)
#
#   bash scripts/na-stats.sh     打印快照:uptime/前台态/循环龄期/帧数/
#                                泵调用与字节/闸门动作计数/会话名单
#                                attached=活跃腿身份(BAR-238:附着 tmux
#                                会话名;裸 shell 腿=腿名 local/remote,
#                                na-type.sh 第一参数照它填)
#
# trace ring 答「发生了什么」,本快照答「现在什么状态」。key=value
# 一行一项,可直接 source 或 awk 取数。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"

gate_touch stats-req   # BAR-233
ok=""
for _ in $(seq 1 30); do
    out=$(gate_result stats-res 2>/dev/null) && { ok=1; break; }
    sleep 0.3
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到应答——值守线程活着吗?" >&2
    exit 1
fi
echo "$out"
