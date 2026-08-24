#!/bin/bash
# na-shot.sh — 画面回传一键入口(2026-08-24,配套 src/screendump.rs)
#
#   bash scripts/na-shot.sh           拍一张,落 /tmp/na-shot.png
#   bash scripts/na-shot.sh --watch 3 每 3 秒拍一张(近同步直播,Ctrl-C 停)
#
# 链路:8024 闸门 touch 触发文件 → na 渲染循环下一帧倒 shot.rgb+shot.dim
# → scp 拉回 → PIL 转 PNG(XRGB 小端 = B,G,R,X 字节序)。
# 前提:na 装着带 screendump 的包且在前台活着(BAR-029 保活后后台也行);
# PIL 用 /root/.venvs/font/bin/python。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
PY=/root/.venvs/font/bin/python

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

shoot() {
    local stamp_before
    stamp_before=$(gate "stat -c%Y $NA_TMP/shot.dim 2>/dev/null || echo 0")
    gate "rm -f $NA_TMP/shot.rgb; touch $NA_TMP/shot-req" >/dev/null
    # 等 na 倒完(dim 时间戳变了且 rgb 文件出现)
    for _ in $(seq 1 20); do
        sleep 0.5
        local now
        now=$(gate "stat -c%Y $NA_TMP/shot.dim 2>/dev/null || echo 0")
        if [ "$now" != "$stamp_before" ] && gate "test -f $NA_TMP/shot.rgb"; then
            break
        fi
    done
    local dim
    dim=$(gate "cat $NA_TMP/shot.dim") || { echo "❌ 没等到 na 倒帧(包里有 screendump 吗?na 活着吗?)"; return 1; }
    scp -P 8024 -i "$NA_KEY" -o BatchMode=yes -o StrictHostKeyChecking=no \
        "localhost:$NA_TMP/shot.rgb" /tmp/na-shot.rgb >/dev/null
    "$PY" - "$dim" <<'EOF'
import sys
from PIL import Image
w, h = int(sys.argv[1]), int(sys.argv[2])
raw = open('/tmp/na-shot.rgb', 'rb').read()
assert len(raw) == w * h * 4, f"尺寸对不上: {len(raw)} != {w}*{h}*4"
img = Image.frombytes('RGBA', (w, h), raw, 'raw', 'BGRA')
img.convert('RGB').save('/tmp/na-shot.png')
EOF
    echo "✅ /tmp/na-shot.png($dim)"
}

if [ "${1:-}" = "--watch" ]; then
    interval=${2:-2}
    while true; do
        shoot || true
        sleep "$interval"
    done
else
    shoot
fi
