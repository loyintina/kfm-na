#!/bin/bash
# redroid-anim-watch.sh — 云安卓动画帧级监控（2026-09-16，BAR-098 后
# 用户拍板「先建完全观测再碰机制」：screencap 单帧拍不到动画过程，
# screenrecord 整段录虚拟屏 + imageio 拆帧 = 每帧像素真相）
#
#   bash scripts/redroid-anim-watch.sh page    # 齿轮→切组件池标签（Page 平移）
#   bash scripts/redroid-anim-watch.sh upper   # 齿轮→组件池→点下池行（Upper 平移）
#   bash scripts/redroid-anim-watch.sh custom 'tap 1155 78; sleep 1; tap 320 78'
#
# 产物：/tmp/redroid-anim/<case>/frame-%03d.png（全帧，按编码序）+
#       同目录 meta.txt（帧数/尺寸）。看帧用 ReadMediaFile。
# 配套逻辑真相：panc 遥测（na-trace.sh，按时间戳过滤——环是 256 帽
# 滚动副本，rm trace.txt 不清环，旧帧会混进来，2026-09-16 仪器病实锤）
set -euo pipefail
ADB=/root/kfm-na-toolchain/sdk/platform-tools/adb
PY=/root/.venvs/video/bin/python
DEV=${REDROID_SERIAL:-localhost:5555}
CASE=${1:?用法: page|upper|custom 'cmds'}
OUT=/tmp/redroid-anim/$CASE
REC=/sdcard/anim-watch.mp4

rm -rf "$OUT"; mkdir -p "$OUT"
$ADB -s $DEV shell "rm -f $REC"

# 后台起录（SIGINT 收尾才落完整 mp4）
# 2026-09-16 卡点实锤：子进程持有 tty 会让 adb shell 阻塞到 screenrecord
# 自然结束（默认 180s）——必须重定向输出脱离 tty，--time-limit 兜底
$ADB -s $DEV shell "screenrecord --time-limit 20 $REC >/dev/null 2>&1 & echo \$! > /sdcard/anim-watch.pid"
sleep 1.2 # 编码器热身（首帧建立参考帧）

run_cmds() { # 分号分隔的 'tap x y' / 'sleep 秒'
    local cmds="$1"
    IFS=';' read -ra steps <<< "$cmds"
    for st in "${steps[@]}"; do
        st="$(echo "$st" | xargs)"
        [[ -z "$st" ]] && continue
        if [[ $st == sleep* ]]; then
            sleep "${st#sleep }"
        else
            $ADB -s $DEV shell "input $st"
        fi
    done
}

case "$CASE" in
    page)  run_cmds 'tap 1155 78; sleep 1.6; tap 320 78; sleep 1.4' ;;
    upper) run_cmds 'tap 1155 78; sleep 1.6; tap 320 78; sleep 1.6; tap 450 1568; sleep 1.4' ;;
    custom) run_cmds "${2:?custom 需要指令串}" ;;
esac

PID=$($ADB -s $DEV shell "cat /sdcard/anim-watch.pid" | tr -d '\r')
$ADB -s $DEV shell "kill -2 $PID" 2>/dev/null || true
sleep 1.0 # 等 muxer 收尾
$ADB -s $DEV pull $REC "$OUT/anim.mp4" >/dev/null

$PY - "$OUT" << 'EOF'
import sys, imageio.v3 as iio
from PIL import Image
out = sys.argv[1]
frames = iio.imread(f"{out}/anim.mp4", index=None)
for i, fr in enumerate(frames):
    Image.fromarray(fr).save(f"{out}/frame-{i:03d}.png")
with open(f"{out}/meta.txt", "w") as f:
    f.write(f"frames={len(frames)} size={frames[0].shape if len(frames) else '-'}\n")
print(f"frames={len(frames)} → {out}/frame-*.png")
EOF
