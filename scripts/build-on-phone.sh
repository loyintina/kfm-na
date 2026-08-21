#!/usr/bin/env bash
# build-on-phone.sh — 手机编译回路（2026-08-21 用户定案，state.md「构建流程」）：
#   服务器 = 代码事实来源 + 出题判卷（pre-commit chain 全绿才算数）；
#   手机   = 拉绿了的 master → 本地编 APK → 本地调安装器。
# 为什么：APK 带 bootstrap 资产后 37M，每趟 scp 回传太贵；源码 diff 走隧道秒级，
# 且编译负载挪出服务器（多 agent 共线时服务器会卡）。
#
# 前提（一次性铺设，已做）：
#   1. 服务器 kfm-na 仓有 phone remote：
#      ssh://localhost:8022/data/data/com.termux/files/home/kfm-na
#   2. 手机侧仓库 receive.denyCurrentBranch=updateInstead（push 直更工作树）
#   3. bootstrap 资产已同步到手机 ~/kfm-na-toolchain/bootstrap-aarch64.zip
#      （package-apk.sh 的 $HOME 候选路径命中；bootstrap 重编后要重同步）
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== [phone-build 1/2] push master → 手机 ==="
git push phone master

echo "=== [phone-build 2/2] 手机本地打包 + 调起安装器 ==="
ssh -p 8022 -o BatchMode=yes -o ConnectTimeout=8 localhost \
    "cd /data/data/com.termux/files/home/kfm-na && bash scripts/deploy-phone.sh --build"
