#!/usr/bin/env bash
# test-bar-new.sh — bar-new.sh 领号唯一源考题（BAR-189，0055 甲案三补丁，挂 chain）
# 五夹具全走 BAR_NEW_* 测试口 + NO_LETTER=1（不碰真账真信箱）：
#   ①跨树可见（185 场景复跑）②信箱不可读拒领 ③归档防回退 ④只认 H1 不认文件名
#   ⑤名册预检（名不在册拒领 / 在册放行）
set -uo pipefail
cd "$(dirname "$0")/../.."

BARNEW=scripts/bar-new.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
pass=0; fail=0
chk() { # chk <名> <期望exit> <实际exit> [期望输出含]
    if [ "$2" = "$3" ] && { [ -z "${4:-}" ] || grep -qF "$4" "$T/out"; }; then
        echo "  ✓ $1"; pass=$((pass+1));
    else echo "  ✗ $1（期望 exit=$2 实得 $3 输出:$(head -c 80 "$T/out" 2>/dev/null)）"; fail=$((fail+1)); fi
}

# 公共夹具：树内两账 max=184；名册含观澜
printf '| BAR-180 | x |\n' > "$T/bugs.md"
printf '{"bar":184}\n' > "$T/reg.jsonl"
mkdir -p "$T/mb"
printf '{"names":{"观澜":{"functions":["开发部"]}}}\n' > "$T/mb/roster.json"
run() { # run <额外env...> -- 跑领号，输出落 $T/out；registry 每轮重置防累积
    printf '{"bar":184}\n' > "$T/reg.jsonl"
    env BAR_NEW_BUGS="$T/bugs.md" BAR_NEW_REGISTRY="$T/reg.jsonl" \
        BAR_NEW_MAILBOX="$T/mb" BAR_NEW_NO_LETTER=1 "$@" \
        bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
}

# ①跨树可见（185 场景复跑）：信箱 H1 有 BAR-190，树内账只有 184 → 领 191
printf '# BAR-190 别线追踪信\n' > "$T/mb/0046号某人的通报.md"
run; chk "跨树可见(185复跑)" 0 $? "BAR-191 已领"

# ②信箱不可读拒领：mailbox 指向不存在目录 → exit 1（不许退回树内旧账）
env BAR_NEW_BUGS="$T/bugs.md" BAR_NEW_REGISTRY="$T/reg.jsonl" \
    BAR_NEW_MAILBOX="$T/不存在" BAR_NEW_NO_LETTER=1 \
    bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
chk "信箱不可读拒领" 1 $? "拒领"

# ③归档防回退：archive-v1 里老号信 BAR-195 → 领 196（号不许回退重发）
mkdir -p "$T/mb/archive-v1"; rm -f "$T/mb/0046号某人的通报.md"
printf '# BAR-195 已归档老信\n' > "$T/mb/archive-v1/0001号旧人的通报.md"
run; chk "归档防回退" 0 $? "BAR-196 已领"

# ④只认 H1 不认文件名：文件名带 BAR-999 陷阱、H1 无 BAR → 按树内账 184 领 185
rm -rf "$T/mb/archive-v1"
printf '# 普通信件标题\n' > "$T/mb/0099号某人BAR-999陷阱的通报.md"
run; chk "只认H1不认文件名" 0 $? "BAR-185 已领"

# ⑤名册预检：名册只有清和，观澜领号 → 拒；放回观澜 → 放行（①已证）
printf '{"names":{"清和":{"functions":["研究部"]}}}\n' > "$T/mb/roster.json"
run; chk "名不在册拒领" 1 $? "不在名册"
printf '{"names":{"观澜":{"functions":["开发部"]}}}\n' > "$T/mb/roster.json"
run; chk "在册放行" 0 $?

echo "[test-bar-new] $pass 过 / $fail 红"
[ "$fail" = 0 ]
