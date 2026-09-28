#!/bin/bash
# mailbox.sh — na 信箱（/root/.kfm/session/信箱）执法闸（2026-09-29 立，
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

MB="${KFM_NA_MAILBOX:-/root/.kfm/session/信箱}"
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

if [ -f "$MB/README.md" ] && grep -q 'gen:agent-inbox:start' "$MB/README.md"; then
    "$BIN" gen --mailbox "$MB" --check-only || { echo "❌ na 信箱台账投影漂移"; exit 1; }
else
    echo "[mailbox.sh] README 缺 gen 标记区段——gen --check-only 跳过（标记段就位后自动生效）"
fi
