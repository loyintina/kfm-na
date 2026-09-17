#!/bin/bash
# check-fix-instrument.sh — C档感官域 fix 的仪器证据门（hard fail，2026-09-17 立）
#
# 思想（BAR-104/105/106 三部曲 + nz 唯一产出迭代范式「形态效力阶梯」）：
# 感官域修复没有仪器证据 = 假设驱动修复——BAR-098/099 式「修了真 bug 但不
# 是用户报的那个」，三天三部曲的直接成因。教训只挂账不入节奏 = 漏 cd 三天
# 三犯（nz EXP-009）；本门把「先仪器定罪再动手」从文档段落升成代码守卫。
#
# 规则：提交信息首行命中 fix(渲染|动画|设置页|手势|平移): 时——
#   1. 提交信息必须引 BAR-NNN（钉纪律的延伸：感官域修复必须先立案）；
#   2. docs/ledger/bugs.md 对应行须含观测证据通道词（定罪或判卷列有其一）：
#      panend/panc/差分/遥测/实录/录屏/截屏/na-rec/epoch/帧账/redroid
# 豁免：纯逻辑/数学病灶（钉即判卷，仪器不适用）——提交信息**独立一行**
# 写 `instrument:na`（独立行语法防 prose 误认，同 tests:na 先例）。
#
# 用法：check-fix-instrument.sh --staged <msgFile>   （commit-msg 钩子）
#       check-fix-instrument.sh                     （构建链兜底，查 HEAD）
# 测试通道：KFM_FIX_INSTRUMENT_BUGS_MD 环境变量覆盖 bugs.md 路径
# （scripts/test-check-fix-instrument.sh 用；本门不读暂存文件清单，与 git 状态无关）
cd "$(dirname "$0")/../.." || exit 1

BUGS_MD="${KFM_FIX_INSTRUMENT_BUGS_MD:-docs/ledger/bugs.md}"

if [ "$1" = "--staged" ]; then
  message=$(cat "$2" 2>/dev/null)
  label="本次提交（暂存区）"
else
  message=$(git log -1 --format=%B 2>/dev/null)
  label="HEAD 提交"
fi

first_line=$(echo "$message" | head -1)
echo "$first_line" | grep -qE '^fix\((渲染|动画|设置页|手势|平移)\):' || {
  echo "[check-fix-instrument] OK — ${label}（非感官域 fix）"; exit 0; }

exempt=$(echo "$message" | grep -cxE 'instrument:na[[:space:]]*' || true)
if [ "$exempt" -gt 0 ]; then
  echo "[check-fix-instrument] OK — ${label}（instrument:na 豁免）"; exit 0; fi

bar=$(echo "$message" | grep -oE 'BAR-[0-9]+' | head -1)
row=""
[ -n "$bar" ] && row=$(grep -F "| ${bar} |" "$BUGS_MD" 2>/dev/null | head -1)

if [ -z "$row" ]; then
  echo "╔══════════════════════════════════════════════════════════════╗"
  echo "║  🚫 感官域 fix 未立案/账本行缺失                              ║"
  echo "╚══════════════════════════════════════════════════════════════╝"
  echo "[check-fix-instrument] ❌ ${label}是感官域 fix 但提交信息未引 BAR-NNN"
  echo "[check-fix-instrument] 或 docs/ledger/bugs.md 无对应行。感官域修复必须先立案；"
  echo "[check-fix-instrument] 纯逻辑病灶豁免：提交信息独立一行写 instrument:na"
  exit 1
fi

if ! echo "$row" | grep -qE 'panend|panc|差分|遥测|实录|录屏|截屏|na-rec|epoch|帧账|redroid'; then
  echo "╔══════════════════════════════════════════════════════════════╗"
  echo "║  🚫 感官域 fix 无仪器证据——假设驱动修复拦截                   ║"
  echo "╚══════════════════════════════════════════════════════════════╝"
  echo "[check-fix-instrument] ❌ ${bar} 账本行无观测证据通道词（panend/panc/差分/"
  echo "[check-fix-instrument] 遥测/实录/录屏/截屏/na-rec/epoch/帧账/redroid）。"
  echo "[check-fix-instrument] 先仪器定罪再动手（BAR-098/099 假设驱动修复教训）；"
  echo "[check-fix-instrument] 纯逻辑病灶豁免：提交信息独立一行写 instrument:na"
  exit 1
fi
echo "[check-fix-instrument] OK — ${label}（${bar} 行带观测证据）"
