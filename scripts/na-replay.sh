#!/bin/bash
# na-replay.sh — 拉回飞行记录仪 + host 回放末屏(2026-08-24 自观测)
#
#   bash scripts/na-replay.sh          # 回放 local 会话末屏
#   bash scripts/na-replay.sh remote   # 回放 remote 会话末屏
#
# 原理:na 侧 flight-rec.bin(输出流+尺寸事件+时间戳)→ scp 拉回 →
# src/bin/na-replay.rs 喂进 host 侧同一台 TermView 复现。
# 前提:na 隧道活着(na-ssh.sh:9022 首选/8024 备援),探针钥匙 /root/.ssh/na_probe_key。
set -euo pipefail

NA_TMP=/data/data/dev.kfm.na/files/usr/tmp
HERE="$(cd "$(dirname "$0")/.." && pwd)"

# shellcheck source=scripts/lib/na-ssh.sh
source "$(dirname "$0")/lib/na-ssh.sh"

na_scp_pull "$NA_TMP/flight-rec.bin" /tmp/flight-rec.bin
cargo run --quiet --manifest-path "$HERE/Cargo.toml" --bin na-replay -- \
    /tmp/flight-rec.bin "${1:-local}"
