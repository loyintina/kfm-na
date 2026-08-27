#!/bin/bash
# na-regress.sh — 真机回归套件 runner(2026-08-27,调试闸门.md §十四配套)
#
#   bash scripts/na-regress.sh              跑全部考官
#   bash scripts/na-regress.sh BAR-040 ...  只跑点名的
#
# 考官 = scripts/cases/*-accept.sh,统一协议:
#   exit 0 过 / 非 0 挂 / 77 跳过(手机不可达/数据不足);stdout 一行证据。
# 命名两类:BAR-xxx = 案卷判卷法固化;PIN-xxx = 行为钉(无编号案/特性)。
#
# 顺序纪律:普通考官在前,「重启类」(脚本内含 na-restart 的)在后——
# 重启会清现场,不许干扰别的考官。runner 靠脚本头注释「重启类」分拣。
# 末尾 exit 码 = 有无挂卷(0 全过或全跳过,1 有挂)。
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CASES="$ROOT/scripts/cases"

if [ $# -gt 0 ]; then
    files=()
    for bar in "$@"; do
        f="$CASES/$bar-accept.sh"
        [ -f "$f" ] || { echo "❌ 没有考官 $f" >&2; exit 2; }
        files+=("$f")
    done
else
    shopt -s nullglob
    files=("$CASES"/*-accept.sh)
fi
if [ ${#files[@]} -eq 0 ]; then
    echo "(还没有考官——scripts/cases/*-accept.sh 为空)" >&2
    exit 0
fi

# 分拣:普通在前,重启类在后(头 20 行内声明「重启类」)
normal=(); restarter=()
for f in "${files[@]}"; do
    if head -20 "$f" | grep -q "重启类"; then
        restarter+=("$f")
    else
        normal+=("$f")
    fi
done

passed=0; failed=0; skipped=0; failed_names=()
echo "=== na 真机回归 $(date '+%m-%d %H:%M') | ${#files[@]} 卷 ==="
for f in "${normal[@]}" "${restarter[@]}"; do
    name=$(basename "$f" -accept.sh)
    out=$(timeout 120 bash "$f" 2>&1)
    rc=$?
    case $rc in
        0)  passed=$((passed+1));  echo "$out" ;;
        77) skipped=$((skipped+1)); echo "$out" ;;
        *)  failed=$((failed+1));  failed_names+=("$name"); echo "$out" ;;
    esac
done

echo "=== 报表:过 $passed / 挂 $failed / 跳过 $skipped ==="
if [ "$failed" -gt 0 ]; then
    echo "挂卷:${failed_names[*]}" >&2
    exit 1
fi
exit 0
