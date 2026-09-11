#!/bin/bash
# na-text.sh — 视野纯文本回传(2026-08-24,配套 src/gate.rs text-req 通道)
#
#   bash scripts/na-text.sh        拉当前视野文本,直接打印
#   bash scripts/na-text.sh 文件   拉到指定路径
#
# 比截图便宜一百倍还能 grep;滚动中导出跟视野走(对齐「所见」)。
# v1 只有当前屏,不含 scrollback(要历史用 tmux)。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"
OUT=${1:-/tmp/na-screen.txt}

# 等 screen.txt「重新出现」(先清场,不存在时间戳 race)
gate "rm -f $NA_TMP/screen.txt; touch $NA_TMP/text-req" >/dev/null
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/screen.txt"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到 na 倒文本——na 活着吗(终端装上了吗)?" >&2
    exit 1
fi
gate_pull "$NA_TMP/screen.txt" "$OUT"
[ "$OUT" = /tmp/na-screen.txt ] && cat "$OUT" || echo "✅ $OUT"
