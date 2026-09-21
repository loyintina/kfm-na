#!/bin/bash
# check-modal-cursor.sh — 跳框模态压顶源码守卫（BAR-114，2026-09-21 形态升级）
#
# 病灶形态：组件池跳框被下池三级框光标行压盖——BAR-096 拆层后下池光标层
# 合成序在配置槽之上，而跳框模态只画进配置槽画布（槽内最后画=槽内最上），
# 槽外还有光标层 → 模态被压。用户实机目击「下池的三级框行叠加在跳框上」。
#
# 守卫契约：src/android_app.rs 里每一处 lp.cursor / lp.cursor_old 合成位
# 赋值都必须处在 cs.modal.is_none() 闸的保护窗内——跳框模态 = 配置页
# 最上层，光标层（及未来任何拆层件）的合成位不许不问模态在不在。
#
# 用法：check-modal-cursor.sh [源文件]（默认 src/android_app.rs；考题传
# 变异样本判红）
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

SRC="${1:-src/android_app.rs}"

python3 - "$SRC" <<'PY'
import re, sys

lines = open(sys.argv[1], encoding="utf-8").read().splitlines()

hits = [i for i, l in enumerate(lines) if re.search(r"lp\.cursor(_old)?\s*=\s*Some", l)]
if not hits:
    print("❌ 一个 lp.cursor/cursor_old 合成位都没找到——守卫盲区，不许静默")
    sys.exit(1)

bad = 0
for i in hits:
    # 保护窗：向上 25 行内必须见到 modal.is_none 闸（同 if 条件块或其
    # 包裹层）；窗内先撞见另一个 lp.* 赋值 = 闸不属本赋值，继续上探
    win = lines[max(0, i - 25):i]
    if not any("modal.is_none" in w for w in win):
        print(f"❌ 第 {i+1} 行 {lines[i].strip()[:50]} 上方 25 行无 modal.is_none 闸——BAR-114 压顶红线")
        bad += 1

if bad:
    sys.exit(1)
print(f"[check-modal-cursor] OK — {len(hits)} 处光标层合成位全部吃 modal.is_none 闸（模态压顶）")
PY
