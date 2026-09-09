#!/bin/bash
# na-anim-cap.sh — 点播下一轮动画的渲染源采样(BAR-076,P3 真相通道)
#
#   bash scripts/na-anim-cap.sh    # 投 anim-cap-req 触发:下一轮动画带采样帧
#
# 点播制(2026-09-09 起):奇偶轮播时代每两轮动画就有一轮被 readPixels
# 压到 16fps(实测 61ms/帧)——仪器噪音成了体验税。不点播 = 零开销。
# 配套:采样帧走 [anim-strip] 报表通道,服务器 anim-strip-png.py 拼 PNG;
# 触发在动画开表时消费(摘文件,单次点播单次采样)。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
    -o StrictHostKeyChecking=no localhost "touch $NA_TMP/anim-cap-req"
echo "✅ 已点播:下一轮动画带渲染源采样([anim-strip] 报表通道回收)"
