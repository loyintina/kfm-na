#!/bin/bash
# gate-lib.sh — 真机考官共享库(2026-08-27,真机回归套件配套)
#
# 用法: accept 脚本开头 `source "$(dirname "$0")/../lib/gate-lib.sh"`
# (caller 先 set -uo pipefail,本库不 set -e——考官要自己控制 exit 码)
#
# 提供:
#   NA_KEY / NA_TMP       闸门连接常量(与各 na-*.sh 同源)
#   gate "<cmd>"          闸门沙箱执行(stdin 透传,两种传输同协议)
#   gate_pull <远> <本>   沙箱文件拉回本地
#   gate_am_start         拉起 app 前台
#   pass "<BAR>" "<证据>" 打印 ✅ + exit 0
#   fail "<BAR>" "<证据>" 打印 ❌ + exit 1
#   need_device "<BAR>"   设备不可达时打印 ⏭ + exit 77(runner 据此记跳过)
#
# 传输层(2026-09-11 redroid 云安卓接线):NA_TRANSPORT=adb 时闸门走
# adbd root 直读沙箱——云安卓没有 app 内 sshd(overlay 是 aarch64 核),
# 也不需要隧道,文件触发协议(.new→mv 原子写/轮询应答)一字不变。
# ssh 路 = na-ssh.sh 统一入口(2026-09-23 起 9022 首选/8024 备援,不再写死)。
# 串口用 NA_ADB_SERIAL 覆盖(默认 localhost:5555)。

NA_KEY=$HOME/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
NA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# shellcheck source=scripts/lib/na-ssh.sh
source "$(dirname "${BASH_SOURCE[0]}")/na-ssh.sh"

NA_TRANSPORT=${NA_TRANSPORT:-ssh}
NA_ADB=${NA_ADB:-$HOME/40-资产/kfm-na-toolchain/sdk/platform-tools/adb}
NA_ADB_SERIAL=${NA_ADB_SERIAL:-localhost:5555}

# BAR-248（NA0177）：Termux 腿 8022 是「会漂的编号」——neo6s Termux →
# 11 机 → 9 机三易其主（谁反连谁应答），无核验的 am start 曾误拉 9 机
# 旧核引爆互踢风暴。用编号前先问「你是谁」，答非所期即拒。默认期待
# 11 机（主生产机）；判卷/他机显式 NA_TERMUX_MODEL=<机型> 覆盖。
NA_TERMUX_MODEL=${NA_TERMUX_MODEL:-V2573A}

gate() {
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" shell "$1"
    else
        na_ssh "$1"
    fi
}

# ---- BAR-233（乙案 v1）：闸门触发腿走数据面（HTTP→na-server→QUIC 长轮询）
# 触发下发与结果上行都不再依赖 sshd；HTTP 不通（老核/数据面断）自动退
# ssh 老路——兼容未升级核。服务器本地直连 na-server（127.0.0.1）。
na_gate_http() {
    curl -m 4 -s -o /dev/null -w '%{http_code}'         -X POST --data-binary @-         "http://127.0.0.1:${NA_HTTP_PORT:-9021}/api/gate/$1"
}

# gate_touch <name> [payload-file]：写触发（payload 可空）。HTTP 2xx =
# 已入队（na 侧长轮询秒级取走落盘）；不通退 ssh/adb 老路。
gate_touch() {
    local name=$1 pf=${2:-/dev/null}
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" shell "cat > $NA_TMP/$name.new && mv $NA_TMP/$name.new $NA_TMP/$name" < "$pf"
        return $?
    fi
    local code
    code=$(na_gate_http "$name" < "$pf") || code=000
    if [[ $code == 2* ]]; then
        return 0
    fi
    # 退老路：payload 进文件再改名（.new→mv 原子语义）；空 payload = touch
    if [[ -s "$pf" ]]; then
        na_ssh "cat > $NA_TMP/$name.new && mv $NA_TMP/$name.new $NA_TMP/$name" < "$pf"
    else
        na_ssh "touch $NA_TMP/$name"
    fi
}

# gate_result <name> [outfile]：取结果（HTTP 一次性取走；404/不通退
# cat 老路）。outfile 给了落文件，没给 stdout。
gate_result() {
    local name=$1 out=${2:-}
    if [[ $NA_TRANSPORT != adb ]]; then
        if [[ -n $out ]]; then
            local code
            code=$(curl -m 30 -s -o "$out" -w '%{http_code}' \
                "http://127.0.0.1:${NA_HTTP_PORT:-9021}/api/gate/result/$name") || code=000
            [[ $code == 2* ]] && return 0
        else
            local body code
            body=$(curl -m 30 -s -w $'\n%{http_code}' \
                "http://127.0.0.1:${NA_HTTP_PORT:-9021}/api/gate/result/$name") || body=""
            code="${body##*$'\n'}"
            if [[ $code == 2* ]]; then
                printf '%s' "${body%$'\n'*}"
                return 0
            fi
        fi
    fi
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" shell "cat $NA_TMP/$name"
    else
        na_ssh "cat $NA_TMP/$name"
    fi
}

gate_pull() {
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" pull "$1" "$2" >/dev/null
    else
        na_scp_pull "$1" "$2" >/dev/null
    fi
}

# gate_termux_model <ssh口>：取对面机型；不通/取不到 = 空串
gate_termux_model() {
    ssh -p "$1" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost \
        'getprop ro.product.model 2>/dev/null | head -1' 2>/dev/null \
        | tr -d '\r' | head -1
}

# gate_termux_am <am start 参数>：Termux 腿发 am start，身份核验先行——
# 对面机型 ≠ NA_TERMUX_MODEL 即拒（响亮 stderr + exit 1），宁拒不错拉。
gate_termux_am() {
    local port=8022 model
    model=$(gate_termux_model "$port")
    if [[ $model != "$NA_TERMUX_MODEL" ]]; then
        echo "❌ BAR-248 身份核验拒：$port 对面机型「${model:-取不到}」≠ 期待「$NA_TERMUX_MODEL」——编号会漂（8022 三易其主），用 NA_TERMUX_MODEL=<机型> 显式指定目标机" >&2
        return 1
    fi
    ssh -p "$port" -o BatchMode=yes -o ConnectTimeout=8 \
        -o StrictHostKeyChecking=no localhost "am start $1" >/dev/null
}

gate_am_start() {
    if [[ $NA_TRANSPORT == adb ]]; then
        # adb 臂 -s 序列号即身份，不核
        "$NA_ADB" -s "$NA_ADB_SERIAL" shell \
            "am start -n dev.kfm.na/.MainActivity" >/dev/null
    else
        gate_termux_am "-n dev.kfm.na/.MainActivity"
    fi
}

pass() {
    echo "✅ $1 | $2"
    exit 0
}

fail() {
    echo "❌ $1 | $2" >&2
    exit 1
}

need_device() {
    if ! gate "true" >/dev/null 2>&1; then
        echo "⏭ $1 | 设备不可达(传输=$NA_TRANSPORT 通道不通),跳过" >&2
        exit 77
    fi
}

# 前置探针(2026-09-11 redroid 接线):本卷依赖活会话时点名——
# stats 的 local_dead/remote_dead 字段是事实源(壳层 Opened/死亡同步)。
# 平台不适用(云安卓 local 起不来/remote 没服务端)→ 跳过,不许挂卷。
need_alive() {  # $1=BAR 名  $2=会话名(local|remote)
    local field="${2}_dead" v
    v=$(bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null | grep "^${field}=" | cut -d= -f2)
    if [[ $v != "false" ]]; then
        echo "⏭ $1 | ${2} 会话不可用(${field}=${v:-未知}),平台不适用,跳过" >&2
        exit 77
    fi
}

# 双会话全灭探针:终端无活内容可判的卷(滚动/读屏类)用
need_any_alive() {  # $1=BAR 名
    local s
    s=$(bash "$NA_ROOT/scripts/na-stats.sh" 2>/dev/null)
    if grep -q '^local_dead=true' <<<"$s" && grep -q '^remote_dead=true' <<<"$s"; then
        echo "⏭ $1 | 双会话全灭(local/remote 均不可用),无内容可判,跳过" >&2
        exit 77
    fi
}
