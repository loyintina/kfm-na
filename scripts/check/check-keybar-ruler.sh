#!/bin/bash
# check-keybar-ruler.sh — 快捷键行「三处同尺」源码守卫（BAR-123，2026-09-21）
#
# 病灶形态：键盘弹起态快捷键行抬手全落空——BAR-119 摘解析页 inset 链时
# 误把抬手 hit 的 chrome_inset 摘掉（只剩输入栏高），Started in_bar/渲染
# 仍吃 chrome_inset+栏高，按下认、抬手丢。遥测铁证：inset=890 全落空、
# inset=0 全命中；git log -L 铁证 7018c1a 改行。
#
# 守卫契约：src/android_app.rs 里每一处 keybar::hit( / keybar::in_bar(
# 调用的实参表必须含 chrome_inset——「眼手同尺」升级为「按下/抬手/渲染
# 三处同尺」，再有人摘 inset 链 = 当场红。
#
# 用法：check-keybar-ruler.sh [源文件]（默认 src/android_app.rs；考题传
# 变异样本判红）
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

SRC="${1:-src/android_app.rs}"

python3 - "$SRC" <<'PY'
import re, sys

src = open(sys.argv[1], encoding="utf-8").read()

# 抽出每个 keybar::hit( / keybar::in_bar( 的完整实参表（括号配平）
calls = []
for m in re.finditer(r"keybar::(?:hit|in_bar)\s*\(", src):
    depth, i = 0, m.end() - 1
    while i < len(src):
        c = src[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                break
        i += 1
    calls.append((src[m.start():m.start()+20].split("(")[0], src[m.end():i]))

bad = 0
for name, args in calls:
    if "chrome_inset" not in args:
        line = src[:src.find(args)].count("\n") + 1
        print(f"❌ {name} 实参缺 chrome_inset（{args.strip()[:60]}…）——BAR-123 同尺红线")
        bad += 1

if not calls:
    print("❌ 一个 keybar::hit/in_bar 调用都没找到——守卫盲区，不许静默")
    sys.exit(1)
if bad:
    sys.exit(1)
print(f"[check-keybar-ruler] OK — {len(calls)} 处 hit/in_bar 调用全部吃 chrome_inset（三处同尺）")
PY
