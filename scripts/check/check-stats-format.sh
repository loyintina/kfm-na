#!/bin/bash
# check-stats-format.sh — stats 字段 ↔ format! 占位符咬合闸(2026-08-27,
# 评审裁决建议落地:BAR-040 复核提醒,StatsSnap 加字段忘进 format! 的
# 半成品态(E0063 级)不许过链)。
#
# 判法:StatsSnap 结构体的每个 pub 字段名,必须以 `<name>=` 形态出现在
# format_stats 的格式串里。缺一个 = 加了字段忘输出(观测静默瞎一格)。
#
# 输出别名表(有意的重命名/派生,新别名必须在此登记——不许静默):
#   uptime_ms→uptime(单位挪进值)  loop_age_ms→loop_beat_age
#   draw_total_ms→draw_avg_ms(输出派生均值,原始累计不直出)
ALIASES="uptime_ms:uptime loop_age_ms:loop_beat_age draw_total_ms:draw_avg_ms"
set -euo pipefail
cd "$(dirname "$0")/../.."

src=src/gate.rs
# 结构体字段:pub struct StatsSnap { ... } 区块里的 pub <name>:
fields=$(awk '/pub struct StatsSnap \{/,/^\}/' "$src" | grep -oE 'pub [a-z_0-9]+:' | awk '{print $2}' | tr -d ':' | sort)
# format! 区块(取 format_stats 函数体)
fmt=$(awk '/pub fn format_stats/,/^}/' "$src")

missing=""
for f in $fields; do
    out="$f"
    for a in $ALIASES; do
        [ "${a%%:*}" = "$f" ] && out="${a##*:}"
    done
    case "$fmt" in
        *"$out="*) ;;          # 输出键名后紧跟 = 才算咬合(防子串误认)
        *) missing="$missing $f(输出键:$out)" ;;
    esac
done
if [ -n "$missing" ]; then
    echo "❌ StatsSnap 字段未进 format! 输出:$missing" >&2
    exit 1
fi
n=$(echo "$fields" | wc -l)
echo "[check-stats-format] OK — $n 个字段全部咬合"
