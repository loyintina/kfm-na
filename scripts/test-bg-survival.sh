#!/bin/bash
# test-bg-survival.sh — BAR-029 实拍判卷:na 退后台后 8024 闸门必须存活
#
# 原理(2026-08-23 实证闭环):
#   控制面:8022(Termux)`am start` 遥控前后台切换,不用人碰手机;
#   探针面:8024 ssh 探针——na 被 cached-app 冻结器冻住时的症状是
#   「Connection timed out during banner exchange」(TCP 握手由内核
#   backlog 完成,但进程冬眠发不出 banner),一探一个准。
#
# 流程:前台基线通 → 切后台 → 持续探测(默认 120s,每 10s 一探) →
# 拉回前台。后台全程零断流 = 通过。
#
# 用法: bash scripts/test-bg-survival.sh [持续秒数,默认 120]
# 前提: kalo 隧道活着(8022/8024),探针钥匙 /root/.ssh/na_probe_key
set -uo pipefail

NA_KEY=/root/.ssh/na_probe_key

probe() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost true >/dev/null 2>&1
}

phone() {
    ssh -p 8022 -o ConnectTimeout=8 -o StrictHostKeyChecking=no \
        localhost "$1" >/dev/null 2>&1
}

dur=${1:-120}

echo "== 1/4 基线:na 拉前台,探针必须通"
phone 'am start -n dev.kfm.na/.MainActivity'
sleep 4
probe || { echo "❌ 前台基线探针不通——先修闸门本身(kalo/na-sshd)"; exit 1; }
echo "   ✅ 前台探针通"

echo "== 2/4 切后台(Termux 上前台)"
phone 'am start -n com.termux/.app.TermuxActivity'

echo "== 3/4 后台保活观察 ${dur}s(每 10s 一探)"
fails=0
for ((i = 10; i <= dur; i += 10)); do
    sleep 10
    if probe; then echo "   [+${i}s] ✅ 通"; else echo "   [+${i}s] ❌ 断"; fails=$((fails + 1)); fi
done

echo "== 4/4 恢复:na 拉回前台"
phone 'am start -n dev.kfm.na/.MainActivity'

if [ "$fails" -eq 0 ]; then
    echo "✅ BAR-029 判卷通过:后台 ${dur}s 探针零断流"
else
    echo "❌ BAR-029 未愈:后台断流 $fails 次"
    exit 1
fi
