#!/bin/bash
# serve-overlays.sh — L2 交接点文件服务(在手机真 Termux 里跑)
# 设计:docs/active/l2-overlay.md §3(2026-08-22 实拍修正:na 读不了共享
# 存储根,交接改走手机本机回环 HTTP)
#
# 用法: scripts/serve-overlays.sh     # 前台跑;挂后台自己加 nohup &
# 只绑 127.0.0.1——服务na 终端回环拉包,不对任何网卡开口
set -euo pipefail

dir="$HOME/w/kfm-na-overlays"
mkdir -p "$dir"
echo "serving $dir @ http://127.0.0.1:8027 (Ctrl-C 收摊)"
exec python3 -m http.server 8027 --bind 127.0.0.1 -d "$dir"
