#!/usr/bin/env bash
# bar-new.sh — BAR 号唯一领取口（2026-09-29 用户拍板：与信箱领号制同构，
# 治病 = 「grep 空号再写账」的 TOCTOU 竞态——三线共享工作树，grep 与落账
# 之间隔一个工单周期，两线同小时各 grep 出同一空号即双号）。
#
# BAR-189（0055 裁决甲案+三补丁）：领号唯一源扶正**信箱**——树内两账
# （bugs.md/registry）在分支流下树-local，登记到落 master 隔一个工单周期
# （185 双号实案 0053）；信箱开信即 commit 小仓 = 唯一跨线即时可见账。
# 领号扫描 = 三账取 max+1：bugs.md + registry + 信箱全量信件 H1 的
# BAR-NNN（含 archive-v1/ 等归档目录——补丁①：老号归档 max 不许回退；
# 口径只认 H1 首行不认文件名——0054 闻灯：让号改号只改 H1）。
# 补丁②：信箱不可读 = 拒领，不许静默退回树内旧账。补丁③：名册预检提到
# 领号前（§六 名字登记前移后开信 fail-closed，预检杜绝「号领了信生不出」）。
#
# 一次原子完成四件事（flock 包裹，同刻只许一个领号者）：
#   1. 领号：max(bugs.md, bar-registry.jsonl, 信箱全量 H1) + 1
#   2. 登记：docs/ledger/bar-registry.jsonl 追加一行（登记即占有）
#   3. 开追踪信：mailbox-cli new（致全体/通报/待落地通报）+ 当场填白话结论
#      与立案节（不留占位——chain 第 4 步信箱执法常绿，不挡别线的闸）
#   4. 回写：信号（NNNN）补入登记行；信箱小仓自动 commit
#
# 用法: scripts/bar-new.sh --func 开发部 --name 观澜 "主题一句话"
#   主题禁含双引号与反斜杠（jsonl 转义从简）。
#
# 夹具测试口（test-bar-new.sh 用，正常领号别设）：
#   BAR_NEW_BUGS=路径      替代 bugs.md 扫号源
#   BAR_NEW_REGISTRY=路径  替代 bar-registry.jsonl（锁取同目尾加 .lock）
#   BAR_NEW_NO_LETTER=1    跳过开信（只领号+登记；信箱扫描仍执行）
#   BAR_NEW_MAILBOX=路径   替代信箱目录（扫描源/名册/new 指向它）
set -euo pipefail

FUNC="" NAME="" TITLE=""
while [ $# -gt 0 ]; do
    case "$1" in
        --func) FUNC="$2"; shift 2 ;;
        --name) NAME="$2"; shift 2 ;;
        --*) echo "未知参数: $1" >&2; exit 2 ;;
        *) TITLE="$1"; shift ;;
    esac
done
[ -n "$FUNC" ] && [ -n "$NAME" ] && [ -n "$TITLE" ] || {
    echo "用法: scripts/bar-new.sh --func <职能> --name <名字> \"主题\"" >&2; exit 2; }
case "$TITLE" in *[\"\\]*) echo "主题禁含双引号/反斜杠" >&2; exit 2 ;; esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUGS="${BAR_NEW_BUGS:-$ROOT/docs/ledger/bugs.md}"
REG="${BAR_NEW_REGISTRY:-$ROOT/docs/ledger/bar-registry.jsonl}"
LOCK="$REG.lock"
CLI="$ROOT/target/debug/mailbox-cli"
MAILBOX="${BAR_NEW_MAILBOX:-/root/.kfm/session/信箱}"

# 名册预检（0055 补丁③，提到领号前）：§六《名字登记前移》后开信对未入册名
# fail-closed——不预检会留下「号领了、信生不出」的坑。名册出处 = 信箱
# roster.json → 主册 roster.json；两处都没有 = 无法校验（双环境手机端），
# 告警放行。查到名册而名不在册 = 拒领。
ROSTER="$MAILBOX/roster.json"
[ -f "$ROSTER" ] || ROSTER="/root/kfmv4/docs/ledger/agent-inbox/roster.json"
if [ -f "$ROSTER" ]; then
    grep -q "\"$NAME\"" "$ROSTER" || {
        echo "名字 $NAME 不在名册（$ROSTER）——先入册再领号（信箱契约 §六）" >&2; exit 1; }
else
    echo "警告：名册两处皆无，名字在册预检跳过（双环境告警）" >&2
fi

exec 9>"$LOCK"
flock 9

MAX=0
for f in "$BUGS" "$REG"; do
    [ -f "$f" ] || continue
    # 两种账面都扫：bugs.md 的 BAR-NNN 与 registry 的 "bar":NNN；
    # 零命中是合法态（首领/纯一方有号），grep 空果不许触发 pipefail
    n=$(grep -oE '"bar":[0-9]+|BAR-[0-9]+' "$f" | grep -oE '[0-9]+' | sort -n | tail -1 || true)
    if [ -n "$n" ] && [ "$n" -gt "$MAX" ]; then MAX=$n; fi
done

# 第三账 = 领号唯一源扶正（0055 甲案）：信箱全量信件 **H1** 的 BAR-NNN。
# 树内两账（bugs.md/registry）在分支流下是树-local——登记到落 master 隔一个
# 工单周期，跨树领号互不可见（185 双号实案 0053）；信箱开信即 commit 小仓，
# 是唯一跨线即时可见的账。口径 = H1 首行（闻灯 0054：文件名出生冻结，
# 让号改号只改 H1——扫文件名会复活旧号）。含归档目录（0055 补丁①：
# 老号信归档后 max 不许回退，否则等于把抽过的号再发一次）。
# 补丁②：信箱不可读 = 拒领，不许静默退回树内旧账（那正是用了过期账的病灶）。
# 扫描无条件执行（NO_LETTER 只跳过开信，不跳过领号账——夹具请配 BAR_NEW_MAILBOX）。
[ -d "$MAILBOX" ] || {
    echo "信箱不可读：$MAILBOX——拒领（0055 补丁②，不许退回树内旧账）" >&2; exit 1; }
while IFS= read -r n; do
    [ -n "$n" ] && [ "$n" -gt "$MAX" ] && MAX=$n
done < <(find "$MAILBOX" -name '*.md' -not -path '*/.git/*' -exec head -q -n 1 {} + 2>/dev/null \
    | grep -oE 'BAR-[0-9]+' | grep -oE '[0-9]+' || true)
NEXT=$((MAX + 1))

LETTER=""
if [ "${BAR_NEW_NO_LETTER:-}" != "1" ]; then
    [ -x "$CLI" ] || { echo "缺 $CLI——先 cargo build -p mailbox-cli" >&2; exit 1; }
    NOW=$(date '+%Y-%m-%d %H:%M %z')
    TODAY=$(date '+%Y-%m-%d')
    out=$("$CLI" new --mailbox "$MAILBOX" --from-func "$FUNC" --from-name "$NAME" \
        --to-all --type 通报 --title "BAR-$NEXT $TITLE" \
        --kind 通报 --expect "承办线按 AGENTS.md「BAR 追踪信」条款同信追加进展" \
        --criteria "BAR-$NEXT 用户终验结案（信头状态翻 已验证）" \
        --status 待落地通报)
    file=$(printf '%s' "$out" | sed -n 's/.*已生成 \([^ ]*\.md\).*/\1/p')
    [ -n "$file" ] || { echo "mailbox-cli new 输出解析失败: $out" >&2; exit 1; }
    # 2026-09-29 评审修订（白露）：不写本册自指码——契约 §二「分拣码…本册信一律不写，
    # 只在跨册引用时写全码」。原行给编号无条件加 NA 前缀，致使 registry 的 letter 字段
    # 与提交信息的「追踪信 NA00NN」指向并不存在的文件名（NA0015 即此误用的成品）。
    # 新写法剥掉可选分拣码，取纯流水号（`0016号…` 与 `NA0016号…` 都归 `0016`）。
    LETTER=$(printf '%s' "$file" | sed -n 's/^\([A-Z]\{0,4\}\)\([0-9]\{4\}\).*/\2/p')
    path="$MAILBOX/$file"
    plain="本信追踪的问题是：$TITLE。当前状态 = 已立案开工，承办 $FUNC$NAME；立案/定罪/修复/判卷/终验都在本信追加（规矩 = na 仓 AGENTS.md「BAR 追踪信」节），隐藏读者看最新一节即可，无需做任何事。"
    body="### §一 立案（$NOW）\n\n- 主题：$TITLE\n- 承办：$FUNC$NAME\n- 定罪/修复/判卷/终验自下一节起按日期追加；信头状态随生命周期翻转\n  （待落地通报 → 已落地 → 已验证），打回翻回 待落地通报。\n\n——$FUNC$NAME · $TODAY"
    sed -i "s|（待填：面向隐藏读者。三句话内说清是什么事、要不要他做事；不写工作术语。）|$plain|" "$path"
    sed -i "s|（待填：结论与推导一起写——精确留给承重的数字和名字，其余白话；每条发现标「明写／推断／补全」。）|$body|" "$path"
    grep -q '（待填' "$path" && { echo "占位未填尽: $path" >&2; exit 1; }
    "$CLI" verify --mailbox "$MAILBOX" "$path" >/dev/null
    "$CLI" gen --mailbox "$MAILBOX" >/dev/null
    if [ "$MAILBOX" = "/root/.kfm/session/信箱" ] && [ -d "$MAILBOX/.git" ]; then
        (cd "$MAILBOX" && git add "$file" README.md letters-index.jsonl letter-tokens.jsonl \
            && git -c user.name=kfm-na -c user.email=na@kfm.local commit -q \
               -m "feat(信箱): $LETTER BAR-$NEXT 追踪信开卷（$FUNC$NAME：$TITLE）" </dev/null)
    fi
fi

TS=$(date '+%Y-%m-%d %H:%M %z')
printf '{"bar":%d,"title":"%s","claimant_func":"%s","claimant":"%s","letter":"%s","ts":"%s"}\n' \
    "$NEXT" "$TITLE" "$FUNC" "$NAME" "$LETTER" "$TS" >> "$REG"

echo "BAR-$NEXT 已领（$FUNC$NAME：$TITLE）${LETTER:+；追踪信 $LETTER}"
