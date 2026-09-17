#!/bin/bash
# test-check-fix-instrument.sh — 仪器证据门（check-fix-instrument.sh）回归考题
# （2026-09-17，挂 chain 第 9 步，与 Rust 钉同效）
#
# 形态效力阶梯（nz 范式）：守卫本身也要被守卫——本考题钉死八言判卷，
# 守卫被改坏/误放 chain 红；守卫被摘另有 chain 第 3 步自守卫拦截。
set -u
cd "$(dirname "$0")/.." || exit 1

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# 夹具 bugs.md：一行带证据词（BAR-901），一行裸（BAR-902），一行编号前缀陷阱（BAR-90 不存在但有 BAR-901）
cat >"$TMP/bugs.md" <<'EOF'
| BAR-901 | x | 定罪：vc449 panend 全序列差分 | V | 已修 | tests/x.rs `t` | vc450 判卷达标 |
| BAR-902 | x | 纯逻辑病灶，无任何仪器 | V | 已修 | tests/x.rs `t` | 钉判卷 |
EOF
export KFM_FIX_INSTRUMENT_BUGS_MD="$TMP/bugs.md"

pass=0; fail=0
# 言 <期望exit> <用例名> <消息文件>
言() {
  local want=$1 name=$2 msg=$3
  if bash scripts/check/check-fix-instrument.sh --staged "$msg" >/dev/null 2>&1; then got=0; else got=1; fi
  if [ "$got" -eq "$want" ]; then pass=$((pass+1)); else
    fail=$((fail+1)); echo "❌ $name：期望 exit=$want 实得 $got"
  fi
}

m() { printf '%s\n' "$1" >"$TMP/$2"; }

m 'fix(渲染): BAR-901 贴死闪变根修' ok1
m 'fix(渲染): BAR-902 裸行也想过' bad1
m 'fix(动画): 没立案直接修' bad2
m 'fix(设置页): BAR-902 纯逻辑病灶
instrument:na' ok2
m 'fix(渲染): BAR-902 正文里提 instrument:na 不算独立行豁免' bad3
m 'fix(文档): BAR-902 非感官域随便过' ok3
m 'feat(渲染): BAR-902 非 fix 不过问' ok4
m 'fix(手势): BAR-90 前缀陷阱（只有 BAR-901 行）' bad4

言 0 带证据行放行   "$TMP/ok1"
言 1 裸行拦截       "$TMP/bad1"
言 1 未立案拦截     "$TMP/bad2"
言 0 独立行豁免放行 "$TMP/ok2"
言 1 prose误认拦截  "$TMP/bad3"
言 0 非感官域跳过   "$TMP/ok3"
言 0 非fix跳过      "$TMP/ok4"
言 1 编号前缀陷阱   "$TMP/bad4"

echo "仪器证据门考题：过 $pass / 挂 $fail（8 言）"
[ "$fail" -eq 0 ] && [ "$pass" -eq 8 ]
