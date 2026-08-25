#!/bin/bash
# na-ping.sh — loop 看门狗随查(2026-08-25,配套 gate.rs watch_loop)
#
#   bash scripts/na-ping.sh     问一次:alive beat_age=Nms / stall / 未起跳
#
# 重绘泵是忙轮询(about_to_wait 每圈盖戳),龄期 >3000ms = 循环卡死/冬眠。
# 被动档案在闸门目录 loop-stall.log(只在卡死/复活迁移时写);
# panic 档案在 panic.log(追加制,一行一案)。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

gate "rm -f $NA_TMP/ping-res; touch $NA_TMP/ping-req" >/dev/null
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/ping-res"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到应答——值守线程活着吗?" >&2
    exit 1
fi
gate "cat $NA_TMP/ping-res"
