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

source "$(dirname "$0")/lib/gate-lib.sh"
PY=/root/.venvs/font/bin/python

# redroid 云安卓平台差异（2026-09-11 实测，state.md redroid 条）：
# shot-gl(GPU 回读)出来 180° 翻转，shot.rgb(CPU 重画)正常——
# adb 传输下直接走 CPU 路，不碰 GL 回读
PREFER_CPU=0
[[ $NA_TRANSPORT == adb ]] && PREFER_CPU=1

shoot() {
    # 先清场再触发:等待信号 = 「重新出现」（不存在秒级时间戳 race）。
    # 双触发零竞态（2026-09-07 软件内截屏）：shot-gles-req = 真·GLES 合成
    # 帧（前台画帧时消费，观测真相）；静态屏无帧可消费 → 回退 shot.rgb
    # （值守 CPU 重画，画面没动过内容等价，后台也活）
    if [[ $PREFER_CPU == 1 ]]; then
        gate "rm -f $NA_TMP/shot.rgb $NA_TMP/shot.dim; touch $NA_TMP/shot-req" >/dev/null
    else
        gate "rm -f $NA_TMP/shot.rgb $NA_TMP/shot.dim $NA_TMP/shot-gl.rgb $NA_TMP/shot-gl.dim; touch $NA_TMP/shot-req $NA_TMP/shot-gles-req" >/dev/null
    fi
    local which=rgb ok=""
    if [[ $PREFER_CPU == 0 ]]; then
        for _ in $(seq 1 16); do
            sleep 0.5
            if gate "test -f $NA_TMP/shot-gl.rgb -a -f $NA_TMP/shot-gl.dim"; then
                which=gl; ok=1; break
            fi
        done
    fi
    if [ -z "$ok" ]; then
        for _ in $(seq 1 30); do
            sleep 0.5
            if gate "test -f $NA_TMP/shot.rgb -a -f $NA_TMP/shot.dim"; then
                ok=1; break
            fi
        done
    fi
    if [ -z "$ok" ]; then
        echo "❌ 23 秒内没等到 na 倒帧 —— 触发器没被消费"
        gate "test -f $NA_TMP/shot-req" >/dev/null \
            && echo "   触发文件还在:na 没有在画帧。应用在前台吗?把它切到前台再拍。"
        return 1
    fi
    local dim rgbpath dimpath
    if [ "$which" = gl ]; then
        rgbpath=$NA_TMP/shot-gl.rgb; dimpath=$NA_TMP/shot-gl.dim
    else
        rgbpath=$NA_TMP/shot.rgb; dimpath=$NA_TMP/shot.dim
    fi
    dim=$(gate "cat $dimpath")
    gate_pull "$rgbpath" /tmp/na-shot.rgb
    "$PY" - $dim <<EOF
import sys
from PIL import Image
w, h = int(sys.argv[1]), int(sys.argv[2])
raw = open('/tmp/na-shot.rgb', 'rb').read()
assert len(raw) == w * h * 4, f"尺寸对不上: {len(raw)} != {w}*{h}*4"
img = Image.frombytes('RGBA', (w, h), raw, 'raw', 'BGRA')
img.convert('RGB').save('/tmp/na-shot.png')
print('来源: shot-$which (gl=GPU 合成帧 / rgb=CPU 重画)')
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
