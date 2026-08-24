#!/bin/bash
# na-type.sh — 远程键盘(2026-08-24,配套 src/gate.rs keys-in 通道)
#
#   bash scripts/na-type.sh 'ls -la\r'    把参数当裸字节注入 na 活跃会话
#   bash scripts/na-type.sh '你好'         中文也行(IME 落字到底也是字节流)
#
# 字节语义:\r = 回车;\x03 = Ctrl+C;Ctrl 组合直接写控制字节。
# 注意:Ctrl-](\x1d)会话切换是 UI 层逻辑,闸门不支持(有意留白)。
# 协议:先写 keys-in.new 再 mv(原子防半读);na 值守线程 300ms 内消费。
# 应用退后台也能注入(BAR-029 保活 + 值守线程不归事件循环管)。
set -euo pipefail

NA_KEY=/root/.ssh/na_probe_key
NA_TMP=/data/data/dev.kfm.na/files/usr/tmp

if [ $# -ne 1 ]; then
    echo "用法: bash scripts/na-type.sh '命令字节串(\r 结尾=回车)'" >&2
    exit 64
fi

# 经 stdin 过 SSH 写字节(防引号/转义在两条 shell 间走样),远端 cat 落盘再 mv
printf '%s' "$1" | ssh -p 8024 -i "$NA_KEY" -o BatchMode=yes -o ConnectTimeout=6 \
    -o StrictHostKeyChecking=no localhost \
    "cat > $NA_TMP/keys-in.new && mv $NA_TMP/keys-in.new $NA_TMP/keys-in"
echo "✅ 已注入 ${#$1} 字节(300ms 内落地;na-text.sh 可读屏核对)"
