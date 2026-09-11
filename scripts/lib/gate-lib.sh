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
# 默认 ssh = 真机 8024。串口用 NA_ADB_SERIAL 覆盖(默认 localhost:5555)。

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
NA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

NA_TRANSPORT=${NA_TRANSPORT:-ssh}
NA_ADB=${NA_ADB:-/root/kfm-na-toolchain/sdk/platform-tools/adb}
NA_ADB_SERIAL=${NA_ADB_SERIAL:-localhost:5555}

gate() {
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" shell "$1"
    else
        ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
            -o StrictHostKeyChecking=no localhost "$1"
    fi
}

gate_pull() {
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" pull "$1" "$2" >/dev/null
    else
        scp -P 8024 -i "$NA_KEY" -o BatchMode=yes -o StrictHostKeyChecking=no \
            "localhost:$1" "$2" >/dev/null
    fi
}

gate_am_start() {
    if [[ $NA_TRANSPORT == adb ]]; then
        "$NA_ADB" -s "$NA_ADB_SERIAL" shell \
            "am start -n dev.kfm.na/.MainActivity" >/dev/null
    else
        ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 \
            -o StrictHostKeyChecking=no localhost \
            "am start -n dev.kfm.na/.MainActivity" >/dev/null
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
