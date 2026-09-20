#!/bin/bash
# test-bar123-keybar-ruler.sh — BAR-123 同尺守卫的考题（变异抽检，2026-09-21）
#
# 考三件事：
#   ①真源必须绿（守卫不报假警）；
#   ②变异「抬手 hit 摘掉 chrome_inset」（BAR-123 原发病灶形态）→ 必须红；
#   ③变异「Started in_bar 摘掉 chrome_inset」（对称病灶形态）→ 必须红。
# 变异在临时副本上做（守卫吃文件参数），不碰工作区。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

GUARD=scripts/check/check-keybar-ruler.sh
SRC=src/android_app.rs
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# ①真源绿
bash "$GUARD" "$SRC" >/dev/null || { echo "❌ 真源被判红——守卫误伤"; exit 1; }

# ②变异：抬手 hit 摘 chrome_inset（BAR-123 病灶原样复刻）
python3 - "$SRC" "$TMP/mut-hit.rs" <<'PY'
import sys
s = open(sys.argv[1], encoding="utf-8").read()
old = "crate::keybar::hit(\n                        x,\n                        y,\n                        s.width,\n                        s.height,\n                        self.chrome_inset() + self.cur_bar_h(),\n                    )"
new = "crate::keybar::hit(x, y, s.width, s.height, self.cur_bar_h())"
assert old in s, "变异靶串不在——源码形态变了，考题要跟着修"
open(sys.argv[2], "w", encoding="utf-8").write(s.replace(old, new, 1))
PY
if bash "$GUARD" "$TMP/mut-hit.rs" >/dev/null 2>&1; then
    echo "❌ 变异②（抬手 hit 摘 chrome_inset）没被咬住——守卫是摆设"
    exit 1
fi

# ③变异：Started in_bar 摘 chrome_inset（对称病灶）
python3 - "$SRC" "$TMP/mut-inbar.rs" <<'PY'
import sys
s = open(sys.argv[1], encoding="utf-8").read()
old = "crate::keybar::in_bar(y, sh, self.chrome_inset() + bar_h)"
new = "crate::keybar::in_bar(y, sh, bar_h)"
assert old in s, "变异靶串不在——源码形态变了，考题要跟着修"
open(sys.argv[2], "w", encoding="utf-8").write(s.replace(old, new, 1))
PY
if bash "$GUARD" "$TMP/mut-inbar.rs" >/dev/null 2>&1; then
    echo "❌ 变异③（Started in_bar 摘 chrome_inset）没被咬住——守卫是摆设"
    exit 1
fi

echo "✅ BAR-123 同尺守卫考题：真源绿 + 两枚变异全咬"
