#!/bin/bash
# test-relay-timeout.sh — BAR-109 钉入口（chain 第 9 步挂点）
# 行为核在 test-relay-timeout.py（socket 级静默存活判卷），本壳只选解释器。
set -euo pipefail
cd "$(dirname "$0")/.."
PY=$(command -v python3 || { echo "❌ python3 缺失——BAR-109 钉无解释器（两环境均已验证装有）" >&2; exit 1; })
"$PY" scripts/test-relay-timeout.py
