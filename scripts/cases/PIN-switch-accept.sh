#!/bin/bash
# PIN-switch-accept.sh — 考官:switch-req 通道切换往返契约(2026-08-28,
# 调试闸门.md §十五通道九;与 Ctrl-] 同入口的遥控器)
#
# 判三步:
#   ①记当前活跃会话 X;
#   ②switch-req 点火 → active 必翻转为另一会话 Y(10s 上限);
#   ③再点火 → active 必回到 X。往返分毫不差=切换契约在。
# 前置:两会话都在(sessions=local,remote)。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-switch

active() { bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep '^active=' | cut -d= -f2; }

x=$(active)
[ -n "$x" ] || fail PIN-switch "读不到 active——stats 通道活着吗"
case "$x" in local) y=remote ;; remote) y=local ;; *) fail PIN-switch "活跃会话值异常:$x" ;; esac

flip() {
    gate "touch $NA_TMP/switch-req" || return 1
    for _ in $(seq 1 20); do
        sleep 0.5
        [ "$(active)" = "$1" ] && return 0
    done
    return 1
}

flip "$y" || fail PIN-switch "首次点火后 active 未翻转为 $y(仍 $(active))"
flip "$x" || fail PIN-switch "回切后 active 未回到 $x(仍 $(active))"
pass PIN-switch "切换往返 $x→$y→$x 分毫不差(通道九=Ctrl-] 同入口)"
