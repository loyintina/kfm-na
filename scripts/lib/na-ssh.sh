#!/bin/bash
# na-ssh.sh — 手机闸门传输统一入口（2026-09-23，8024→9022 迁移落地）
#
# 端口语义（服务器侧回环，三者都落到手机、但命不一样）：
#   9022 = na 自持隧道反连（na 看门狗自营重拉，BAR-126~133 全线；首选——
#          Termux 冻不冻都不看脸色，na 活着它就在）
#   8024 = 旧反连路（认 na_probe_key；9022 断时的备援）
#   8022 = Termux sshd（认默认私钥；am start/手机仓~/kfm-na/工具链专用，
#          不在本库范围——那些活只有 Termux 的 rootfs 干得了）
#
# 用法：source "$(dirname "$0")/lib/na-ssh.sh"（gate-lib.sh 已 source，直接用）
#   na_ssh "<cmd>"          闸门执行（BatchMode，失败非零）
#   na_scp_pull <远> <本>   拉回
#   na_scp_push <本> <远>   推去
#   na_gate_port            只解析端口（缓存于 NA_GATE_PORT，可环境变量覆盖）
#
# 解析：nc 探 9022 → 通则用之；不通退 8024；双不通返回非零（调用方按不可达处理）。

NA_KEY=${NA_KEY:-/root/.ssh/na_probe_key}

na_gate_port() {
    if [[ -z ${NA_GATE_PORT:-} ]]; then
        if nc -z -w2 127.0.0.1 9022 2>/dev/null; then
            NA_GATE_PORT=9022
        elif nc -z -w2 127.0.0.1 8024 2>/dev/null; then
            NA_GATE_PORT=8024
        else
            return 1
        fi
    fi
    printf '%s\n' "$NA_GATE_PORT"
}

na_ssh() {
    local port
    port=$(na_gate_port) || return 1
    ssh -p "$port" -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no localhost "$1"
}

na_scp_pull() {  # $1=远端路径 $2=本地路径
    local port
    port=$(na_gate_port) || return 1
    scp -P "$port" -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no "localhost:$1" "$2"
}

na_scp_push() {  # $1=本地路径 $2=远端路径
    local port
    port=$(na_gate_port) || return 1
    scp -P "$port" -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
        -o StrictHostKeyChecking=no "$1" "localhost:$2"
}
