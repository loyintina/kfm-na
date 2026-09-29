#!/usr/bin/env bash
# grid-text-ratchet.sh — 自然步进文字 API 棘轮闸（BAR-178，2026-09-29 用户拍板）
#
# 执法对象：src/ 下绕过网格文字引擎（ui::grid_text + termview 网格引擎块）、
# 直调自然步进文字 API 的**调用点**。新功能文字/布局不走引擎 = chain 红；
# 存量登记在基线里，迁移一面收一面——棘轮只许缩不许涨。
#
# 判据：
#   出现基线外新调用点   → 红（新代码必须走引擎，契约 docs/active/网格文字.md）
#   基线行对不上现存调用点 → 红（迁移顺手用 --bless 收紧基线，与迁移同提交）
#
# 用法：
#   scripts/check/grid-text-ratchet.sh           # 判卷（chain 第 5 步）
#   scripts/check/grid-text-ratchet.sh --bless   # 以现状重签基线（仅迁移提交用）
set -euo pipefail
cd "$(dirname "$0")/../.."

BASELINE=scripts/check/grid-text-baseline.txt
# 自然步进 API 名单（定义行不算调用点；引擎自家用词 measure_items_grid/
# draw_grid_text_left 与本表无子串碰撞，天然豁免）
APIS='measure_items|measure_bar_items|text_width|draw_text_left|draw_text_left_ex|draw_text_centered|draw_text_centered_yclip|draw_field_lines|draw_items_left|draw_label'

scan() {
    grep -rnoE "($APIS)\(" src/ --include='*.rs' \
        | grep -vE "fn ($APIS)\(" \
        | sed 's/^\(.*\):\([0-9]*\):\([A-Za-z_]*\)(/\1:\2:\3/' \
        | sort
}

if [ "${1:-}" = "--bless" ]; then
    scan > "$BASELINE"
    echo "[grid-text-ratchet] 基线已重签：$(wc -l < "$BASELINE") 条存量调用点"
    exit 0
fi

[ -f "$BASELINE" ] || { echo "[grid-text-ratchet] ✗ 基线缺失 $BASELINE（--bless 首签）"; exit 1; }
scan > /tmp/grid-text-ratchet.now
new=$(comm -13 "$BASELINE" /tmp/grid-text-ratchet.now)
gone=$(comm -23 "$BASELINE" /tmp/grid-text-ratchet.now)
if [ -n "$new" ]; then
    echo "[grid-text-ratchet] ✗ 基线外新增自然步进调用点（新功能文字必须走网格引擎）："
    echo "$new" | sed 's/^/    /'
    exit 1
fi
if [ -n "$gone" ]; then
    echo "[grid-text-ratchet] ✗ 基线行对不上现存调用点（迁移面收紧基线：--bless 后与迁移同提交）："
    echo "$gone" | sed 's/^/    /'
    exit 1
fi
echo "[grid-text-ratchet] OK — $(wc -l < "$BASELINE") 条存量登记在册，零新增"
