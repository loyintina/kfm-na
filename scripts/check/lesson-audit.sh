#!/bin/bash
# lesson-audit.sh — 教训形态审计报表（2026-09-17，nz 唯一产出迭代范式 na 落地件）
#
# 防「挂账≠入节奏」（nz 漏 cd 三天三犯 / na BAR-098~106 三天三部曲）：
# bugs.md 负责「记录」，本报表负责「形态升级有人催」——扫近 N 天 BAR 行，
# 按形态效力阶梯（代码守卫 > 回归钉 > 清单硬编码项 > 纯文档段落）分级，
# 段落级逐行列出 = 晨班会形态审计议程的输入（07:47 定时任务第一道）。
#
# 只出报表不硬拦：形态升级是 C 档判断（该不该升/怎么升归人或 agent 拍板），
# 但「没被看见」不许发生。
#
# 用法：lesson-audit.sh [天数=7]
cd "$(dirname "$0")/../.." || exit 1
DAYS=${1:-7}
CUTOFF=$(date -d "$DAYS days ago" +%F 2>/dev/null || date -v-"$DAYS"d +%F)

guard=0; pin=0; list=0; prose=0
echo "=== 教训形态审计（近 $DAYS 天，cutoff $CUTOFF） ==="
echo "--- 纯文档段落级（待升级决策） ---"
grep -E '^\| BAR-[0-9]+' docs/ledger/bugs.md | while IFS= read -r row; do
  bar=$(echo "$row" | grep -oE 'BAR-[0-9]+' | head -1)
  date=$(echo "$row" | grep -oE '20[0-9]{2}-[0-9]{2}-[0-9]{2}' | head -1)
  # 无日期行 = 老账（早期行没写日期）——不混进近 N 天报表，只计数
  if [ -z "$date" ]; then echo "NODATE $bar"; continue; fi
  [ "$date" \< "$CUTOFF" ] && continue
  case "$row" in
    *commit-msg*|*钩子*|*chain*|*棘轮*|*防泄漏闸*)
      echo "GUARD  $bar ${date:-无日期}" ;;
    *tests/*|*_test.rs*|*test-*.sh*)
      echo "PIN    $bar ${date:-无日期}" ;;
    *清单*|*矩阵*|*宪法*|*速查*)
      echo "LIST   $bar ${date:-无日期}" ;;
    *)
      echo "PROSE  $bar ${date:-无日期}  ← 段落级" ;;
  esac
done | tee /tmp/lesson-audit.$$ | awk '
  /^GUARD/ {g++} /^PIN/ {p++} /^LIST/ {l++} /^PROSE/ {pr++; print} /^NODATE/ {nd++}
  END {printf "--- 分级计数：代码守卫 %d · 回归钉 %d · 清单项 %d · 纯段落 %d（另有无日期老账 %d 行未列入） ---\n", g, p, l, pr, nd}'
rm -f /tmp/lesson-audit.$$
echo "（PROSE 行逐条过：升形态 or 写明挂账理由；有升级 → AGENTS.md/排障手册 PARADIGM 版本 +1）"
