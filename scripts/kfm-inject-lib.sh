#!/bin/bash
# kfm-inject-lib.sh — 塞话键序唯一源（BAR-252／MAIN0175 楼23 白露拍板）
#
# 本文件**只有一份键序声明** KFM_INJECT_STEPS（空白分隔）：
#   esc / enter / m-enter = tmux 按键名；text = 字面文本（send-keys -l）；
#   sleepN               = 停顿 N 秒（可带小数，如 sleep0.3）。
#
# 消费者（只准读，不许另立第二份）：
#   ① crates/na-server/src/deliver.rs —— include_str! 编译期读入后纯函数解析
#   ② /root/10-项目/kfmv4/scripts/agent-send.sh —— source 本文件后按声明执行
# 改键序只改这里；两处消费者同票验收。
#
# 来源：kfmv4 agent-send.sh v12 长期实测键序——Escape → 0.3s → 字面文本 →
# 0.3s → Enter → 1s → Enter。用户 2026-10-17 真机实测：na deliver 只把
# 消息送进屏幕、没提交，病根是 na 与 agent-send 各写一套键序。

KFM_INJECT_STEPS="esc sleep0.3 text sleep0.3 enter sleep1 enter"
