#!/bin/bash
# PIN-rehatch-accept.sh — 故障注入探针:会话死亡自动重孵契约(2026-08-27,
# 调试闸门.md §十五配套;自我测试缺口③第一枚)
#
# 判卷法(§十五四步):exit 注入杀会话 → session_deaths 必 +1 →
# 活跃方自动重孵(横幅「已重连 = 新 shell」)→ 新 shell 回显活着。
# 探针后 na 自带提示符要几秒重印,统计轮询 10s。
#
# 前置:8024 通(BAR-040 重启类考官之后跑,或独立跑)。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

need_device PIN-rehatch

deaths() {
    bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep '^session_deaths=' \
        | cut -d= -f2
}

before=$(deaths)
[ -n "${before:-}" ] || fail PIN-rehatch "stats 拉不到 session_deaths"

# ① 制造故障:合法入口,shell 正常退出
bash "$NA_ROOT/scripts/na-type.sh" 'exit\r' >/dev/null

# ② 等 death 计数反应(值守 300ms 消费 + 主循环抽干,10s 上限)
ok=""
for _ in $(seq 1 20); do
    sleep 0.5
    now=$(deaths) && [ -n "$now" ] || continue
    if [ "$now" -ge $((before + 1)) ]; then ok=1; break; fi
done
if [ -z "$ok" ]; then
    fail PIN-rehatch "exit 后 10s session_deaths 未涨($before 不变)——死亡事件丢了?"
fi

# ③ 等自动重孵横幅出现在画面上
banner=""
for _ in $(seq 1 16); do
    sleep 0.5
    text=$(bash "$NA_ROOT/scripts/na-text.sh" 2>/dev/null) || continue
    if echo "$text" | grep -q "已重连 = 新 shell"; then
        banner=1; break
    fi
done
[ -n "$banner" ] || fail PIN-rehatch "death 计数涨了($before→$now)但 8s 无重孵横幅——活跃方自动 respawn 失灵?"

# ④ 新 shell 回显活着(重孵是新的 sh,ls 执行成功即证输入输出链通)
bash "$NA_ROOT/scripts/na-type.sh" 'echo probe_$((40+2))\r' >/dev/null
alive=""
for _ in $(seq 1 10); do
    sleep 0.5
    text=$(bash "$NA_ROOT/scripts/na-text.sh" 2>/dev/null) || continue
    if echo "$text" | grep -q "probe_42"; then alive=1; break; fi
done
[ -n "$alive" ] || fail PIN-rehatch "重孵后新 shell 无回显——输入输出链没接上"
pass PIN-rehatch "exit 注入:deaths $before→$now,横幅出,新 shell 回显正常"
