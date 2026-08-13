#!/bin/bash
# check-commit-docs.sh — 文档耦合门（hard fail，移植自 kfmv4 同名检查）
#
# 思想：代码提交后文档没跟上 = 文档腐化的源头。
# 规则：提交触及 src/ 或 scripts/ 但未触及 docs/ 且提交信息无豁免 → 中断。
# 豁免：提交信息**独立一行**写 `docs:na`（声明此改动无文档影响）——
#   独立行语法防正文讨论该标记时 prose 字面串误认。
#
# 用法：check-commit-docs.sh --staged <msgFile>   （commit-msg 钩子）
#       check-commit-docs.sh                     （构建链兜底，查 HEAD）
cd "$(dirname "$0")/../.." || exit 1

if [ "$1" = "--staged" ]; then
  files=$(git -c core.quotepath=false diff --cached --name-only 2>/dev/null)
  message=$(cat "$2" 2>/dev/null)
  label="本次提交（暂存区）"
else
  files=$(git -c core.quotepath=false show --name-only --format= HEAD 2>/dev/null)
  message=$(git log -1 --format=%B 2>/dev/null)
  label="HEAD 提交"
fi

touched_src=$(echo "$files" | grep -cE '^(src|scripts)/' || true)
touched_docs=$(echo "$files" | grep -cE '^docs/' || true)
exempt=$(echo "$message" | grep -cxE 'docs:na[[:space:]]*' || true)

if [ "$touched_src" -gt 0 ] && [ "$touched_docs" -eq 0 ] && [ "$exempt" -eq 0 ]; then
  echo "[check-commit-docs] ❌ $label改了 src//scripts/（${touched_src} 个代码文件）但没动 docs/"
  echo "[check-commit-docs] 若确认无文档影响，提交信息独立一行写 docs:na 豁免；否则补文档更新"
  exit 1
fi
echo "[check-commit-docs] OK — ${label}文档耦合正常$([ "$exempt" -gt 0 ] && echo '（docs:na 豁免）')"
