#!/usr/bin/env bash
# grid-text-ratchet.sh — 自然步进文字 API 棘轮闸（BAR-178，2026-09-29 用户拍板）
#
# 执法对象：src/ 下绕过网格文字引擎（ui::grid_text + termview 网格引擎块）、
# 直调自然步进文字 API 的**调用点**。新功能文字/布局不走引擎 = chain 红；
# 存量登记在基线里，迁移一面收一面——棘轮只许缩不许涨。
#
# 判据（BAR-188 改约：比对键 = file:symbol 多重集，行号不入键）：
#   出现基线外新调用点     → 红（新代码必须走引擎，契约 docs/active/网格文字.md）
#   基线行对不上现存调用点 → 红（迁移顺手用 --bless 收紧基线，与迁移同提交）
#   调用点行号平移         → 绿（BAR-188 前按 file:line:symbol 精确匹配，任何
#                            在调用点上方插行的无关提交都误红——BAR-178 落地与
#                            BAR-182 各撞一次。行号是位置噪声不是行为信号）
# 已知盲区（明文挂账）：同一文件同一符号一删一增对冲时多重集不变、闸不可分辨
#   ——闸是绊线不是证明，对冲情形由 code review 兜。
#
# 用法：
#   scripts/check/grid-text-ratchet.sh           # 判卷（chain 第 5 步）
#   scripts/check/grid-text-ratchet.sh --bless   # 以现状重签基线（仅迁移提交用）
#
# 夹具测试口（test-grid-text-ratchet.sh 用，正常判卷别设）：
#   GRID_TEXT_SRC=路径       替代扫描根（默认 src）
#   GRID_TEXT_BASELINE=路径  替代基线文件
set -euo pipefail
cd "$(dirname "$0")/../.."

SRC_DIR="${GRID_TEXT_SRC:-src}"
BASELINE="${GRID_TEXT_BASELINE:-scripts/check/grid-text-baseline.txt}"
# 自然步进 API 名单（定义行不算调用点；引擎自家用词 measure_items_grid/
# draw_grid_text_left 与本表无子串碰撞，天然豁免）
APIS='measure_items|measure_bar_items|text_width|draw_text_left|draw_text_left_ex|draw_text_centered|draw_text_centered_yclip|draw_field_lines|draw_items_left|draw_label'

scan() {
    # 键 = file:symbol（多重集：同文件同符号多次调用逐条登记，行号剥离）
    # 标识符前缀并入匹配（[A-Za-z_]*）：引擎件 grid_text_width/measure_items_grid
    # 以旧件 text_width/measure_items 为后缀，裸匹配会把引擎调用点误登记为旧件
    # 存量（BAR-196 实证基线 +6 幽灵行）——并入前缀取符号全名后按名单全名过滤
    grep -rnoE "[A-Za-z_]*($APIS)\(" "$SRC_DIR" --include='*.rs' \
        | grep -vE "fn ($APIS)\(" \
        | sed 's/^\(.*\):[0-9]*:\([A-Za-z_]*\)(/\1:\2/' \
        | grep -E ":($APIS)$" \
        | sort
}

if [ "${1:-}" = "--bless" ]; then
    scan > "$BASELINE"
    echo "[grid-text-ratchet] 基线已重签：$(wc -l < "$BASELINE") 条存量调用点（file:symbol 多重集）"
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
