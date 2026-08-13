#!/bin/bash
# check-fix-tests.sh — fix 带钉耦合门（hard fail，移植自 kfmv4 同名检查）
#
# 思想：修 bug 不带回归钉 = 同一个 bug 会回来第二次。
# 规则：提交信息首行是 fix: / fix(范围): 且未触及测试且提交信息无豁免 → 中断。
# 「触及测试」口径（Rust）：tests/ 目录、*_test.rs、tests.rs，或 diff 中含 #[cfg(test)]。
# 豁免：提交信息**独立一行**写 `tests:na`（声明此修复无需/无法补钉，如纯配置/文案）。
#
# 用法：check-fix-tests.sh --staged <msgFile>   （commit-msg 钩子）
#       check-fix-tests.sh                     （构建链兜底，查 HEAD）
cd "$(dirname "$0")/../.." || exit 1

if [ "$1" = "--staged" ]; then
  files=$(git diff --cached --name-only 2>/dev/null)
  message=$(cat "$2" 2>/dev/null)
  label="本次提交（暂存区）"
else
  files=$(git show --name-only --format= HEAD 2>/dev/null)
  message=$(git log -1 --format=%B 2>/dev/null)
  label="HEAD 提交"
fi

first_line=$(echo "$message" | head -1)
echo "$first_line" | grep -qE '^fix(\([^)]*\))?:' || { echo "[check-fix-tests] OK — ${label}（非 fix 提交）"; exit 0; }

touched_tests=$(echo "$files" | grep -cE '^tests/|_test\.rs$|/tests\.rs$|(^|/)tests\.rs$' || true)
exempt=$(echo "$message" | grep -cxE 'tests:na[[:space:]]*' || true)

if [ "$touched_tests" -eq 0 ] && [ "$exempt" -eq 0 ]; then
  echo "╔══════════════════════════════════════════════════════════════╗"
  echo "║  🚫 fix 提交未带回归钉                                        ║"
  echo "╚══════════════════════════════════════════════════════════════╝"
  echo "[check-fix-tests] ❌ ${label}是 fix 但未触及测试。修 bug 必须补回归钉"
  echo "[check-fix-tests] （BAR 编号入测试名 + docs/ledger/bugs.md 登记）；"
  echo "[check-fix-tests] 确属无需补钉（纯配置/文案/构建修复），提交信息独立一行写 tests:na 豁免"
  exit 1
fi
echo "[check-fix-tests] OK — ${label}（fix 带钉$([ "$exempt" -gt 0 ] && echo '，tests:na 豁免')）"
