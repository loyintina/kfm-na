#!/bin/bash
# test-quic-migration.sh — M2 迁移考题（docs/active/quic隧道.md §六）
#
# 场景：客户端在 nsA，经 nsR 的 NAT 出网到宿主服务器。中途把 nsR 的
# conntrack 清空 + snat 源地址换掉（= 运营商掐掉旧 NAT 映射并发新地址）。
#   判卷：QUIC 客户端无损跑完全部 ping-pong（连接迁移，无重连）；
#         TCP 反例对照必死（证明环境真的在模拟断链，不是虚设）。
# 需要 root + iproute2 + nftables + conntrack。重 IO？否——百 KB 流量级。
# 不进 chain 必修闸（CI 无 root/netns 环境），按需 + 01:43 夜间跑。

set -euo pipefail

BIN=${BIN:-/root/kfm-na/target/debug/examples/quic_echo}
NS_A=naq-a
NS_R=naq-r
PORT=9943
N=${N:-60}         # ping-pong 轮数（×100ms）
SWAP_AT=${SWAP_AT:-2}  # 第几秒后换 NAT 映射
LOG_A=$(mktemp /tmp/naq-client.XXXXXX.log)
LOG_T=$(mktemp /tmp/naq-tcpclient.XXXXXX.log)

cleanup() {
    ip netns del $NS_A 2>/dev/null || true
    ip netns del $NS_R 2>/dev/null || true
    ip link del naq-h0 2>/dev/null || true
    pkill -f "quic_echo server 10.2.0.1" 2>/dev/null || true
    pkill -f "quic_echo tcpserver 10.2.0.1" 2>/dev/null || true
}
trap cleanup EXIT
cleanup 2>/dev/null || true

# ---- 拓扑：nsA ─veth─ nsR ─veth─ host ----
# nsA: 10.1.0.2/24, 默认路由经 10.1.0.1
# nsR: 10.1.0.1/24 + 10.2.0.10/24（NAT 源一）→ 换映射时切 10.2.0.11
# host: 10.2.0.1/24（服务器）
ip netns add $NS_A
ip netns add $NS_R

ip link add naq-a0 type veth peer name naq-r0
ip link set naq-a0 netns $NS_A
ip link set naq-r0 netns $NS_R

ip link add naq-h0 type veth peer name naq-r1
ip link set naq-r1 netns $NS_R

ip netns exec $NS_A ip addr add 10.1.0.2/24 dev naq-a0
ip netns exec $NS_A ip link set naq-a0 up
ip netns exec $NS_A ip link set lo up
ip netns exec $NS_A ip route add default via 10.1.0.1

ip netns exec $NS_R ip addr add 10.1.0.1/24 dev naq-r0
ip netns exec $NS_R ip addr add 10.2.0.10/24 dev naq-r1
ip netns exec $NS_R ip link set naq-r0 up
ip netns exec $NS_R ip link set naq-r1 up
ip netns exec $NS_R ip link set lo up
ip netns exec $NS_R sysctl -q net.ipv4.ip_forward=1
# NAT：snat 固定源（masquerade 会跟着地址走，snat 才能显式换源）
# （iptables：本机内核 nft 在新 netns 里建 nat 链报 ENOENT，iptables 正常）
ip netns exec $NS_R iptables -t nat -A POSTROUTING -o naq-r1 -j SNAT --to-source 10.2.0.10

ip addr add 10.2.0.1/24 dev naq-h0
ip link set naq-h0 up
# 回程路由：10.1.0.0/24 经 nsR（SNAT 后其实用不到，保守起见加上）
ip netns exec $NS_R ip route add default via 10.2.0.1 dev naq-r1
ip route add 10.1.0.0/24 via 10.2.0.10 dev naq-h0 2>/dev/null || true

# ---- 服务起在宿主 ----
"$BIN" server 10.2.0.1:$PORT >/dev/null 2>&1 &
"$BIN" tcpserver 10.2.0.1:$((PORT+1)) >/dev/null 2>&1 &
sleep 1

# ---- 正考：QUIC 迁移 ----
ip netns exec $NS_A "$BIN" client 10.2.0.1:$PORT $N >"$LOG_A" 2>&1 &
CPID=$!
sleep "$SWAP_AT"

# 换映射：清 conntrack（旧映射死）+ snat 源 10.2.0.10 → 10.2.0.11
ip netns exec $NS_R iptables -t nat -F POSTROUTING
ip netns exec $NS_R ip addr add 10.2.0.11/24 dev naq-r1
ip netns exec $NS_R iptables -t nat -A POSTROUTING -o naq-r1 -j SNAT --to-source 10.2.0.11
ip netns exec $NS_R conntrack -F

QUIC_OK=0
if wait "$CPID"; then
    LINES=$(grep -c "^echo " "$LOG_A" || true)
    if [ "$LINES" -eq "$N" ]; then
        echo "✅ QUIC 迁移过：NAT 换映射后 $N/$N 行全回还，零重连"
        QUIC_OK=1
    else
        echo "❌ QUIC 迁移失败：只回了 $LINES/$N 行"
        tail -5 "$LOG_A"
    fi
else
    echo "❌ QUIC 客户端进程死亡（连接没迁过去）"
    tail -5 "$LOG_A"
fi

# ---- 反例对照：TCP 必死 ----
TCPPORT=$((PORT+1))
ip netns exec $NS_A "$BIN" tcpclient 10.2.0.1:$TCPPORT $N >"$LOG_T" 2>&1 &
TPID=$!
sleep "$SWAP_AT"
ip netns exec $NS_R iptables -t nat -F POSTROUTING
ip netns exec $NS_R iptables -t nat -A POSTROUTING -o naq-r1 -j SNAT --to-source 10.2.0.10
ip netns exec $NS_R conntrack -F

TCP_DIED=0
if ! wait "$TPID"; then
    TCP_DIED=1
    echo "✅ TCP 反例过：同一操作下 TCP 客户端死亡（环境模拟有效）"
else
    LINES=$(grep -c "^echo " "$LOG_T" || true)
    echo "❌ TCP 反例未死（回了 $LINES 行）——测试环境没模拟出断链，考题虚设"
fi

if [ "$QUIC_OK" = 1 ] && [ "$TCP_DIED" = 1 ]; then
    echo "=== M2 迁移考题 PASS ==="
    exit 0
fi
echo "=== M2 迁移考题 FAIL ==="
exit 1
