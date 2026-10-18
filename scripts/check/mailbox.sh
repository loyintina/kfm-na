#!/bin/bash
# mailbox.sh — na 信箱（$HOME/.kfm/session/信箱）执法闸（2026-09-29 立，
# kfmv4 JS 三件套 Rust 移植 mailbox-cli 落地，提案 0004）。
#
# 两道：
#   1. verify 全册执法（check-letter-token.mjs 主循环的 Rust 版）——信封/三池/
#      令牌台账/白话面/跨信判据，na 册词表：--name-prefix 'kfm-na|na' +
#      顶层 manifest-v1.json（v1 冻结名单）。
#   2. gen --check-only（README gen:pending/gen:agent-inbox 两区段 +
#      letters-index.jsonl 投影漂移检查）——README 缺 gen 标记区段时跳过
#      （标记段就位后自动生效）。
#
# 信箱目录不存在（手机 Termux/新环境无此册）= 跳过 exit 0，不挡链。
# MAILBOX_CLI_BIN 可指定预编译二进制（默认 cargo build -p mailbox-cli）。
set -u
cd "$(dirname "$0")/../.." || exit 1

MB="${KFM_NA_MAILBOX:-$HOME/.kfm/session/信箱}"
if [ ! -d "$MB" ]; then
    echo "[mailbox.sh] 信箱目录不存在（$MB）——跳过（手机/新环境无此册）"
    exit 0
fi

BIN="${MAILBOX_CLI_BIN:-}"
if [ -z "$BIN" ]; then
    cargo build -p mailbox-cli --quiet || { echo "❌ mailbox-cli 构建不过"; exit 1; }
    BIN=target/debug/mailbox-cli
fi

"$BIN" verify --mailbox "$MB" --name-prefix 'kfm-na|na' \
    --v1-manifest "$MB/manifest-v1.json" || { echo "❌ na 信箱全册执法不过"; exit 1; }

if [ -n "${KFM_CHAIN_FROM_HOOK:-}" ]; then
    # 2026-09-30 评审裁定：**派生投影检查不挂提交钩**。
    # 理由（实踩三次）：台账/索引是**全册属性**——谁新投一封信，投影就漂；
    # 于是"别人还没 gen"会挡住**你**的提交，而那个漂移不是你这笔的错，你也没法
    # 替全册负责（你 gen 完，下一个人投信又漂）。这是承重对象的错配。
    # → 分派：**提交钩只执法信本身（verify 全册，上面那道，它与并发无关）；
    #    派生投影检查归"手动跑全链/巡检"这一层**（跑 `bash scripts/chain.sh` 即含）。
    echo "[mailbox.sh] 提交钩上下文：跳过 gen --check-only（派生投影检查归全链/巡检；"
    echo "              理由见本文件此段注释——投影是全册属性，单个人负不了责）"
else
    if [ -f "$MB/README.md" ] && grep -q 'gen:agent-inbox:start' "$MB/README.md"; then
        # BAR-271（2026-10-18 清和 NA0197 楼1 判据②）：**对不上先自愈再报**。
        # 根因＝别线投一层楼，index 就过期，与本地这笔无关；直接红会把设施噪声
        # 当成我的错。自愈＝就地 gen 一次再复检；仍不过才红（真漂移/工具坏）。
        # 自愈会写 index（信箱仓留脏）——chain 不替人提交，调用方需随笔带上。
        if ! "$BIN" gen --mailbox "$MB" --check-only >/tmp/kfm-mailbox-check.$$ 2>&1; then
            echo "[mailbox.sh] 投影对不上——自愈：就地 gen 一次后复检（BAR-271）"
            if "$BIN" gen --mailbox "$MB" >/tmp/kfm-mailbox-gen.$$ 2>&1; then
                if "$BIN" gen --mailbox "$MB" --check-only >/dev/null 2>&1; then
                    echo "[mailbox.sh] ✅ 已自愈：索引已回写（信箱仓会留脏文件，请随本笔提交）"
                else
                    echo "[mailbox.sh] ❌ 自愈后仍漂移——看 /tmp/kfm-mailbox-gen.$$ 与 $MB/letters-index.jsonl"
                    tail -5 /tmp/kfm-mailbox-gen.$$ || true
                    exit 1
                fi
            else
                echo "[mailbox.sh] ❌ 自愈失败（gen 本身报错）——原红如下："
                cat /tmp/kfm-mailbox-check.$$ || true
                exit 1
            fi
        fi
    else
        echo "[mailbox.sh] README 缺 gen 标记区段——gen --check-only 跳过（标记段就位后自动生效）"
    fi
fi
