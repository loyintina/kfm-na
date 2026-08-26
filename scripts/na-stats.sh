#!/bin/bash
# na-stats.sh — 运行时统计随查(2026-08-26,配套 gate.rs stats_answer)
#
#   bash scripts/na-stats.sh     打印快照:uptime/前台态/循环龄期/帧数/
#                                泵调用与字节/闸门动作计数/会话名单
#
# trace ring 答「发生了什么」,本快照答「现在什么状态」。key=value
# 一行一项,可直接 source 或 awk 取数。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

gate "rm -f $NA_TMP/stats-res; touch $NA_TMP/stats-req" >/dev/null
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/stats-res"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到应答——值守线程活着吗?" >&2
    exit 1
fi
gate "cat $NA_TMP/stats-res"
