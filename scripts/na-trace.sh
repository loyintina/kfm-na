#!/bin/bash
# na-trace.sh — 行踪环随查(2026-08-26,配套 src/trace.rs)
#
#   bash scripts/na-trace.sh [行数]     拉全量(默认)或末 N 行
#
# trace ring = report 流的本地滚动副本(256 帽,心跳已滤):进程活着
# 随时查(trace-req → trace.txt);进程死了看 panic-trace.txt(panic
# 钩子自动落的末 64 行)。答的问题:「死前/刚才发生了什么」。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

gate "rm -f $NA_TMP/trace.txt; touch $NA_TMP/trace-req" >/dev/null
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/trace.txt"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到 trace.txt——值守线程活着吗?" >&2
    exit 1
fi
if [ $# -eq 1 ]; then
    gate "tail -$1 $NA_TMP/trace.txt"
else
    gate "cat $NA_TMP/trace.txt"
fi
