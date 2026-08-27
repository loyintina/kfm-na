#!/bin/bash
# BAR-040-accept.sh — 考官:开局横幅必须在首屏(2026-08-27,案卷判卷法固化)
#
# 案卷: bugs.md BAR-040——banner 在 BOOT 80 列印、2ms 后 resize 61 列
# 重排折行 +2,标题两行被顶出首屏(用户要下滑才见)。修复 = banner
# 移居首个真实 resize 之后印。
#
# 判卷法(案卷原文的机器化):重启后**不滚动**直接读屏,
# 首屏必须含标题行「kfm-na 就绪」。含 = 过;10 秒等到稳定仍不含 = 挂。
#
# 注意:本考官会重启 na(判卷法要求从冷启动看),套件里属「重启类」,
# runner 把它排在最后,免得杀掉其他考官的现场。
# 例外:SKIP_RESTART=1 = 冒烟模式——热更闭环刚重启过,直接判当前 boot。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device BAR-040

if [ "${SKIP_RESTART:-0}" != 1 ]; then
    # 冷启动(na-restart 自带 boot 行等待 + ping 判卷)
    if ! bash "$NA_ROOT/scripts/na-restart.sh" >/dev/null 2>&1; then
        fail BAR-040 "重启闭环没走完(熄屏/ROM 冻结?),无法判卷"
    fi
fi

# 首屏稳定后读文本:banner 在 resize 后印,shell 起提示符还要几拍,
# 最多等 10s;标题一旦出现即判
for _ in $(seq 1 20); do
    sleep 0.5
    text=$(bash "$NA_ROOT/scripts/na-text.sh" 2>/dev/null) || continue
    if echo "$text" | grep -q "kfm-na 就绪"; then
        line=$(echo "$text" | grep -n "kfm-na 就绪" | head -1 | cut -d: -f1)
        pass BAR-040 "首屏第 ${line} 行见标题「kfm-na 就绪」(未滚动)"
    fi
done
fail BAR-040 "重启 10s 后首屏无标题「kfm-na 就绪」——横幅又被顶出视野?"
