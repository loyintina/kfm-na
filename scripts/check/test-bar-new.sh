#!/usr/bin/env bash
# test-bar-new.sh — bar-new.sh 领号唯一源考题（BAR-189，0055 甲案三补丁，挂 chain）
# 六夹具全走 BAR_NEW_* 测试口 + NO_LETTER=1（不碰真账真信箱）：
#   ①跨树可见（185 场景复跑）②信箱不可读拒领 ③归档防回退 ④只认 H1 不认文件名
#   ⑤名册预检（名不在册拒领 / 在册放行）⑥prose 毒免疫（BAR-192）
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

# 公共夹具：树内两账 max=184；名册含观澜。
# BAR-192：两账 prose 各埋一个 >max 陷阱号（bugs 行正文 BAR-998 / registry
# title 援引 BAR-997）——每轮领号都带毒跑，期望号全不受毒即免疫常证。
printf '| BAR-180 | 修复（正文援引 BAR-998 陷阱号） |\n' > "$T/bugs.md"
printf '{"bar":184,"title":"参照 BAR-997 的修复"}\n' > "$T/reg.jsonl"
mkdir -p "$T/mb"
printf '{"names":{"观澜":{"functions":["开发部"]}}}\n' > "$T/mb/roster.json"
run() { # run <额外env...> -- 跑领号，输出落 $T/out；registry 每轮重置防累积
    printf '{"bar":184,"title":"参照 BAR-997 的修复"}\n' > "$T/reg.jsonl"
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

# ⑥prose 毒免疫（BAR-192）：H1 标题正文援引次号 BAR-996（>真 max）——
# H1 只认首号（本信号 190），次号不许毒 max → 仍领 191。
# （bugs.md/registry 的 prose 毒免疫由公共夹具埋弹、①~⑤期望号常证。）
printf '# BAR-190 别线信（正文援引 BAR-996 旧案）\n' > "$T/mb/0046号某人的通报.md"
run; chk "H1次号不毒max" 0 $? "BAR-191 已领"

# ⑦BAR-217 源码守卫：MAILBOX 与 DEFAULT_MAILBOX 必须同经 readlink -f
# 归一——2026-10-01 病灶：只归一 MAILBOX 后与未归一 DEFAULT 比字符串
# 恒假，「= 默认信箱才自动 commit」闸永不进，追踪信 untracked 漂流
# （0138/0143/0144 三封实证）。变异：摘 DEFAULT 归一行 → 本咬红。
grep -q 'DEFAULT_MAILBOX="$(readlink -f "$DEFAULT_MAILBOX")"' scripts/bar-new.sh \
    && { echo "  ✓ BAR-217 DEFAULT同归一"; pass=$((pass+1)); } \
    || { echo "  ✗ BAR-217 DEFAULT同归一（bar-new.sh 缺 DEFAULT_MAILBOX readlink 归一）"; fail=$((fail+1)); }

# ⑧BAR-226 源码守卫：自动 commit 闸的册在仓探测必须用 git rev-parse——
# 2026-10-02 病灶：na 册并入设施仓（90-信箱 单仓两册）后 .git 在设施仓根，
# 册子目录无字面 .git 目录，旧闸 [ -d "$MAILBOX/.git" ] 恒假 → 追踪信开了
# 不落盘、静默漂流（0154 领号后 untracked 31 分钟/0155 至发现时仍漂流）。
# 变异：闸条件改回字面 .git 探测 → 本咬双红。
grep -q 'git -C "$MAILBOX" rev-parse --git-dir >/dev/null 2>&1' scripts/bar-new.sh \
    && { echo "  ✓ BAR-226 闸用rev-parse探仓"; pass=$((pass+1)); } \
    || { echo "  ✗ BAR-226 闸用rev-parse探仓（bar-new.sh 自动 commit 闸缺 git -C rev-parse 探测）"; fail=$((fail+1)); }
# 负向咬收窄到闸行（注释里引旧闸形态记录病灶是合法的，全文件 grep 会误伤）
grep '\[ "$MAILBOX" = "$DEFAULT_MAILBOX" \]' scripts/bar-new.sh | grep -q '\[ -d "$MAILBOX/\.git" \]' \
    && { echo "  ✗ BAR-226 字面.git探测回潮（册并入设施仓后恒假 = 静默丢数据）"; fail=$((fail+1)); } \
    || { echo "  ✓ BAR-226 字面.git探测不在闸行"; pass=$((pass+1)); }

# ⑨自动提交走 letters commit（MAIN0123 §五 残留②）：裸 git -c user.name=kfm-na
# 提交会被设施门④拒（作者≠信封作者），拒后暂存区滞留还挡全仓发楼（MAIN0113
# 习性：暂存区有主即拒）。变异：退回裸 git commit 形态 → 负向咬红。
grep -q 'LETTERS_BIN=' scripts/bar-new.sh \
    && { echo "  ✓ 自动提交走letters"; pass=$((pass+1)); } \
    || { echo "  ✗ 自动提交走letters（bar-new.sh 缺 LETTERS_BIN 兜底）"; fail=$((fail+1)); }
grep -q 'user\.name=kfm-na' scripts/bar-new.sh \
    && { echo "  ✗ 裸git署名回潮（撞门④滞留暂存区）"; fail=$((fail+1)); } \
    || { echo "  ✓ 裸git署名不在"; pass=$((pass+1)); }

# ⑩CLI 缺件自愈（0126 9楼观澜）：清主仓 target 连坐领号器前置件——
# bar-new.sh 必须自带重建，不许停在「谁记得补建」。变异：摘自愈块 → 红。
grep -q 'mailbox-cli 缺（清 target 后遗症）' scripts/bar-new.sh \
    && { echo "  ✓ CLI缺件自愈在"; pass=$((pass+1)); } \
    || { echo "  ✗ CLI缺件自愈缺（清 target 后领号全瘫）"; fail=$((fail+1)); }

# ⑪--expect 带主语带事由（白露加急 2026-10-07）：看板事由列直读 expect
# 字段——纯样板文（无承办者无事由）四封信一个脸。变异：退回样板 → 红。
grep -q -- '--expect "承办线（\$FUNC\$NAME）以楼层追加进展：\$TITLE"' scripts/bar-new.sh \
    && { echo "  ✓ expect带主语带事由"; pass=$((pass+1)); } \
    || { echo "  ✗ expect退回纯样板（看板事由列污染回潮）"; fail=$((fail+1)); }

echo "[test-bar-new] $pass 过 / $fail 红"
[ "$fail" = 0 ]
