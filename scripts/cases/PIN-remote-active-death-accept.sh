#!/bin/bash
# PIN-remote-active-death-accept.sh — 故障注入探针③:活跃=远程死亡自动重孵契约
# (2026-08-28,调试闸门.md §十五探针族;靠通道九 switch-req 解锁)
#
# 判五步:
#   ①安全闸:两会话都在(sessions=local,remote);
#   ②switch-req 切到远程(若已在远程则免),确认 active=remote;
#   ③注入:服务器本机 ss -K 掐断 8021 ws——远程作为**活跃方**死亡;
#   ④契约判卷:deaths +1 且 active **仍是 remote**(自动重孵不换名);
#   ⑤重孵实证:重连横幅出现 + 新 shell 回显正常。
#
# ⚠ 运行代价:会弹一次远程会话(重孵=新 shell;amp 的 tmux 现场无损,
#   重连接回即恢复)。宜在远程空闲时跑。可重复:每次运行=真实一次
#   断网重孵,无锁存(与待机半边不同——活跃死亡有自动重孵,天然可重复)。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-remote-active-death

stats_field() {
    bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep "^$1=" | cut -d= -f2
}

# ① 前置:两会话都在
sess=$(stats_field sessions)
case "$sess" in *local*remote*|*remote*local*) : ;; *) echo "⏭ PIN-remote-active-death | sessions 缺一方($sess),跳过" >&2; exit 77 ;; esac

# ② 切到远程(已在则免)
if [ "$(stats_field active)" != "remote" ]; then
    gate "touch $NA_TMP/switch-req" || fail PIN-remote-active-death "switch-req 点火失败"
    ok=""
    for _ in $(seq 1 20); do
        sleep 0.5
        [ "$(stats_field active)" = "remote" ] && { ok=1; break; }
    done
    [ -n "$ok" ] || fail PIN-remote-active-death "切远程未生效(10s)——通道九失灵?"
fi

deaths_before=$(stats_field session_deaths)

# ③ 注入:掐 ws——远程作为活跃方死亡(服务器本机合法运维动作)
ss -K state established '( sport = :8021 )' >/dev/null 2>&1

# ④ 契约判卷:deaths+1 且 active 仍是 remote(自动重孵不换名)
ok=""
for _ in $(seq 1 30); do
    sleep 0.5
    now=$(stats_field session_deaths) && [ -n "$now" ] || continue
    now_act=$(stats_field active)
    if [ "$now" -ge $((deaths_before + 1)) ] && [ "$now_act" = "remote" ]; then ok=1; break; fi
done
[ -n "$ok" ] || fail PIN-remote-active-death "死亡后 15s:deaths=$now active=$now_act——活跃死亡自动重孵契约违约?"

# ⑤ 重孵实证:横幅 + 新 shell 回显
alive=""
for _ in $(seq 1 16); do
    sleep 0.5
    text=$(bash "$NA_ROOT/scripts/na-text.sh" 2>/dev/null) || continue
    if echo "$text" | grep -q "已重连" && echo "$text" | grep -q "stdby_42"; then alive=1; break; fi
done
# 回显探测(横幅可能已滚出视野,回显是硬证据)
if [ -z "$alive" ]; then
    bash "$NA_ROOT/scripts/na-type.sh" 'echo rah_$((40+2))\r' >/dev/null
    for _ in $(seq 1 10); do
        sleep 0.5
        text=$(bash "$NA_ROOT/scripts/na-text.sh" 2>/dev/null) || continue
        echo "$text" | grep -q "rah_42" && { alive=1; break; }
    done
fi
[ -n "$alive" ] || fail PIN-remote-active-death "重孵后远程无回显——自动重孵名存实亡?"

pass PIN-remote-active-death "活跃远程死亡:deaths $deaths_before→$now,active 保持 remote,重孵+回显实证"
