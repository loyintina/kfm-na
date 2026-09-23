#!/bin/bash
# chain-phone.sh — 手机端跑全量 chain（2026-09-23 起降级为可选工具，
# 不再是白天提交闸：闸已全天收回服务器本地，pre-commit 直接跑 chain.sh。
# 保留场景：想在手机工具链上双保险复跑、或验证手机端工具链健康度）。
#
# 用法：bash scripts/chain-phone.sh
# 流程：服务器暂存区全量 diff → 打补丁推到手机 apply → 手机跑全量
#       chain（双环境自适应）→ 绿了落 stamp（补丁哈希+时间）→ 手机
#       reset --hard 还原现场。（stamp 已不被 pre-commit 消费，
#       仅为双保险跑过的凭据。）
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
# --binary：字体等二进制资产变更必须带 literal 增量——裸 diff 只有
# 「Binary files differ」一行，手机端 git apply 必红（2026-09-19
# 月亮字体补丁实踩：cannot apply binary patch without full index line）
git diff --cached --binary HEAD > "$PATCH"
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
# 空补丁直跑基线（2026-09-18 夜班修挂账：docs-only 或无代码变更时
# 0 行补丁 git apply 必红——补丁无 diff 头视为空，跳 apply 跑基线）
if grep -q '^diff ' /data/data/com.termux/files/home/kfm-na-day.patch 2>/dev/null; then
    git apply --index /data/data/com.termux/files/home/kfm-na-day.patch || exit 1
else
    echo "[chain-phone] 空补丁——跳 apply 直跑基线 chain"
fi
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
