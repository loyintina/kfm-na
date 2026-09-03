#!/usr/bin/env bash
# deploy-ai-config.sh — 三路 key 配置推 na 私有目录（期 0③ 换脑配套）。
#
# 做什么：从服务器 kfmv4 的 ~/.kfmv4/providers.json 抽三张卡（Kimi 默认 /
# 智谱 coding 套餐 / DeepSeek 官网），从 ~/.kfmv4/.env 抽对应三条 key，
# 经 8024 调试闸门（na 自己的 sshd，写私有目录的天然权限）推进
# /data/data/dev.kfm.na/files/ai/。
#
# 纪律：key 绝不进 git（三仓两公开）——本脚本只读服务器本地配置、临时
# 目录即推即焚；na 侧这份是 na 的本地配置种子，之后归 na 自管（不是镜像、
# 不同步，D3 不违背）。模型选择器是未来活，默认脑路常量
# （Kimi/kimi-for-coding-highspeed）在 android_app.rs。
#
# 用法：bash scripts/deploy-ai-config.sh（手机 8024 闸不通时先 na 起来）

set -euo pipefail

SRC_PROVIDERS="$HOME/.kfmv4/providers.json"
SRC_ENV="$HOME/.kfmv4/.env"
NA_DIR="/data/data/dev.kfm.na/files/ai"
SSH="ssh -p 8024 -i /root/.ssh/na_probe_key -o BatchMode=yes -o StrictHostKeyChecking=no -o ConnectTimeout=8 localhost"

[ -r "$SRC_PROVIDERS" ] || { echo "❌ 缺 $SRC_PROVIDERS"; exit 1; }
[ -r "$SRC_ENV" ] || { echo "❌ 缺 $SRC_ENV"; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# 抽三卡；deepseek 卡补登记 vision-exp（2026-09-03 官网 /models 实查存在；
# 服务端本就不校验 models[] 白名单、直透传上游，登记只是账齐）
python3 - "$SRC_PROVIDERS" "$TMP/providers.json" << 'EOF'
import json, sys
src = json.load(open(sys.argv[1]))
providers = src if isinstance(src, list) else src.get("providers", [])
keep = {"Kimi", "智谱", "deepseek"}
out = []
for p in providers:
    if p.get("id") in keep:
        if p["id"] == "deepseek" and "deepseek-v4-flash-vision-exp" not in p.get("models", []):
            p.setdefault("models", []).append("deepseek-v4-flash-vision-exp")
        out.append(p)
found = {p["id"] for p in out}
assert found == keep, f"三卡没找齐: {found}"
json.dump(out, open(sys.argv[2], "w"), ensure_ascii=False, indent=2)
print(f"[ai-config] 三卡已抽: {sorted(found)}")
EOF

grep -E "^KFM_PROVIDER_(KIMI|ZHIPU|DEEPSEEK)=" "$SRC_ENV" > "$TMP/.env"
[ "$(wc -l < "$TMP/.env")" = 3 ] || { echo "❌ .env 三 key 没抽齐"; exit 1; }
chmod 600 "$TMP/.env"

# 8024 = na 自己的 sshd（写私有目录的天然权限）；scp 依赖对端有 scp  binary，
# bootstrap 里不一定有——cat 重定向万能
$SSH "mkdir -p $NA_DIR" || { echo "❌ 8024 闸不通——na 起来了吗？"; exit 1; }
$SSH "cat > $NA_DIR/providers.json" < "$TMP/providers.json"
$SSH "cat > $NA_DIR/.env && chmod 600 $NA_DIR/.env" < "$TMP/.env"

# 回读校验（形状，不验 key 值——key 不上日志）
$SSH "ls -la $NA_DIR && head -c 80 $NA_DIR/providers.json"
echo "✅ 已推 na 私有目录 $NA_DIR（Kimi 默认 / 智谱 / DeepSeek 官网；key 不落 git 不上日志）"
