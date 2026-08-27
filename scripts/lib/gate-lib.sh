#!/bin/bash
# gate-lib.sh — 真机考官共享库(2026-08-27,真机回归套件配套)
#
# 用法: accept 脚本开头 `source "$(dirname "$0")/../lib/gate-lib.sh"`
# (caller 先 set -uo pipefail,本库不 set -e——考官要自己控制 exit 码)
#
# 提供:
#   NA_KEY / NA_TMP       闸门连接常量(与各 na-*.sh 同源)
#   gate "<cmd>"          8024 沙箱执行
#   pass "<BAR>" "<证据>" 打印 ✅ + exit 0
#   fail "<BAR>" "<证据>" 打印 ❌ + exit 1
#   need_device "<BAR>"   手机不可达时打印 ⏭ + exit 77(runner 据此记跳过)

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
NA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
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
        echo "⏭ $1 | 手机不可达(8024 不通),跳过" >&2
        exit 77
    fi
}
