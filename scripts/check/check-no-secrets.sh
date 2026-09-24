#!/bin/bash
# check-no-secrets.sh — 密钥/证书防进库闸（BAR-143 形态升级：纯段落 → 代码守卫）
#
# 事故原型：NA_QUIC_CERT 缺省路径在仓内，git add -A 把 quic.key.der
# （TLS 私钥）+ quic.psk（客户端证）扫进提交推了双远端——处置 = amend
# + force-push + 双证轮转。教训编码：密钥类产物落盘前先问「git add -A
# 会不会吃掉它」——本闸机械执法：仓内永不许出现密钥/证书类文件。
#
# 判据 = git ls-files 文件名后缀匹配（白名单制比内容扫描快且零误报：
# 仓内本就不该有任何 .der/.psk/.key/.pem/.keystore/.jks/.p12/.pfx）。
# 合法例外（如 Android 官方公开 debug keystore）在下方白名单逐条登记——
# 登记即「我知道它是公开的」，不许整类放行。
set -u
cd "$(dirname "$0")/../.." || exit 1

# 白名单：全路径精确匹配（当前为空——仓内现无任何密钥类文件）
whitelist=''

hits=$(git ls-files | grep -iE '\.(der|psk|key|pem|keystore|jks|p12|pfx)$' || true)
if [ -n "$whitelist" ]; then
    hits=$(echo "$hits" | grep -vxF "$whitelist" || true)
fi
if [ -n "$hits" ]; then
    echo "❌ 密钥/证书类文件混入仓库（BAR-143：落盘路径必须在仓外）:"
    echo "$hits"
    echo "   处置：git rm --cached + 产物迁仓外 + .gitignore 双保险；"
    echo "   若已推送远端 = 按泄密处置——摘帽≠完事，密钥本体必须轮转。"
    exit 1
fi
exit 0
