#!/bin/bash
# na-touch.sh — 触摸注入(2026-08-27,配套 src/gate.rs touch-in 通道八)
#
#   bash scripts/na-touch.sh 'scroll 3'              # 看历史 3 行
#   bash scripts/na-touch.sh 'scroll -3'             # 回最新 3 行
#   bash scripts/na-touch.sh 'tap 530 400'           # 点按(终端区=唤键盘)
#   bash scripts/na-touch.sh 'down 530 800' 'sleep 600' 'up 530 800'  # 长按
#
# 每个参数 = 一行指令,按序执行。语法(src/gate.rs parse_touch_line 钉死,
# 考题 tests/touch_spec.rs):
#   tap x y           点按(不过阈值 → 唤键盘/keybar 命中路径)
#   down/move/up x y [id]   裸事件序列(默认指 id=90;第二指显式给 id)
#   scroll [+-]n      滚屏语法糖:n>0 看历史 = 手指下扫(scroll.rs 契约)
#   sleep ms          节拍等待(封顶 10s;长按选择等时序手势用)
# 判卷配套:stats 的 touches 字段计数;na-text.sh/na-shot.sh 看结果。
# 协议同 na-type:先写 .new 再 mv(原子防半读),值守线程 300ms 内消费。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

if [ $# -lt 1 ]; then
    echo "用法: bash scripts/na-touch.sh 'scroll 3' ['sleep 600' ...](每参数一行指令)" >&2
    exit 64
fi

script="$(printf '%s\n' "$@")"
printf '%s\n' "$script" | ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
    -o StrictHostKeyChecking=no localhost \
    "cat > $NA_TMP/touch-in.new && mv $NA_TMP/touch-in.new $NA_TMP/touch-in"
echo "✅ 已注入 $# 条触摸指令(300ms 内落地;na-text.sh/na-shot.sh 核对)"
