#!/bin/bash
# test-hot-core-vc.sh — 推核前闸的自钉（MAIN0192 楼418／420）
#
# 闸要能被咬：正确戳放行、错戳拒、无戳拒、多戳拒、用法错拒。
# 变异抽检：把 PASS/FAIL 判据反转、或去掉「无戳拒」分支 → 本考题必红。
set -u
CHK="$(cd "$(dirname "$0")" && pwd)/check/check-hot-core-vc.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fails=0

mk() { # mk <文件名> <内容>
  printf '%s' "$2" >"$tmp/$1"
}

chk() { # chk <期望 rc> <说明> <参数...>
  local want=$1 desc=$2
  shift 2
  bash "$CHK" "$@" >"$tmp/out" 2>&1
  local got=$?
  if [ "$got" -ne "$want" ]; then
    echo "❌ $desc：期望 rc=$want，实得 rc=$got"
    sed 's/^/    /' "$tmp/out"
    fails=$((fails + 1))
  else
    echo "✓ $desc"
  fi
}

mk good.so 'junk vc1792229184 tail'
mk other.so 'junk vc1792312241 tail'
mk none.so 'junk without stamp'
mk multi.so 'vc1792229184 and vc1792312241'

chk 0 "戳一致放行" "$tmp/good.so" 1792229184
chk 1 "戳不一致拒" "$tmp/other.so" 1792229184
chk 2 "无戳拒" "$tmp/none.so" 1792229184
chk 3 "多戳拒" "$tmp/multi.so" 1792229184
chk 4 "versionCode 非数字拒" "$tmp/good.so" vc1792229184
chk 2 "核文件不存在拒" "$tmp/no-such.so" 1792229184

# 第七档（楼517 悬铃指出结构问题后改）：**「闸脚本在位」不能由脚本自己查**——
# 脚本被删时它根本不会执行。那一半已上移到调用方 chain.sh（缺件红与脚本自报错
# 的红分开）。本脚本只查「它是否真的被 chain 调着」这一件——这需要本脚本存在才
# 能查，故留在内。
CHAIN="$(dirname "$CHK")/../chain.sh"
if grep -q 'check-hot-core-vc.sh' "$CHAIN" 2>/dev/null; then
  echo "✓ 闸已挂进 chain"
else
  echo "❌ chain 里没有 check-hot-core-vc.sh 的调用链——闸等于不存在"
  fails=$((fails + 1))
fi

if [ "$fails" -ne 0 ]; then
  echo "[test-hot-core-vc] $fails 挂"
  exit 1
fi
echo "[test-hot-core-vc] 6 过 / 0 红"
