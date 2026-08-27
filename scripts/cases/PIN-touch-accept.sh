#!/bin/bash
# PIN-touch-accept.sh — 考官:scroll 注入与真手势同入口(2026-08-27,
# 通道八通车 + c59b637 挂起态修复的判卷法固化)
#
# 判卷法(挂起态实证「首行分毫不差」的机器化):
#   注入 seq 1 200 造出远超一屏的历史(首验实拍教训:seq 1 50 在高
#   行数屏幕上全装进一屏,零 scrollback,scroll 无物可滚=假空转)
#   → 读屏记首行 A
#   → scroll 5(看历史)→ 首行必须变(≠A)
#   → scroll -5(回底)→ 首行必须**精确回到 A**。
# 滚多滚少、回不来、空转(c59b637 的病)三步内必现形。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-touch

first_line() {
    bash "$NA_ROOT/scripts/na-text.sh" 2>/dev/null | grep -m1 -v '^[[:space:]]*$'
}

bash "$NA_ROOT/scripts/na-type.sh" 'seq 1 200\r' >/dev/null \
    || fail PIN-touch "注入 seq 失败"
sleep 2.5
A=$(first_line) || true
[ -n "${A:-}" ] || fail PIN-touch "造完历史读屏为空——终端活着吗"

bash "$NA_ROOT/scripts/na-touch.sh" 'scroll 5' >/dev/null
sleep 1.5
B=$(first_line) || true
if [ "${B:-}" = "$A" ]; then
    fail PIN-touch "scroll 5 后首行没变(空转?)——A=[$A]"
fi

bash "$NA_ROOT/scripts/na-touch.sh" 'scroll -5' >/dev/null
sleep 1.5
C=$(first_line) || true
if [ "${C:-}" != "$A" ]; then
    fail PIN-touch "scroll -5 后首行没回位:A=[$A] C=[$C]"
fi
pass PIN-touch "scroll ±5 首行精确往返(A=[$A])"
