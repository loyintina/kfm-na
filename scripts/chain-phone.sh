#!/bin/bash
# chain-phone.sh — 白天 chain 闸在手机端跑（2026-09-01 用户拍板：
# 大负载任务白天只准手机端；服务器 chain 全量只准 01:00-07:00）。
#
# 用法：bash scripts/chain-phone.sh
# 流程：服务器暂存区全量 diff → 打补丁推到手机 apply → 手机跑全量
#       chain（双环境自适应）→ 绿了落 stamp（补丁哈希+时间）→ 手机
#       reset --hard 还原现场。pre-commit 白天校验 stamp（补丁哈希必须
#       与当前暂存区一致，6 小时有效）——改了代码就必须重跑，无侥幸。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

PHONE=${PHONE:-"u0_a376@localhost"}
PORT=${PORT:-8022}
PATCH=/tmp/kfm-na-day-chain.patch
STAMP=.git/chain-phone-stamp

git add -A
# stamp 只绑代码内容——docs 变更不作废 stamp(docs 耦合由另一闸管)
PATCH_HASH=$(git diff --cached HEAD -- . ':(exclude)docs' | md5sum | cut -d' ' -f1)
if [ -z "$PATCH_HASH" ]; then echo "❌ 暂存区为空"; exit 1; fi
git diff --cached HEAD > "$PATCH"
echo "[chain-phone] 补丁 $(wc -l < "$PATCH") 行 哈希 $PATCH_HASH"

ssh_rc=1
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12; do
    if ! scp -P "$PORT" -o StrictHostKeyChecking=no "$PATCH" \
        "$PHONE:/data/data/com.termux/files/home/kfm-na-day.patch"; then
        if [ $attempt -lt 12 ]; then
            echo "[chain-phone] 第${attempt}轮 scp 失败，60s 后重试(隧道自愈中)"
            sleep 60
            continue
        fi
        echo "❌ 补丁传输 4 轮全败"
        exit 1
    fi
    ssh -p "$PORT" -o StrictHostKeyChecking=no "$PHONE" bash -s <<'REMOTE'
set -uo pipefail
cd ~/kfm-na || exit 1
git reset --hard >/dev/null 2>&1
git apply --index /data/data/com.termux/files/home/kfm-na-day.patch || exit 1
ionice -c3 nice -n 10 bash scripts/chain.sh > chain-last.log 2>&1
RC=$?
tail -60 chain-last.log
git reset --hard >/dev/null 2>&1   # 还原现场(updateInstead 需要干净树)
echo "[chain-phone] 手机 chain 退出码 $RC"
exit $RC
REMOTE
    ssh_rc=$?
    [ $ssh_rc -eq 0 ] && break
    if [ $ssh_rc -eq 255 ] && [ $attempt -lt 12 ]; then
        echo "[chain-phone] 第${attempt}轮连接失败，60s 后重试(隧道自愈中)"
        sleep 60
    else
        break
    fi
done

if [ $ssh_rc -eq 0 ]; then
    printf '%s %s\n' "$PATCH_HASH" "$(date +%s)" > "$STAMP"
    echo "✅ 手机 chain 全绿，stamp 已落（6h 内提交有效；改码需重跑）"
else
    rm -f "$STAMP"
    echo "❌ 手机 chain 红，不落 stamp"
fi
exit $ssh_rc
