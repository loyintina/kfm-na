#!/bin/bash
# test-bar114-modal-cursor.sh — BAR-114 模态压顶守卫的考题（变异抽检，
# 2026-09-21 晨班会形态审计：BAR-114 段落级 → 代码守卫）
#
# 考三件事：
#   ①真源必须绿（守卫不报假警）；
#   ②变异「合成位闸摘除 cs.modal.is_none()」（BAR-114 原发病灶形态
#     ——光标层不问模态直接画）→ 必须红；
#   ③变异「闸语义反转 is_none → is_some」（模态在才画光标 = 同样压顶
#     且常态光标消失）→ 必须红。
# 变异在临时副本上做（守卫吃文件参数），不碰工作区。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

GUARD=scripts/check/check-modal-cursor.sh
SRC=src/android_app.rs
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# ①真源绿
bash "$GUARD" "$SRC" >/dev/null || { echo "❌ 真源被判红——守卫误伤"; exit 1; }

# ②变异：合成位闸整段摘除（BAR-114 病灶原样复刻）
python3 - "$SRC" "$TMP/mut-off.rs" <<'PY'
import sys
s = open(sys.argv[1], encoding="utf-8").read()
old = "if cs.modal.is_none()\n                        && !cs.rows.is_empty()"
new = "if !cs.rows.is_empty()"
assert old in s, "变异靶串不在——源码形态变了，考题要跟着修"
open(sys.argv[2], "w", encoding="utf-8").write(s.replace(old, new, 1))
PY
if bash "$GUARD" "$TMP/mut-off.rs" >/dev/null 2>&1; then
    echo "❌ 变异②（modal 闸整段摘除）没被咬住——守卫是摆设"
    exit 1
fi

# ③变异：闸语义反转 is_none → is_some
python3 - "$SRC" "$TMP/mut-flip.rs" <<'PY'
import sys
s = open(sys.argv[1], encoding="utf-8").read()
old = "if cs.modal.is_none()\n                        && !cs.rows.is_empty()"
new = "if cs.modal.is_some()\n                        && !cs.rows.is_empty()"
assert old in s, "变异靶串不在——源码形态变了，考题要跟着修"
open(sys.argv[2], "w", encoding="utf-8").write(s.replace(old, new, 1))
PY
if bash "$GUARD" "$TMP/mut-flip.rs" >/dev/null 2>&1; then
    echo "❌ 变异③（闸语义反转 is_some）没被咬住——守卫是摆设"
    exit 1
fi

echo "✅ BAR-114 模态压顶守卫考题：真源绿 + 两枚变异全咬"
