#!/bin/bash
# BAR-123-accept.sh — 考官:键盘弹起态快捷键行抬手命中（2026-09-21）
#
# 命案:BAR-119 摘解析页 inset 链时误把抬手 hit 的 chrome_inset 摘掉
# （只剩输入栏高 220）——Started in_bar/渲染吃 chrome_inset+220，按下认、
# 抬手丢。用户真机 inset=890 连点 CTRL 8+ 次全落空（渲染 CTRL 中心
# 1630 = 用户实际点 1624~1651，眼没看错，是抬手尺错了）。
#
# 判卷法（全程遥测自校正，不写死 inset）:
#   ①前台 + 右滑收回裸终端页（面板靠泊时键行被盖，命中路径不走键行）;
#   ②点终端区唤出软键盘，等新「键盘 inset 变化: Npx」(N>0);
#     键盘原已弹起（无新行）→ 取全日志最后 inset 当候选，探针会校正;
#   ③探针:候选 inset 对应的行带里点 ↑ 列（无害键）——
#     「快捷键行抬手 (x,y), inset=M」应答 = Started 尺活着 + 真 inset 到手
#     （抬手行印的是点按时刻壳层现值，候选陈了也会被纠回）;
#   ④按真 inset 算 CTRL 渲染中心（行带=屏底−inset−栏高220−行高240，
#     CTRL=row1 col1）点按 → 必须见「快捷键行点按: CTRL」+「修饰键粘滞位: 001」;
#   ⑤再点一次灭灯（粘滞位 000）收尾——不留粘滞修饰键给用户。
# 病灶形态下④必落空（抬手尺少 inset）——本卷就是那把尺的常驻判官。
#
# 节拍:redroid 报表链路（容器 nc 环→docker 网桥接力→kfmv4）实测滞后
# 40~60s，所有应答窗给 90s；手机链路（ssh 隧道直达）秒回，早见早走。
set -uo pipefail
source "$(dirname "$0")/../lib/gate-lib.sh"

BAR=BAR-123
LOG=/root/kfm-na/field-reports.log
BAR_H=220     # input_bar::HEIGHT_PX（单行；开考前输入栏必须空——空栏恒单行）
KEYBAR_H=240  # keybar::HEIGHT_PX（2 行 × 120）
WAIT=90       # 应答窗秒数（redroid 报表滞后实测 40~60s）

need_device $BAR

# ---- 工具 ----
new_lines() {  # $1=开考时的行号水位 → 那之后的新行
    tail -n +$(($1 + 1)) "$LOG" 2>/dev/null
}
wait_grep() {  # $1=水位 $2=模式 $3=秒上限(默认 WAIT) → 命中行
    local off=$1 pat=$2 n=$(( ${3:-$WAIT} * 2 )) i
    for i in $(seq 1 "$n"); do
        local hit
        hit=$(new_lines "$off" | grep -m1 -E "$pat" || true)
        [ -n "$hit" ] && { echo "$hit"; return 0; }
        sleep 0.5
    done
    return 1
}
win_dim() {  # 闸门截图协议拿「宽 高」（shot.dim 是壳层权威窗尺寸）
    gate "rm -f $NA_TMP/shot.rgb $NA_TMP/shot.dim; touch $NA_TMP/shot-req" >/dev/null
    for _ in $(seq 1 20); do
        sleep 0.5
        if gate "test -f $NA_TMP/shot.dim" >/dev/null 2>&1; then
            gate "cat $NA_TMP/shot.dim"; return 0
        fi
    done
    return 1
}

# ---- ①前台 + 裸终端页 ----
# 熄屏/锁屏态 am start 唤不起前台（Vivo 夜间实测）——判不了就不判
# （BAR-122 同哲学：环境账不记码上），重试窗口给到 ~20s 再跳过
fg=""
for _ in $(seq 1 5); do
    gate_am_start
    sleep 4
    if bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep -q '^foreground=true'; then
        fg=1; break
    fi
done
[ -n "$fg" ] || {
    echo "⏭ $BAR | am start 五轮仍非前台（熄屏/锁屏态唤不起，环境产物不判码），跳过" >&2
    exit 77
}

# 面板靠泊时键行被盖（命中路径根本不走键行）——右滑收回到裸终端页
for _ in 1 2 3; do
    top=$(bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep '^panel_top=' | cut -d= -f2)
    [ "$top" = "none" ] && break
    bash "$NA_ROOT/scripts/na-touch.sh" \
        'down 600 1280' 'move 900 1280' 'sleep 60' 'move 1100 1280' 'up 1150 1280' >/dev/null
    sleep 1.5
done
[ "$(bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep '^panel_top=' | cut -d= -f2)" = "none" ] \
    || fail $BAR "右滑三次仍收不回裸终端页（panel_top=$top）"

# ---- ②唤键盘拿候选 inset ----
off=$(wc -l < "$LOG")
bash "$NA_ROOT/scripts/na-touch.sh" 'tap 630 1200' >/dev/null \
    || fail $BAR "点终端区注入失败"
ime_line=$(wait_grep "$off" '键盘 inset 变化: [0-9]+px' || true)
if [ -n "$ime_line" ]; then
    inset=$(echo "$ime_line" | grep -oE '[0-9]+px' | grep -oE '[0-9]+')
    [ "$inset" -gt 0 ] 2>/dev/null || {
        # 点到的是收键盘（原已弹起）→ 再点一次唤回
        off=$(wc -l < "$LOG")
        bash "$NA_ROOT/scripts/na-touch.sh" 'tap 630 1200' >/dev/null
        ime_line=$(wait_grep "$off" '键盘 inset 变化: [1-9][0-9]*px' || true)
        [ -n "$ime_line" ] || fail $BAR "两次点终端区都没唤出键盘（inset 全 0）"
        inset=$(echo "$ime_line" | grep -oE '[0-9]+px' | grep -oE '[0-9]+')
    }
else
    # 键盘原已弹起（无变化行）→ 取全日志最后一条 inset 遥测当候选（探针校正）
    inset=$(grep -oE '键盘 inset 变化: [0-9]+px' "$LOG" | tail -1 | grep -oE '[0-9]+' || true)
    [ -n "${inset:-}" ] && [ "$inset" -gt 0 ] 2>/dev/null \
        || { echo "⏭ $BAR | inset 遥测缺失（键盘状态不可确知），平台不适用，跳过" >&2; exit 77; }
fi

read -r W H <<<"$(win_dim)" || fail $BAR "shot.dim 拿不到窗尺寸"
[ "${W:-0}" -gt 0 ] && [ "${H:-0}" -gt 0 ] || fail $BAR "窗尺寸异常: $W x $H"

# ---- ③探针:行带里点 ↑ 列（col4 row0，无害键）拿真 inset 应答 ----
probe_x=$(( W * 9 / 14 ))   # col4 中心 = (4+0.5)/7
up_line=""
for attempt in 1 2; do
    band_mid=$(( H - inset - BAR_H - KEYBAR_H / 2 ))
    off=$(wc -l < "$LOG")
    bash "$NA_ROOT/scripts/na-touch.sh" "tap $probe_x $band_mid" >/dev/null
    up_line=$(wait_grep "$off" '快捷键行抬手 \([0-9]+,[0-9]+\), inset=[0-9]+' || true)
    [ -n "$up_line" ] && break
    # 探针落空：候选 inset 陈了（键盘可能已被收）——再唤一次重来
    [ "$attempt" = 1 ] || continue
    off=$(wc -l < "$LOG")
    bash "$NA_ROOT/scripts/na-touch.sh" 'tap 630 1200' >/dev/null
    ime_line=$(wait_grep "$off" '键盘 inset 变化: [1-9][0-9]*px' 30 || true)
    [ -n "$ime_line" ] && inset=$(echo "$ime_line" | grep -oE '[0-9]+px' | grep -oE '[0-9]+')
done
[ -n "$up_line" ] || fail $BAR "探针两轮无抬手应答——Started 尺也病了？"
inset=$(echo "$up_line" | grep -oE 'inset=[0-9]+' | grep -oE '[0-9]+')
[ "$inset" -gt 0 ] || fail $BAR "探针应答 inset=0——键盘弹起态没造成，判卷前提不成立"

# ---- ④真 inset 算 CTRL 中心点按 ----
ctrl_x=$(( W * 3 / 14 ))                         # col1 中心 = (1+0.5)/7
ctrl_y=$(( H - inset - BAR_H - KEYBAR_H + 180 )) # row1 中心 = 带顶 + 1.5×120
off=$(wc -l < "$LOG")
bash "$NA_ROOT/scripts/na-touch.sh" "tap $ctrl_x $ctrl_y" >/dev/null
tap_line=$(wait_grep "$off" '快捷键行点按: CTRL' || true)
[ -n "$tap_line" ] || {
    miss=$(new_lines "$off" | grep -m1 '命中落空' || true)
    fail $BAR "键盘弹起态(inset=$inset)点 CTRL ($ctrl_x,$ctrl_y) 未命中——抬手尺病灶在。${miss:+遥测: $miss}"
}
wait_grep "$off" '修饰键粘滞位: 001' >/dev/null \
    || fail $BAR "CTRL 点按后粘滞位没点亮"

# ---- ⑤灭灯收尾 ----
off=$(wc -l < "$LOG")
bash "$NA_ROOT/scripts/na-touch.sh" "tap $ctrl_x $ctrl_y" >/dev/null
wait_grep "$off" '修饰键粘滞位: 000' >/dev/null \
    || fail $BAR "第二次点 CTRL 灭灯失败——粘滞位残留会污染用户下一次输入"

pass $BAR "键盘弹起态(inset=$inset) CTRL 点按/粘滞/灭灯全绿（窗 ${W}x${H}，遥测定罪同款判卷）"
