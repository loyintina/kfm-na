#!/bin/bash
# na-trace.sh — 行踪环随查(2026-08-26,配套 src/trace.rs)
#
#   bash scripts/na-trace.sh [行数]     拉全量(默认)或末 N 行
#
# trace ring = report 流的本地滚动副本(256 帽,心跳已滤):进程活着
# 随时查(trace-req → trace.txt);进程死了看 panic-trace.txt(panic
# 钩子自动落的末 64 行)。答的问题:「死前/刚才发生了什么」。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"

gate_touch trace-req   # BAR-233
ok=""
for _ in $(seq 1 30); do
    gate_result trace.txt /tmp/na-trace.txt >/dev/null 2>&1 && { ok=1; break; }
    sleep 0.3
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到 trace.txt——值守线程活着吗?" >&2
    exit 1
fi
if [ $# -eq 1 ]; then
    tail -$1 /tmp/na-trace.txt
else
    cat /tmp/na-trace.txt
fi
