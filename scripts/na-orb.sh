#!/bin/bash
# na-orb.sh — AI 外显事件注入(2026-08-30,配套 src/gate.rs orb-inject 通道十,
# ai-presence 期 0 组件一,规格书 ai-presence.md §八 D9 驱动轨)
#
#   bash scripts/na-orb.sh 'tap'              # 点球:终端 ↔ AI 全屏往返
#   bash scripts/na-orb.sh 'drag 500 800'     # 拖球到 (500,800)(状态核钳制)
#   bash scripts/na-orb.sh 'run 3000'         # 假跑 3000ms(= 长按球的 debug 钩子)
#   bash scripts/na-orb.sh 'end'              # 结束运行(灯灭,浮层驻留后隐)
#   bash scripts/na-orb.sh 'dismiss'          # 甩掉浮层(本次运行不现)
#
# 每个参数 = 一行指令,按序执行。语法(src/gate.rs parse_orb_line 钉死,
# 考题 tests/ai_presence_spec.rs)。值守线程直调 AiPresenceState 服务方法
# (人走触摸、AI 走注入,同一状态核),处理后落 orb-inject-res 回执——
# 本脚本等回执并打印(应用条数 + 事后快照)。判卷配套:na-stats.sh 的
# ai_* 字段族(机器轨) + na-shot.sh 实拍(视觉轨)。
# 协议同 na-type:先写 .new 再 mv(原子防半读),值守线程 300ms 内消费。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

if [ $# -lt 1 ]; then
    echo "用法: bash scripts/na-orb.sh 'tap' ['drag 500 800' ...](每参数一行指令)" >&2
    exit 64
fi

script="$(printf '%s\n' "$@")"
gate "rm -f $NA_TMP/orb-inject-res" >/dev/null
printf '%s\n' "$script" | ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
    -o StrictHostKeyChecking=no localhost \
    "cat > $NA_TMP/orb-inject.new && mv $NA_TMP/orb-inject.new $NA_TMP/orb-inject"
ok=""
for _ in $(seq 1 30); do
    sleep 0.3
    if gate "test -f $NA_TMP/orb-inject-res"; then
        ok=1; break
    fi
done
if [ -z "$ok" ]; then
    echo "❌ 9 秒内没等到回执——值守线程活着吗?(na-ping.sh 先探)" >&2
    exit 1
fi
gate "cat $NA_TMP/orb-inject-res"
