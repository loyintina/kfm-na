#!/bin/bash
# PIN-boot-accept.sh — 考官:冷启动 boot 段不得劣化(2026-08-27,
# BAR-020~023 启动族判卷法固化;族案无单一编号,行为钉形式收编)
#
# 判卷法(排障手册 §六的机器化):trace 环里最近一次 boot
# (末一条「android_main 进入」之后)的 [+XXms boot] 行最大毫秒数
# < 3000ms。族病全是启动路径塞进秒级阻塞(76MB 字体解析/同步直报
# 冷隧道),本考官是其回归闸。阈值是「回归绊线」不是用户体感模型
# ——健康基线末行 <100ms,族病期 4600~6700ms,3000 取其间。
#
# 口径(首验自抓):只判「刚发生的 boot」。首版直接判环里最后一次
# boot,结果撞上一发熄屏boot(系统冻结 mid-boot 17.9s)误报——
# 那是环境产物不是码病。故本考官标「重启类」:全套件跑时排在
# BAR-040 之后,判它刚造的新鲜 boot;冒烟模式(SKIP_RESTART=1)
# 判热更刚完成的 boot。standalone 跑 = 判环里最后一次,证据行带
# 构建戳,误报时人对戳自知。
#
# 局限(诚实版):trace 环帽 256,开机久了 boot 行会被顶出环——
# 那时判不了,跳过(exit 77),不算挂。要新鲜判卷先跑 BAR-040(它重启)。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-boot

trace=$(bash "$NA_ROOT/scripts/na-trace.sh" 2>/dev/null) \
    || fail PIN-boot "trace 拉不到"
# 末次 android_main 进入之后的 boot 段行,取最大毫秒戳
max_ms=$(echo "$trace" | awk '
    /android_main 进入/ { boot=1; next }
    boot && /^\[\+[0-9]+ms boot\]/ {
        ms=$1; gsub(/^\[\+0*|ms.*$/, "", ms);
        if (ms+0 > m) m = ms+0
    }
    END { if (m > 0) print m }')
if [ -z "$max_ms" ]; then
    echo "⏭ PIN-boot | trace 环里已没有 boot 行(开机太久被顶出),跳过" >&2
    exit 77
fi
vc=$(echo "$trace" | grep -a "android_main 进入" | tail -1 | grep -o "vc[0-9]*")
if [ "$max_ms" -lt 3000 ]; then
    pass PIN-boot "末次 boot($vc)段末行 ${max_ms}ms < 3000ms(启动族回归闸)"
else
    fail PIN-boot "boot($vc)段末行 ${max_ms}ms ≥ 3000ms——启动路径又塞了秒级阻塞?(BAR-020~023 族病回潮)"
fi
