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

ok "✅ 云安卓在场:$SERIAL($("$ADB" -s "$SERIAL" shell getprop ro.product.model | tr -d '\r'))"
