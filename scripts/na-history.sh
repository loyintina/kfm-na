#!/bin/bash
# na-history.sh — stats 水位环随查(2026-08-27,配套 gate.rs history_tick)
#
#   bash scripts/na-history.sh     打印最近 24 分钟的快照序列(每 30s
#                                  一张,一行一张):帧耗/CPU/RSS/吞吐/
#                                  死亡数/活跃会话
#
# na-stats.sh 答「现在什么状态」,本脚本答「这一路怎么走的」——趋势类
# bug(越来越慢/内存爬坡)的判卷尺。awk 取列即可画曲线,例:
#   bash scripts/na-history.sh | awk -F'rss=|kb' '{print $2}'
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

gate "rm -f $NA_TMP/history.txt; touch $NA_TMP/history-req" >/dev/null
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/history.txt"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到应答——值守线程活着吗?" >&2
    exit 1
fi
gate "cat $NA_TMP/history.txt"
