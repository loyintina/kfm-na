#!/bin/bash
# na-anim-bench.sh — 池区动画帧数一键判卷（BAR-096/097 验收仪器，2026-09-16）
#
#   bash scripts/na-anim-bench.sh            # 真机（8024 闸门，默认）
#   NA_TRANSPORT=adb bash scripts/na-anim-bench.sh   # redroid（合成率物理极限，只判正确性）
#
# 用途：池区动画（视口平移/光标滑行/池高）拆层前后的**帧数**对比判卷。
# 前提：na 在前台——脚本先 am start -S 重拉并用 stats 的 foreground 自证；
# 后台（挂起休假）= 循环停跳，注入无效（凌晨熄屏实测踩过）。
# 输出：两轮（Page 切标签 / Upper 点下池）各给 panc 帧数 + panc 明细 +
# panel-anim 仪表（动画期逐帧记账，含推算 fps）。
# 基线（vc433，拆层前）：两轮 panc 均 3 帧（14MB 逐帧重烘把 250ms 动画
# 压到 ~21fps）；BAR-096 件一件二后 Page 轮（重烘源=标签栏小层 0.65MB）
# 应显著上升；Upper 轮仍受池高一路（14MB，BAR-097 待拆）约束。
set -uo pipefail
NA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$NA_ROOT/scripts/lib/gate-lib.sh"

gate_am_start
sleep 5
echo "== 设备状态（foreground=true 才有效）"
bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | head -3

# 环污染修法（2026-09-16 仪器病实锤）：trace 是 256 帽内存环，
# `rm trace.txt` 不清环——下次 trace-req 落盘仍是整环滚动副本，旧帧混进
# 判卷（曾把多次平移的累积行数当单次帧数，还造出时间戳矛盾假象）。
# 口径：判卷前先取水位线（环内最大 boot_ms），只数水位线之后的新行。
ring_watermark() {
    gate "touch $NA_TMP/trace-req"
    sleep 1.2
    gate "cat $NA_TMP/trace.txt" 2>/dev/null \
        | sed -n 's/^\[+\([0-9]*\)ms .*/\1/p' | sort -n | tail -1
}
# 只输出水位线之后的行（awk 数值比较，8 位零填充时间戳天然防八进制坑）
after_watermark() { # $1 = 水位线 ms
    awk -v wm="$1" 'match($0, /^\[\+[0-9]+ms/) {
        if (substr($0, 3, RLENGTH - 4) + 0 > wm) print
    }'
}

run_case() { # $1 = 用例名，其余 = na-touch 指令逐条
    local name="$1"
    shift
    local wm
    wm=$(ring_watermark)
    wm=${wm:-0}
    bash "$NA_ROOT/scripts/na-touch.sh" "$@" >/dev/null
    gate "touch $NA_TMP/trace-req"
    sleep 1.5
    local n
    n=$(gate "grep panc $NA_TMP/trace.txt" | after_watermark "$wm" | wc -l)
    echo "== $name：panc 帧数 = $n（水位线 +${wm}ms 之后的新行）"
    gate "grep panc $NA_TMP/trace.txt" | after_watermark "$wm" | head -14
    gate "grep panel-anim $NA_TMP/trace.txt" | after_watermark "$wm" | tail -2
}

# 设置齿轮 (1166,70) → 设置页；组件池标签 (334,87) → Page 平移；
# 下池第二行 (630,1211) → Upper 平移（坐标为 1260 宽真机实测值）
run_case "Page 平移（切标签）" 'tap 1166 70' 'sleep 900' 'tap 334 87' 'sleep 1300'
run_case "Upper 平移（点下池）" 'tap 630 1211' 'sleep 1300'
echo "== 判卷口径：帧数↑ 且 panc t 单调到 1.000；面板静止后（贴死）无残余动画"
