#!/bin/bash
# test-na-type-bytes.sh — na-type.sh 字节语义判卷(2026-08-24 实拍案:
# printf '%s' 把 \r 当字面「反斜杠+r」两字符发出,四条注入命令全堆在
# 提示符上零执行——链路四环全活,病在最后一厘米的编码)
#
# 判法:PATH 里注假 ssh(stdin 落盘,不理远端命令),跑真脚本,断言落盘
# 字节里 \r 翻成了真 CR(0x0d),且不含字面 "\r"(0x5c 0x72)。
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/ssh" <<'EOF'
#!/bin/bash
# 假 ssh:把 stdin 落盘,参数(远端命令)全忽略
cat > "$FAKE_SSH_OUT"
EOF
chmod +x "$tmp/ssh"

export FAKE_SSH_OUT="$tmp/payload"
PATH="$tmp:$PATH" bash "$here/na-type.sh" 'inject-me\r' >/dev/null

hex=$(od -An -tx1 "$tmp/payload" | tr -d ' \n')
fail() { echo "❌ $1(落盘 hex: $hex)"; exit 1; }

case "$hex" in
    *0d) : ;;  # 末尾必须是真 CR
    *) fail "\\r 没翻成 CR(0x0d)" ;;
esac
case "$hex" in
    *5c72*) fail "含字面反斜杠+r(5c 72)——'%s' 旧病复发" ;;
    *) : ;;
esac

echo "✅ na-type 字节语义:\\r → 真 CR,无字面残留"
