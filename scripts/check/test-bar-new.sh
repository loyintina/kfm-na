#!/usr/bin/env bash
# test-bar-new.sh — bar-new.sh 领号唯一源考题（BAR-189 0055 甲案三补丁；
# BAR-241 MAIN0135 裁决改口径：号源 = 号位登记簿 ∪ 信箱 H1，树内两账降投影）
# 夹具全走 BAR_NEW_* 测试口 + NO_LETTER=1（不碰真账真信箱）：
#   ①登记簿权威（只在登记簿的号不许重发——237 撞号事故复现，MAIN0135 §五.1）
#   ②登记簿不可读拒领（fail-closed）③双格式夹具（带/不带空格都被读到——
#     0135 楼1：registry 恒空病灶的转正判据）④roundtrip（领号落登记簿、
#     再领不重用——写读格式互咬）⑤写入格式钉死（换格式钉红，0135 楼2）
#   ⑥不跳号回归（两账同步 max 相同）⑦H1 次号不毒 max（BAR-192）
#   ⑧树内两账降投影（bugs.md 账行 BAR-999 不影响领号）
#   ⑨归档防回退 ⑩名册预检 ⑪源码守卫（BAR-217/226）
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

# 公共夹具：树内两账 max=184 且埋 >max 陷阱号（投影不许再当号源——
# 埋毒常证领号不受毒）；信箱名册含观澜；登记簿基线 190（带空格格式，
# 照生产真格式——0135 楼2：夹具与生产格式脱节 = 测试永绿生产瞎）。
printf '| BAR-180 | 修复（正文援引 BAR-998 陷阱号） |\n| BAR-999 | 账行首格毒弹 |\n' > "$T/bugs.md"
printf '{"bar": 184, "title": "参照 BAR-997 的修复"}\n' > "$T/reg.jsonl"
mkdir -p "$T/mb"
printf '{"names":{"观澜":{"functions":["开发部"]}}}\n' > "$T/mb/roster.json"
printf '{"bar": 190, "title": "基线", "claimant_func": "评审部", "claimant": "白露", "letter": "", "ts": "2026-10-09 09:00 +0800", "src": "存量"}\n' > "$T/ledger.jsonl"
# BAR_ABOUT 纯汉字短题（MAIN0135 楼6：号不进 about——JS gen 关于段守纯汉字，
# 号必含 ASCII 必卡全册 gen；号只在 H1 与登记簿）
run() { # run <额外env...> -- 跑领号，输出落 $T/out；投影每轮重置防累积
    printf '{"bar": 184, "title": "参照 BAR-997 的修复"}\n' > "$T/reg.jsonl"
    env BAR_NEW_LEDGER="$T/ledger.jsonl" BAR_NEW_REGISTRY="$T/reg.jsonl" \
        BAR_NEW_MAILBOX="$T/mb" BAR_NEW_NO_LETTER=1 BAR_ABOUT="考题短题" "$@" \
        bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
}

# ⑥不跳号回归（先行）：登记簿 190 + 信箱 H1 190 → 领 191
printf '# BAR-190 别线追踪信\n' > "$T/mb/0046号某人的通报.md"
run; chk "不跳号回归" 0 $? "BAR-191 已领"
# roundtrip 上半：刚才那领应把 191 落进登记簿（NO_LETTER 也登记）
grep -q '"bar": 191,' "$T/ledger.jsonl" \
    && { echo "  ✓ roundtrip-领号落登记簿"; pass=$((pass+1)); } \
    || { echo "  ✗ roundtrip-领号落登记簿（登记簿无 191 行）"; fail=$((fail+1)); }
# ⑤写入格式钉死：登记簿新行必须带 `"bar": N, ` 钉死形态（换格式钉红）
tail -1 "$T/ledger.jsonl" | grep -qE '^\{"bar": [0-9]+, "title": ".*", "claimant_func": ".*", "claimant": ".*", "letter": ".*", "ts": ".*", "src": "(开信|领号)"\}$' \
    && { echo "  ✓ 写入格式钉死"; pass=$((pass+1)); } \
    || { echo "  ✗ 写入格式钉死（实得: $(tail -1 "$T/ledger.jsonl" | head -c 60)…）"; fail=$((fail+1)); }
# ④roundtrip 下半：再领必须 192（不重用 191——写读格式互咬）
run; chk "roundtrip-再领不重用" 0 $? "BAR-192 已领"

# ①登记簿权威（237 撞号复现）：重置登记簿为「只有 240（模拟改号占号、
# 不开信、信箱 H1 无）」→ 领 241，绝不许 237-240 段重发
printf '{"bar": 240, "title": "改号占号无信", "claimant_func": "评审部", "claimant": "白露", "letter": "", "ts": "t", "src": "改号"}\n' > "$T/ledger.jsonl"
run; chk "登记簿权威(237事故复现)" 0 $? "BAR-241 已领"

# ②fail-closed：登记簿不可读（不存在）→ 拒领（信箱/树内账都在也不许领）
run_x() { # 显式 env 版（缺登记簿）
    env BAR_NEW_LEDGER="$T/不存在.jsonl" BAR_NEW_REGISTRY="$T/reg.jsonl" \
        BAR_NEW_MAILBOX="$T/mb" BAR_NEW_NO_LETTER=1 BAR_ABOUT="考题短题" \
        bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
}
run_x; chk "登记簿不可读拒领" 1 $? "拒领"
# 信箱不可读拒领（旧判据保留）
env BAR_NEW_LEDGER="$T/ledger.jsonl" BAR_NEW_REGISTRY="$T/reg.jsonl" \
    BAR_NEW_MAILBOX="$T/不存在" BAR_NEW_NO_LETTER=1 BAR_ABOUT="考题短题" \
    bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
chk "信箱不可读拒领" 1 $? "拒领"

# ③双格式夹具：紧凑行（无空格）238 + 带空格行 240 → 241（两格式都被读到；
# 只读带空格得 241 偶然也对——故先单跑紧凑行：只有紧凑 238 → 必须 239）
printf '{"bar":238,"title":"紧凑格式","claimant_func":"x","claimant":"x","letter":"","ts":"t","src":"补登"}\n' > "$T/ledger.jsonl"
run; chk "双格式-紧凑行被读到" 0 $? "BAR-239 已领"
printf '{"bar":238,"title":"紧凑","claimant_func":"x","claimant":"x","letter":"","ts":"t","src":"补登"}\n{"bar": 240, "title": "带空格", "claimant_func": "x", "claimant": "x", "letter": "", "ts": "t", "src": "改号"}\n' > "$T/ledger.jsonl"
run; chk "双格式-两格式同账取max" 0 $? "BAR-241 已领"

# ⑦H1 次号不毒 max（BAR-192）：H1 首号 190 正文援引 996 → 按登记簿 240 领 241
# （run 每次成功都落登记簿——重置登记簿防上轮累积毒期望）
printf '{"bar": 240, "title": "改号占号无信", "claimant_func": "评审部", "claimant": "白露", "letter": "", "ts": "t", "src": "改号"}\n' > "$T/ledger.jsonl"
printf '# BAR-190 别线信（正文援引 BAR-996 旧案）\n' > "$T/mb/0046号某人的通报.md"
run; chk "H1次号不毒max" 0 $? "BAR-241 已领"

# ⑧树内两账降投影：bugs.md 账行首格 BAR-999（公共夹具已埋）+ registry
# 184——领号全按登记簿/H1 max+1，投影毒弹无效（上面各 run 期望号已常证）；
# 投影面仍须回写（登记即占有的树内投影）——上轮领的 241 须在
grep -q '"bar": 241,' "$T/reg.jsonl" \
    && { echo "  ✓ 投影仍回写"; pass=$((pass+1)); } \
    || { echo "  ✗ 投影仍回写（registry 缺新行）"; fail=$((fail+1)); }

# ⑨归档防回退：登记簿清 240、archive-v1 老号信 BAR-195 → 领 196
printf '' > "$T/ledger.jsonl"; rm -f "$T/mb/0046号某人的通报.md"
mkdir -p "$T/mb/archive-v1"
printf '# BAR-195 已归档老信\n' > "$T/mb/archive-v1/0001号旧人的通报.md"
run; chk "归档防回退" 0 $? "BAR-196 已领"
rm -rf "$T/mb/archive-v1"

# ⑩名册预检：名册只有清和 → 观澜拒领；放回观澜 → 放行
# （信箱扫描面须有信——归档夹具已拆，补一封普通信）
printf '{"bar": 190, "title": "基线", "claimant_func": "x", "claimant": "x", "letter": "", "ts": "t", "src": "存量"}\n' > "$T/ledger.jsonl"
printf '# 普通信件标题\n' > "$T/mb/0099号某人的通报.md"
printf '{"names":{"清和":{"functions":["研究部"]}}}\n' > "$T/mb/roster.json"
run; chk "名不在册拒领" 1 $? "不在名册"
printf '{"names":{"观澜":{"functions":["开发部"]}}}\n' > "$T/mb/roster.json"
run; chk "在册放行" 0 $?

# ⑫ABOUT 闸（MAIN0135 楼5/6：号不进 about、缺则拒——77e0034 ABOUT_OPT
# 字面引号急件的转正判据；缺 ABOUT 或含 ASCII（如 BAR 号）一律拒领，
# 别等 mailbox-cli/gen 才炸）
env BAR_NEW_LEDGER="$T/ledger.jsonl" BAR_NEW_REGISTRY="$T/reg.jsonl" \
    BAR_NEW_MAILBOX="$T/mb" BAR_NEW_NO_LETTER=1 \
    bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
chk "缺ABOUT拒领" 1 $? "缺关于段"
env BAR_NEW_LEDGER="$T/ledger.jsonl" BAR_NEW_REGISTRY="$T/reg.jsonl" \
    BAR_NEW_MAILBOX="$T/mb" BAR_NEW_NO_LETTER=1 BAR_ABOUT="修复BAR240" \
    bash "$BARNEW" --func 开发部 --name 观澜 "夹具主题" > "$T/out" 2>&1
chk "ABOUT含ASCII拒领" 1 $? "纯汉字"

# ⑪a BAR-217 源码守卫：MAILBOX 与 DEFAULT_MAILBOX 必须同经 readlink -f
# 归一——2026-10-01 病灶：只归一 MAILBOX 后与未归一 DEFAULT 比字符串
# 恒假，「= 默认信箱才自动 commit」闸永不进，追踪信 untracked 漂流
# （0138/0143/0144 三封实证）。变异：摘 DEFAULT 归一行 → 本咬红。
grep -q 'DEFAULT_MAILBOX="$(readlink -f "$DEFAULT_MAILBOX")"' scripts/bar-new.sh \
    && { echo "  ✓ BAR-217 DEFAULT同归一"; pass=$((pass+1)); } \
    || { echo "  ✗ BAR-217 DEFAULT同归一（bar-new.sh 缺 DEFAULT_MAILBOX readlink 归一）"; fail=$((fail+1)); }

# ⑪b BAR-226 源码守卫：commit 闸的册在仓探测必须用 git rev-parse——
# 册并入设施仓后字面 .git 探测恒假 → 追踪信静默漂流（0154/0155 实证）。
# 变异：闸条件改回字面 .git 探测 → 本咬双红。
grep -q 'git -C "$MAILBOX" rev-parse --git-dir >/dev/null 2>&1' scripts/bar-new.sh \
    && { echo "  ✓ BAR-226 闸用rev-parse探仓"; pass=$((pass+1)); } \
    || { echo "  ✗ BAR-226 闸用rev-parse探仓（bar-new.sh commit 闸缺 git -C rev-parse 探测）"; fail=$((fail+1)); }
# 负向咬收窄到闸行（注释里引旧闸形态记录病灶是合法的，全文件 grep 会误伤）
grep 'git -C "$MAILBOX" rev-parse' scripts/bar-new.sh | grep -q '\[ -d "$MAILBOX/\.git" \]' \
    && { echo "  ✗ BAR-226 字面.git探测回潮"; fail=$((fail+1)); } \
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
