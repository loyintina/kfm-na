#!/bin/bash
# test-serve-overlays.sh — serve-overlays 单例守卫判卷(2026-08-24)
#
# 判的坑:8027 已被占(kalo 的 overlay 服务)时,serve-overlays.sh 必须
# 退让不抢——重复开会撞端口,exec python3 直接崩(set -e 下 = 脚本死)。
#
# 判法:先自己占 8027(假 kalo),跑脚本,期望「跳过」且干净 exit 0、
# 占坑者毫发无损。host/手机通吃(只需 python3 + pgrep)。
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"

# 占坑:假 kalo overlay 服务
python3 -m http.server 8027 --bind 127.0.0.1 >/dev/null 2>&1 &
occupant=$!
trap 'kill $occupant 2>/dev/null' EXIT
sleep 0.5

out="$(bash "$here/serve-overlays.sh" 2>&1)"
rc=$?

fail() { echo "❌ $1"; exit 1; }

[ $rc -eq 0 ] || fail "被占时应干净退让(exit 0),实际 rc=$rc out=$out"
echo "$out" | grep -q "跳过" || fail "被占时应报跳过,实际: $out"
kill -0 $occupant 2>/dev/null || fail "占坑服务被误伤——守卫不该碰别人"
curl -s -o /dev/null --max-time 3 http://127.0.0.1:8027/ \
    || fail "占坑服务被抢占后失能"

echo "✅ serve-overlays 单例守卫:被占退让、不占坑者不伤"
