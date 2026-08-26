#!/bin/bash
# na-restart.sh — 体面重启 na(2026-08-26,热更新闭环的重启腿)
#
#   bash scripts/na-restart.sh
#
# 链路:闸门目录 touch restart-req → 值守线程 exit(0)(不经过事件循环,
# 挂起态也杀得死)→ 8024 断连 = 确认死 → Termux 侧 am start 拉回 →
# 等 field-reports.log 出现新 boot 行 → na-ping 判 alive。
#
# 两个已知边界(诚实版):
# - 进程被 ROM 冻结时值守线程不进片,触发文件没人看——拉回靠 BAR-037
#   重跑防御:am start 让旧进程二进 android_main,遗言 + exit(0) 让位,
#   本脚本见状补第二次 am start 起全新进程。
# - 熄屏/锁屏时 am start 可能被系统挡(到不了前台),那时会提示手动点图标。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
REPORTS=/root/kfm-na/field-reports.log

gate() {
    ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=4 \
        -o StrictHostKeyChecking=no localhost "$1"
}

termux() {
    ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=6 localhost "$1"
}

boot_count() {
    grep -ac 'android_main 进入' "$REPORTS" 2>/dev/null || echo 0
}

wait_dead() {  # $1=秒数上限;8024 探活失败 = 死透
    local i
    for i in $(seq 1 "$(( $1 * 2 ))"); do
        gate "true" >/dev/null 2>&1 || return 0
        sleep 0.5
    done
    return 1
}

pull_foreground() {
    termux 'am start -n dev.kfm.na/.MainActivity >/dev/null 2>&1'
}

BEFORE=$(boot_count)
echo "=== ① 触发 restart-req(重跑防御同步实证:此番若旧进程冻结,BAR-037 接) ==="
if ! gate "touch $NA_TMP/restart-req"; then
    echo "⚠️ 8024 不通——na 本就死着或隧道断,直接拉回" >&2
fi

echo "=== ② 等 8024 断连(确认旧进程死透) ==="
FROZEN=0
if wait_dead 10; then
    echo "    已断连"
else
    echo "    10 秒未断——进程可能被 ROM 冻结(值守线程看不到触发文件)"
    FROZEN=1
fi

echo "=== ③ am start 拉回 ==="
pull_foreground
if [ "$FROZEN" = 1 ]; then
    # 拉回即重跑:BAR-037 防御让位(exit 0)后再拉一次,起全新进程
    sleep 2
    pull_foreground
fi

echo "=== ④ 等新 boot 报告 ==="
ok=""
for _ in $(seq 1 60); do
    sleep 0.5
    if [ "$(boot_count)" -gt "$BEFORE" ]; then ok=1; break; fi
done
if [ -z "$ok" ]; then
    echo "❌ 30 秒没有新 boot 行——熄屏/锁屏可能挡住了 am start,请点一下桌面图标" >&2
    exit 1
fi
grep -a 'android_main 进入' "$REPORTS" | tail -1

echo "=== ⑤ ping 判卷 ==="
sleep 2
bash "$(dirname "$0")/na-ping.sh"
echo "✅ 重启闭环完成"
