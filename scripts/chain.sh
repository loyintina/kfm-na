#!/bin/bash
# chain.sh — KFM-NA 唯一检查入口（纪律第一档，2026-08-13 出生即有）
#
# fmt → clippy → android-check → test → build，任一红即中断。pre-commit 钩子挂本脚本，
# 保证每个提交都是绿的。kfmv4 的 chain:auto 有 51 步是复杂度长出来的结果，
# 本项目从 4 步开始长（2026-08-13 第 5 步：android 目标 check——cfg 盲区防烂）——新检查一律加在这里，禁止另起入口。
cd "$(dirname "$0")/.." || exit 1

echo "=== [chain 1/4] cargo fmt --check ==="
cargo fmt --check || { echo "❌ fmt 不过：跑 cargo fmt 后重试"; exit 1; }

echo "=== [chain 2/4] cargo clippy ==="
cargo clippy --all-targets -- -D warnings || { echo "❌ clippy 不过"; exit 1; }

echo "=== [chain 3/5] cargo check --target aarch64-linux-android ==="
# Android 代码 cfg 在宿主不可见（fmt/clippy/test 都跳过它）——不查就会烂在盲区
cargo check --target aarch64-linux-android || { echo "❌ Android 目标编译不过"; exit 1; }

echo "=== [chain 4/5] cargo test ==="
cargo test || { echo "❌ 测试不过"; exit 1; }

echo "=== [chain 5/5] cargo build ==="
cargo build || { echo "❌ 构建不过"; exit 1; }

echo "=== [chain] ✅ 全部通过 ==="
