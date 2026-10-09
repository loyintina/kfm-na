#!/bin/bash
# na-type.sh — 远程键盘(2026-08-24,配套 src/gate.rs keys-in 通道)
#
#   bash scripts/na-type.sh main 'ls -la\r'   把字节注入活跃会话——**第一参数
#   bash scripts/na-type.sh kimi-白露 '你好'   是目标会话名**（BAR-238 归属校验）
#
# BAR-238（MAIN0134 误投事故：裸字节盲发活跃会话，承影救机命令进了白露
# 会话）：载荷首行必须 `>@<目标会话名>`——消费端与活跃腿当前附着名同锁
# 比对，不符/无头一律拒注并报账（拒注账带时刻/目标/当前活跃/字节数，
# 报表可查）。**旧的一参形态已废**（fail-closed 拒）。目标名怎么知道：
# 你读屏那一刻 tmux 状态栏上的会话名（na-text.sh 读屏核对同一名）。
#
# 字节语义:\r = 回车;\x03 = Ctrl+C;Ctrl 组合直接写控制字节。
# 注意:Ctrl-](\x1d)会话切换是 UI 层拦截不落 PTY,本通道注不进——
# 切换走通道九 switch-req(touch $NA_TMP/switch-req,gate.rs 通道九,
# 2026-09-20 真机探针实证可用)。
# 协议:先写 keys-in.new 再 mv(原子防半读);na 值守线程 300ms 内消费。
# 应用退后台也能注入(BAR-029 保活 + 值守线程不归事件循环管)。
set -euo pipefail

source "$(dirname "$0")/lib/gate-lib.sh"

if [ $# -ne 2 ]; then
    echo "用法: bash scripts/na-type.sh <目标会话名> '命令字节串(\r 结尾=回车)'" >&2
    echo "  BAR-238 起第一参数必填目标会话名（归属校验，不符即拒注）；" >&2
    echo "  会话名以读屏时 tmux 状态栏为准（na-text.sh 可核对）" >&2
    exit 64
fi

# 载荷 = 目标头 + 裸字节。先落临时文件再交闸门:/dev/stdin 直管在 HTTP
# 腿失败退 ssh 老路时会被 curl 吃空、且 -s 判管道恒假导致载荷丢失
# (2026-10-07 判卷实测)。printf '%b':把 \r \x03 等转义翻成真字节——'%s'
# 会当字面两字符发出去(2026-08-24 实拍:四条注入命令带字面 "\r" 全堆在提示符上)
pf=$(mktemp)
trap 'rm -f "$pf"' EXIT
{ printf '>@%s\n' "$1"; printf '%b' "$2"; } > "$pf"
gate_touch keys-in "$pf"   # BAR-233
n=$(printf '%b' "$2" | wc -c)
echo "✅ 已注入 ${n} 字节 → 目标 $1（归属不符会被拒注，gate 报表可查；300ms 内落地;na-text.sh 可读屏核对）"
