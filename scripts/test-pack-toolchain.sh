#!/bin/bash
# test-pack-toolchain.sh — BAR-136 pack-toolchain 冻结韧性契约钉（2026-09-23）
# 三跑命案：8022 长 tar 流被 vivo 冻结掐死（24MB/31MB 残桩）。契约四条：
# ①手机侧落盘且远端产物路径绝对写死（$HOME 服务器侧展开成 /root 命案）
# ②rsync --append-verify 断点续拉 ③sha256 对账才算成 ④files-only 清单
# （dpkg -L 连目录列，tar 见目录递归 /data 全树 9.4GB 事故）
# 反模式一条：ssh 长流直接管给 zstd（冻结掐流残桩的根源形态）。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
S=scripts/pack-toolchain.sh
fail() { echo "❌ pack-toolchain 契约破：$1"; exit 1; }
grep -q 'rsync --partial --append-verify' "$S" || fail "断点续传被摘"
grep -q 'sha256sum' "$S" || fail "sha256 对账被摘"
grep -q '\-f "\\$f"' "$S" || fail "files-only 清单被摘（目录递归事故回魂）"
grep -q 'REMOTE_PACK=/data/data/com.termux/files/home' "$S" \
    || fail "远端产物路径必须是绝对路径（\$HOME 服务器侧展开命案回魂）"
grep -q "REMOTE' | zstd" "$S" && fail "长流直压反模式回魂"
echo "✅ pack-toolchain 契约钉过"
