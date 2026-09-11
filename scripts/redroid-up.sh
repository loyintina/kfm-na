#!/bin/bash
# redroid-up.sh — 云安卓(redroid)一键起场(2026-09-11)
#
# 用途:服务器上的模拟安卓环境——永远前台、永不熄屏、仪器全开,
# 补真机后台测试受限(vivo 后台拉起限制/进程回收)的短板。
# 定位:自动化回归 + 帧级仪器常驻;C 档手感判卷仍归真机,
# swiftshader 下帧率/性能数据失真,不作判卷依据。
#
# 本脚本幂等:已起则体检通过即退;重启后 binder 内核模块与
# binderfs 挂载会丢,这里一并补齐。
#
# 用法:
#   bash scripts/redroid-up.sh           # 起容器 + adb 打通 + 体检
#   bash scripts/redroid-up.sh --recreate # 销毁重建容器(数据清盘)
set -euo pipefail

ADB=/root/kfm-na-toolchain/sdk/platform-tools/adb
IMAGE=redroid/redroid:12.0.0_64only-latest
NAME=redroid12
SERIAL=localhost:5555

die() { echo "[redroid-up] ❌ $*" >&2; exit 1; }
ok() { echo "[redroid-up] $*"; }

# ① 内核 binder(重启后需重载)
if ! grep -qw binder /proc/filesystems; then
    modprobe binder_linux || die "modprobe binder_linux 失败(内核不支持?)"
fi
if [[ ! -e /dev/binderfs/binder-control ]]; then
    mkdir -p /dev/binderfs
    mount -t binder binder /dev/binderfs || die "挂载 binderfs 失败"
fi
ok "binder 就绪"

# ② 容器
if [[ "${1:-}" == "--recreate" ]]; then
    docker rm -f "$NAME" >/dev/null 2>&1 || true
fi
if ! docker ps --format '{{.Names}}' | grep -qx "$NAME"; then
    if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
        docker start "$NAME" >/dev/null
    else
        docker run -itd --name "$NAME" --privileged \
            -v /dev/binderfs:/dev/binderfs -p 5555:5555 \
            "$IMAGE" androidboot.redroid_gpu_mode=guest >/dev/null
    fi
    ok "容器 $NAME 已起"
else
    ok "容器 $NAME 在跑"
fi

# ③ adb 打通 + 等开机
"$ADB" connect "$SERIAL" >/dev/null
"$ADB" -s "$SERIAL" wait-for-device
for _ in $(seq 1 60); do
    [[ $("$ADB" -s "$SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r') == "1" ]] && break
    sleep 2
done
[[ $("$ADB" -s "$SERIAL" shell getprop sys.boot_completed | tr -d '\r') == "1" ]] \
    || die "60×2s 内没等到 boot_completed"
ok "Android 已开机($SERIAL)"

# ④ adbd 提 root(读应用沙箱闸门目录的前置)
[[ $("$ADB" -s "$SERIAL" shell id -u | tr -d '\r') == "0" ]] || {
    "$ADB" -s "$SERIAL" root >/dev/null; sleep 1
    "$ADB" -s "$SERIAL" wait-for-device
}
[[ $("$ADB" -s "$SERIAL" shell id -u | tr -d '\r') == "0" ]] || die "adbd root 失败"
ok "adbd root 就位——闸门目录可直读直写"

# ④.5 报表通道接力（2026-09-12）：adb reverse 数据面已死（adb 37.0.1 ↔
# redroid12 adbd，注册成功但数据永不转发），改走 nc 接力：
#   容器 127.0.0.1:8021 →(loop-relay)→ 172.18.0.1:8021 →(report-relay)→
#   host 127.0.0.1:8021 kfmv4。两臂都是幂等保活：host 侧靠 pgrep 查活，
# 容器侧重推脚本 + 查监听。
if ! pgrep -f 'redroid-report-relay.py' >/dev/null; then
    nohup /root/.venvs/font/bin/python \
        "$(dirname "$0")/redroid-report-relay.py" \
        >/tmp/redroid-relay.log 2>&1 &
    ok "host 报表中继已拉起（/tmp/redroid-relay.log）"
else
    ok "host 报表中继在跑"
fi
"$ADB" -s "$SERIAL" push "$(dirname "$0")/redroid-loop-relay.sh" \
    /data/local/tmp/redroid-loop-relay.sh >/dev/null
# 容器内监听探活：无 0100007F:1F55 LISTEN 则拉起（sh 直读，/data/local/tmp
# noexec 不能直接 exec 脚本）
if ! "$ADB" -s "$SERIAL" shell 'su 0 cat /proc/net/tcp' | grep -q '0100007F:1F55 .* 0A '; then
    "$ADB" -s "$SERIAL" shell 'su 0 sh -c "nohup sh /data/local/tmp/redroid-loop-relay.sh >/data/local/tmp/relay8021.log 2>&1 </dev/null &"'
    sleep 1
    "$ADB" -s "$SERIAL" shell 'su 0 cat /proc/net/tcp' | grep -q '0100007F:1F55 .* 0A ' \
        || die "容器内报表接力起不来（127.0.0.1:8021 无监听）"
    ok "容器报表接力已拉起"
else
    ok "容器报表接力在跑"
fi

ok "✅ 云安卓在场:$SERIAL($("$ADB" -s "$SERIAL" shell getprop ro.product.model | tr -d '\r'))"
