#!/bin/bash
# PIN-signal-accept.sh — 考官:坠机信号链路活着(2026-08-27,
# 自观测第四块①判卷法固化——f949730 SIGURG 修复的实弹判卷姿势)
#
# 判卷法:kill -URG $(cat na.pid) → panic.log 的 SIGNAL 行数 +1,
# 且进程不死。ART 认领 SIGUSR1 的教训(f949730):探针信号选错 =
# handler 装了也白装——本考官每次跑都是对「链路真活着」的再确认。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-signal

count() { gate "grep -ac SIGNAL $NA_TMP/panic.log 2>/dev/null || echo 0"; }

before=$(count)
gate "kill -URG \$(cat $NA_TMP/na.pid)" \
    || fail PIN-signal "kill 没发出去(na.pid 还在吗)"
sleep 1
after=$(count)
if [ "$after" -le "$before" ]; then
    fail PIN-signal "URG 后 SIGNAL 行数没涨($before→$after)——handler 死了?(ART 又抢信号?)"
fi
if ! gate "kill -0 \$(cat $NA_TMP/na.pid)"; then
    fail PIN-signal "探针把进程打死了——SIGURG 路径不许致命"
fi
pass PIN-signal "SIGNAL 行 $before→$after,进程活着(sig=23 链路在)"
