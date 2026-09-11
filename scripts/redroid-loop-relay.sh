#!/system/bin/sh
# redroid-loop-relay.sh — 容器内 127.0.0.1:8021 → 172.18.0.1:8021 接力
# （2026-09-12，配套服务器侧 scripts/redroid-report-relay.py；
# adb reverse 数据面死后的兜底，病灶见 relay.py 头注释）
#
# 形态修订：fifo 环方案废弃（toybox 0.8.4 位置参数不绑地址 +
# 客户端快退竞态丢请求），实测可用形态 = toybox nc -L（inetd 式
# 每连接 fork）+ 子命令直挂上游 nc。-q 3：客户端 EOF 后留 3s 等
# 响应回流（NA try_post 要读 200，没 -q 上行可能截断）。
#
# 本脚本自身只是「拉起一次 -L 常驻监听」；-L 自身不退，无需 while 环。
exec toybox nc -L -s 127.0.0.1 -p 8021 toybox nc -q 3 -w 5 172.18.0.1 8021
