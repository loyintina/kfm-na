#!/bin/bash
# PIN-standby-death-accept.sh — 故障注入探针②:远程会话死亡记账契约
# (2026-08-28,调试闸门.md §十五探针族;与 PIN-rehatch 合计盖住两种传输)
#
# 判五步:
#   ①安全闸:活跃=local 且 sessions 含 remote(用户在远程作业则跳过);
#   ②注入:服务器本机 ss -K 掐断 8021 已建立 ws 连接——真实触发器=
#     网络断开(契约守护的正是这个);
#   ③session_deaths 必 +1(远程死亡记账);
#   ④活跃侧不受扰:active 仍 local;
#   ⑤服务自愈:na 传输层自动重连,8021 ws 重建,远程会话复活。
#
# 阴性对照知识(17:5x 实测,重要):杀 node 下的 tmux attach 客户端
# **不产生**死亡事件——服务端对 shell/attach 层死亡不上报,death 的
# 定义=ws 断开。故本探针 v1/v2 的 tmux 击杀路线作废,勿复用。
# 一生一发(18:1x 实测修正 v4「可重复」误判):远程待机死亡记账后
# 锁存——同进程内远程变僵尸条目,再掐 ws 也无第二枚事件(切换重连
# 后重置)。marker 记 na.pid,同进程再跑=跳过;重启/热更后重新武装。
# 遗留态=远程经传输层自动重连复活为新会话,旧 shell 现场灭(真实断
# 网行为本身)。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-standby-death

stats_field() {
    bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep "^$1=" | cut -d= -f2
}

# ① 安全闸:活跃必须 local(用户在远程作业则绝不注错),sessions 含 remote
act=$(stats_field active)
[ "$act" = "local" ] || { echo "⏭ PIN-standby-death | 活跃会话=$act(非 local),跳过" >&2; exit 77; }
sess=$(stats_field sessions)
case "$sess" in *remote*) : ;; *) echo "⏭ PIN-standby-death | sessions 无 remote($sess),跳过" >&2; exit 77 ;; esac

# ⓪ 一生一发锁存
marker="$NA_TMP/pin-standby-marker"
cur_pid=$(gate "cat $NA_TMP/na.pid" 2>/dev/null)
[ -n "$cur_pid" ] || fail PIN-standby-death "读不到 na.pid"
if gate "test -f $marker && grep -qx $cur_pid $marker" 2>/dev/null; then
    echo "⏭ PIN-standby-death | 本 na 进程($cur_pid)已消费一发(待机死亡锁存),跳过" >&2
    exit 77
fi

deaths_before=$(stats_field session_deaths)

# ② 注入:掐断服务器侧 8021 已建立 ws(=网络断,合法运维动作 ss -K)
ss -K state established '( sport = :8021 )' >/dev/null 2>&1

# ③ death 记账轮询(断开传播数秒,15s 上限)
ok=""
for _ in $(seq 1 30); do
    sleep 0.5
    now=$(stats_field session_deaths) && [ -n "$now" ] || continue
    if [ "$now" -ge $((deaths_before + 1)) ]; then ok=1; break; fi
done
[ -n "$ok" ] || fail PIN-standby-death "15s 内 session_deaths 未涨(仍 ${now:-$deaths_before})——ws 断开事件丢失?"

# ④ 活跃侧不受扰
now_act=$(stats_field active)
[ "$now_act" = "local" ] || fail PIN-standby-death "死亡后活跃会话漂移为 $now_act——远程死亡不许扰活跃"

# ⑤ 服务自愈:na 传输层自动重连,8021 应重建连接
ws=""
for _ in $(seq 1 16); do
    sleep 0.5
    n=$(ss -tn state established '( sport = :8021 )' 2>/dev/null | grep -c 8021)
    [ "${n:-0}" -ge 1 ] && { ws=1; break; }
done
[ -n "$ws" ] || fail PIN-standby-death "ws 断开后 8s 未重连——传输层自愈失灵?"

gate "echo $cur_pid > $marker"
pass PIN-standby-death "ws 掐断:deaths $deaths_before→$now,活跃 local 不受扰,8021 已自愈重连"
